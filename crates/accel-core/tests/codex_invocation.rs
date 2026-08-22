//! Does the command Accelerate builds actually run?
//!
//! Ignored by default: needs the `codex` CLI and a local model server. To run:
//!
//! ```text
//! cargo test --test codex_invocation -- --ignored --nocapture
//! ```
//!
//! `CODEX_MODEL` and `CODEX_HOST` override the model and server address.

use accel_core::config::{ModelProfile, RunnerKind};
use accel_core::runner::{build_command, run_agent, AgentEvent, CancelToken, Completion};
use std::collections::BTreeMap;
use std::path::Path;
use tokio::sync::mpsc;

fn profile(runner: RunnerKind) -> ModelProfile {
    ModelProfile {
        id: "codex".into(),
        name: "Codex".into(),
        runner,
        model: Some(std::env::var("CODEX_MODEL").unwrap_or_else(|_| "qwen3:8b".into())),
        extra_args: Vec::new(),
        env: BTreeMap::new(),
        timeout_secs: Some(120),
        autonomous: true,
    }
}

async fn run(profile: &ModelProfile, prompt: &str) -> (Completion, Vec<AgentEvent>) {
    let command = build_command(profile, prompt);
    eprintln!("running: {}", command.display(prompt));

    let (tx, mut rx) = mpsc::channel(256);
    let collected = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    });

    let outcome = run_agent(
        command,
        prompt,
        Path::new("."),
        Some(std::time::Duration::from_secs(120)),
        CancelToken::new(),
        tx,
    )
    .await
    .expect("codex should spawn — is it on PATH?");

    (outcome.completion, collected.await.unwrap())
}

/// The regression this exists for: `codex exec` rejects `--full-auto`, so a run
/// died with "unexpected argument '--full-auto' found" before doing any work.
#[tokio::test]
#[ignore = "needs the codex CLI and a local model server"]
async fn a_local_model_runs_without_an_argument_error() {
    let host = std::env::var("CODEX_HOST").ok();
    let (completion, events) =
        run(&profile(RunnerKind::CodexOss { base_url: host }), "say hi").await;

    let complaints: Vec<&AgentEvent> = events
        .iter()
        .filter(|event| match event {
            AgentEvent::Error { message } => message.contains("unexpected argument"),
            AgentEvent::Raw { text } => text.contains("unexpected argument"),
            _ => false,
        })
        .collect();
    assert!(
        complaints.is_empty(),
        "codex rejected an argument: {complaints:?}"
    );

    assert_eq!(
        completion,
        Completion::Succeeded,
        "run did not complete: {events:#?}"
    );

    // The output must have parsed into events, not arrived as raw JSON lines.
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Text { .. })),
        "expected the agent's message to be parsed: {events:#?}"
    );
}
