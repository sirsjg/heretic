//! Spawning and supervising an agent process.
//!
//! Agent CLIs spawn children of their own (shells, language servers, test
//! runners), so cancellation has to reach the whole process tree — killing only
//! the direct child leaves orphans holding the worktree open. On Unix we put each
//! agent in its own process group and signal the group.

use super::command::AgentCommand;
use super::stream::{parse_line, AgentEvent, OutputFormat};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Notify};

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completion {
    /// Exited zero.
    Succeeded,
    /// Exited non-zero.
    Failed,
    /// Stopped because someone asked it to stop.
    Cancelled,
    /// Ran past its configured time limit.
    TimedOut,
}

/// The outcome of one agent invocation.
#[derive(Debug, Clone)]
pub struct AgentOutcome {
    pub completion: Completion,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    /// Everything the agent emitted, in order.
    pub transcript: Vec<AgentEvent>,
}

impl AgentOutcome {
    pub fn succeeded(&self) -> bool {
        self.completion == Completion::Succeeded
    }

    /// The agent's own closing summary, when it produced one; otherwise the last
    /// thing it said.
    pub fn final_message(&self) -> Option<String> {
        self.transcript
            .iter()
            .rev()
            .find_map(|event| match event {
                AgentEvent::Result { text, .. } => text.clone(),
                _ => None,
            })
            .or_else(|| {
                self.transcript.iter().rev().find_map(|event| match event {
                    AgentEvent::Text { text } => Some(text.clone()),
                    _ => None,
                })
            })
    }

    /// The last `limit` lines of the transcript, for Flux comments and error reports.
    pub fn tail(&self, limit: usize) -> String {
        let lines: Vec<String> = self
            .transcript
            .iter()
            .map(|event| event.summary())
            .filter(|line| !line.trim().is_empty())
            .collect();
        let start = lines.len().saturating_sub(limit);
        lines[start..].join("\n")
    }
}

/// A cooperative stop signal, shared between the UI and a running agent.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the run to stop. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolves once cancelled. Returns immediately if already cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        // A waiter registered before the check avoids missing a concurrent cancel.
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// Run an agent to completion, forwarding every event to `sink` as it happens.
///
/// The returned outcome always contains the full transcript, even when the run
/// was cancelled or timed out.
pub async fn run_agent(
    command: AgentCommand,
    prompt: &str,
    working_dir: &Path,
    timeout: Option<Duration>,
    cancel: CancelToken,
    sink: mpsc::Sender<AgentEvent>,
) -> std::io::Result<AgentOutcome> {
    let started = Instant::now();

    let mut process = tokio::process::Command::new(&command.program);
    process
        .args(&command.args)
        .current_dir(working_dir)
        .stdin(if command.prompt_via_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    for (key, value) in &command.env {
        process.env(key, value);
    }

    // Own process group, so cancellation can reach the agent's children.
    #[cfg(unix)]
    process.process_group(0);

    let mut child = process.spawn()?;
    let pid = child.id();

    if command.prompt_via_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await?;
        }
    }

    // Merge stdout and stderr into one ordered event stream.
    let (line_tx, mut line_rx) = mpsc::channel::<(bool, String)>(256);
    if let Some(stdout) = child.stdout.take() {
        let tx = line_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send((false, line)).await.is_err() {
                    break;
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = line_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send((true, line)).await.is_err() {
                    break;
                }
            }
        });
    }
    drop(line_tx);

    let mut transcript: Vec<AgentEvent> = Vec::new();
    let mut cancelled = false;
    let mut timed_out = false;

    let deadline = timeout.map(|limit| Instant::now() + limit);
    let exit_status = loop {
        let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if remaining == Some(Duration::ZERO) {
            timed_out = true;
            stop_process_tree(pid, true).await;
            break child.wait().await.ok();
        }

        tokio::select! {
            // Bias towards draining output so a fast-exiting process still
            // reports everything it printed.
            biased;

            line = line_rx.recv() => match line {
                Some((is_stderr, text)) => {
                    // stderr is never structured, whatever the backend is.
                    let format = if is_stderr {
                        OutputFormat::Plain
                    } else {
                        command.output
                    };
                    if let Some(event) = parse_line(&text, format) {
                        let _ = sink.send(event.clone()).await;
                        transcript.push(event);
                    }
                }
                None => break child.wait().await.ok(),
            },

            _ = cancel.cancelled(), if !cancelled => {
                cancelled = true;
                let notice = AgentEvent::Raw { text: "Stopping agent…".into() };
                let _ = sink.send(notice.clone()).await;
                transcript.push(notice);
                stop_process_tree(pid, false).await;
            }

            _ = sleep_until(deadline) => {
                timed_out = true;
                stop_process_tree(pid, true).await;
                break child.wait().await.ok();
            }
        }
    };

    // Drain anything still buffered after the process exited.
    while let Ok((is_stderr, text)) = line_rx.try_recv() {
        let format = if is_stderr {
            OutputFormat::Plain
        } else {
            command.output
        };
        if let Some(event) = parse_line(&text, format) {
            let _ = sink.send(event.clone()).await;
            transcript.push(event);
        }
    }

    let exit_code = exit_status.and_then(|status| status.code());
    let completion = if cancelled {
        Completion::Cancelled
    } else if timed_out {
        Completion::TimedOut
    } else if exit_code == Some(0) {
        Completion::Succeeded
    } else {
        Completion::Failed
    };

    Ok(AgentOutcome {
        completion,
        exit_code,
        duration: started.elapsed(),
        transcript,
    })
}

/// Resolves at `deadline`, or never when there is no deadline.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(instant) => tokio::time::sleep_until(tokio::time::Instant::from_std(instant)).await,
        None => std::future::pending::<()>().await,
    }
}

/// Signal an agent's whole process group: SIGINT first so it can clean up, then
/// SIGKILL if it is still alive a few seconds later.
async fn stop_process_tree(pid: Option<u32>, force: bool) {
    let Some(pid) = pid else { return };

    #[cfg(unix)]
    {
        let group = pid as i32;
        let signal = if force { libc::SIGKILL } else { libc::SIGINT };
        // Safety: killpg on our own child's process group.
        unsafe {
            libc::killpg(group, signal);
        }

        if !force {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                unsafe {
                    libc::killpg(group, libc::SIGKILL);
                }
            });
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (pid, force);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn shell_command(script: &str) -> AgentCommand {
        AgentCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: BTreeMap::new(),
            prompt_via_stdin: false,
            output: OutputFormat::Plain,
        }
    }

    async fn run(
        command: AgentCommand,
        cancel: CancelToken,
        timeout: Option<Duration>,
    ) -> AgentOutcome {
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        run_agent(command, "", Path::new("."), timeout, cancel, tx)
            .await
            .expect("agent should spawn")
    }

    #[tokio::test]
    async fn a_successful_run_captures_its_output() {
        let outcome = run(
            shell_command("echo hello; echo world"),
            CancelToken::new(),
            None,
        )
        .await;

        assert_eq!(outcome.completion, Completion::Succeeded);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.tail(10).contains("hello"));
        assert!(outcome.tail(10).contains("world"));
    }

    #[tokio::test]
    async fn a_failing_run_is_reported_as_failed() {
        let outcome = run(shell_command("exit 3"), CancelToken::new(), None).await;
        assert_eq!(outcome.completion, Completion::Failed);
        assert_eq!(outcome.exit_code, Some(3));
    }

    #[tokio::test]
    async fn stderr_is_captured_alongside_stdout() {
        let outcome = run(
            shell_command("echo to-stderr 1>&2; exit 0"),
            CancelToken::new(),
            None,
        )
        .await;
        assert!(outcome.tail(10).contains("to-stderr"));
    }

    #[tokio::test]
    async fn cancelling_stops_a_long_running_agent() {
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            trigger.cancel();
        });

        let outcome = run(shell_command("sleep 30"), cancel, None).await;
        assert_eq!(outcome.completion, Completion::Cancelled);
    }

    #[tokio::test]
    async fn a_run_past_its_limit_times_out() {
        let outcome = run(
            shell_command("sleep 30"),
            CancelToken::new(),
            Some(Duration::from_millis(200)),
        )
        .await;
        assert_eq!(outcome.completion, Completion::TimedOut);
    }

    #[tokio::test]
    async fn the_prompt_can_be_delivered_on_stdin() {
        let mut command = shell_command("cat");
        command.prompt_via_stdin = true;

        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let outcome = run_agent(
            command,
            "prompt-from-stdin",
            Path::new("."),
            None,
            CancelToken::new(),
            tx,
        )
        .await
        .unwrap();

        assert_eq!(outcome.completion, Completion::Succeeded);
        assert!(outcome.tail(5).contains("prompt-from-stdin"));
    }

    #[tokio::test]
    async fn a_cancel_before_the_run_starts_is_still_honoured() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let outcome = run(shell_command("sleep 30"), cancel, None).await;
        assert_eq!(outcome.completion, Completion::Cancelled);
    }
}
