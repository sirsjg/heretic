//! The run state machine: plan, implement, review, revise, document, integrate.
//!
//! Written against the traits in [`super::types`], so the whole flow is testable
//! without Flux, git, or a real agent.

use super::types::{
    AgentExecutor, Board, RunFeedItem, RunOutcome, RunProgress, RunResult, RunStage, Workspace,
};
use crate::config::{ModelProfile, Pipeline, Role};
use crate::model::TaskStatus;
use crate::prompt::{self, TaskContext, Verdict};
use crate::runner::{AgentEvent, CancelToken, Completion};
use crate::worktree::ChangeSummary;
use std::collections::BTreeMap;
use tokio::sync::mpsc;

/// Which agent plays which role for this run.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub pipeline: Pipeline,
    pub profiles: BTreeMap<Role, ModelProfile>,
}

impl RunConfig {
    fn profile(&self, role: Role) -> Option<&ModelProfile> {
        self.profiles.get(&role)
    }
}

/// Run one task to completion.
///
/// Never panics and never returns early without leaving the board in a coherent
/// state: a task is either done, or back in Todo with a blocker explaining why.
pub async fn execute_run(
    context: TaskContext,
    config: RunConfig,
    board: &dyn Board,
    workspace: &dyn Workspace,
    executor: &dyn AgentExecutor,
    cancel: CancelToken,
    progress: mpsc::Sender<RunProgress>,
) -> RunOutcome {
    let task_id = context.task.id.clone();

    let Some(implementer) = config.profile(Role::Implementer).cloned() else {
        return finish(
            board,
            &task_id,
            None,
            RunResult::Failed {
                stage: RunStage::Preparing,
                reason: "No model is assigned to the Implementer role.".into(),
            },
            0,
            ChangeSummary::default(),
        )
        .await;
    };

    let implementer_name = implementer.agent_name(Role::Implementer);

    // Claim the task. A failure here means we are out of step with the board, so
    // stop rather than let an agent work a task someone else may have taken.
    note(&progress, "Claiming the task on the board").await;
    if let Err(error) = board
        .move_status(&task_id, TaskStatus::InProgress, Some(&implementer_name))
        .await
    {
        return finish(
            board,
            &task_id,
            None,
            RunResult::Failed {
                stage: RunStage::Preparing,
                reason: format!("Could not claim the task: {error}"),
            },
            0,
            ChangeSummary::default(),
        )
        .await;
    }
    let _ = board.set_blocked_reason(&task_id, None).await;

    // --- Planning -------------------------------------------------------------
    let mut brief: Option<String> = None;
    if config.pipeline.plan {
        match config.profile(Role::Orchestrator) {
            Some(planner) => {
                let prompt = prompt::planner_prompt(&context);
                match run_stage(
                    RunStage::Planning,
                    Role::Orchestrator,
                    planner,
                    &prompt,
                    workspace,
                    executor,
                    &cancel,
                    &progress,
                )
                .await
                {
                    StageResult::Ok(summary) => brief = summary,
                    StageResult::Cancelled => {
                        return cancelled(board, &task_id, 0, workspace).await
                    }
                    // A failed plan is not fatal: the implementer has the full
                    // task context regardless.
                    StageResult::Failed(reason) => {
                        note(&progress, &format!("Planning did not finish ({reason}); implementing from the task itself")).await;
                    }
                }
            }
            None => {
                note(
                    &progress,
                    "Planning is enabled but no model is assigned to the Orchestrator role; skipping it",
                )
                .await;
            }
        }
    }

    // --- Implement, review, revise -------------------------------------------
    let mut revisions: u32 = 0;
    let mut revision_notes: Option<String> = None;
    let mut implementer_summary = String::new();

    let outcome_result = loop {
        if cancel.is_cancelled() {
            return cancelled(board, &task_id, revisions, workspace).await;
        }

        let prompt =
            prompt::implementer_prompt(&context, brief.as_deref(), revision_notes.as_deref());

        match run_stage(
            RunStage::Implementing,
            Role::Implementer,
            &implementer,
            &prompt,
            workspace,
            executor,
            &cancel,
            &progress,
        )
        .await
        {
            StageResult::Ok(summary) => {
                implementer_summary = summary.unwrap_or_default();
            }
            StageResult::Cancelled => {
                return cancelled(board, &task_id, revisions, workspace).await
            }
            StageResult::Failed(reason) => {
                break RunResult::Failed {
                    stage: RunStage::Implementing,
                    reason,
                }
            }
        }

        if !config.pipeline.review {
            break RunResult::Completed;
        }

        // Nothing to review means nothing was done — never quietly pass that.
        let changes = workspace.summarise().await.unwrap_or_default();
        if changes.is_empty() {
            break RunResult::NeedsAttention {
                reason: "The implementer made no changes.".into(),
            };
        }

        let Some(reviewer) = config.profile(Role::Reviewer) else {
            note(
                &progress,
                "Review is enabled but no model is assigned to the Reviewer role; accepting the work unreviewed",
            )
            .await;
            break RunResult::Completed;
        };

        let diff = workspace.diff().await.unwrap_or_default();
        let review_prompt = prompt::reviewer_prompt(&context, &diff, &implementer_summary);

        let review_text = match run_stage(
            RunStage::Reviewing,
            Role::Reviewer,
            reviewer,
            &review_prompt,
            workspace,
            executor,
            &cancel,
            &progress,
        )
        .await
        {
            StageResult::Ok(summary) => summary.unwrap_or_default(),
            StageResult::Cancelled => {
                return cancelled(board, &task_id, revisions, workspace).await
            }
            StageResult::Failed(reason) => {
                break RunResult::NeedsAttention {
                    reason: format!("Review could not be completed: {reason}"),
                }
            }
        };

        match prompt::parse_verdict(&review_text) {
            Verdict::Approved => break RunResult::Completed,
            Verdict::Unclear => {
                break RunResult::NeedsAttention {
                    reason: "The reviewer did not give a clear verdict.".into(),
                }
            }
            Verdict::ChangesRequested => {
                if revisions >= config.pipeline.max_revisions {
                    break RunResult::NeedsAttention {
                        reason: format!(
                            "Review still requested changes after {} attempt(s).",
                            revisions + 1
                        ),
                    };
                }
                revisions += 1;
                let _ = progress
                    .send(RunProgress::RevisionRequested {
                        attempt: revisions,
                        notes: review_text.clone(),
                    })
                    .await;
                revision_notes = Some(review_text);
            }
        }
    };

    // --- Documentation --------------------------------------------------------
    if outcome_result.succeeded() && config.pipeline.document {
        match config.profile(Role::Documenter) {
            Some(documenter) => {
                let changes = workspace.summarise().await.unwrap_or_default();
                let change_text = describe_changes(&changes, &implementer_summary);
                let prompt = prompt::documenter_prompt(&context, &change_text);

                match run_stage(
                    RunStage::Documenting,
                    Role::Documenter,
                    documenter,
                    &prompt,
                    workspace,
                    executor,
                    &cancel,
                    &progress,
                )
                .await
                {
                    StageResult::Cancelled => {
                        return cancelled(board, &task_id, revisions, workspace).await
                    }
                    // Documentation is a nicety; its failure does not undo the work.
                    StageResult::Failed(reason) => {
                        note(
                            &progress,
                            &format!("Documentation pass did not finish: {reason}"),
                        )
                        .await;
                    }
                    StageResult::Ok(_) => {}
                }
            }
            None => {
                note(
                    &progress,
                    "Documentation is enabled but no model is assigned to the Documenter role; skipping it",
                )
                .await;
            }
        }
    }

    // --- Integration ----------------------------------------------------------
    let changes = workspace.summarise().await.unwrap_or_default();

    if outcome_result.succeeded() {
        let _ = progress
            .send(RunProgress::StageStarted {
                stage: RunStage::Integrating,
                role: None,
                agent: None,
            })
            .await;

        let message = format!("{}: {}", context.task.id, context.task.title);
        if let Err(error) = workspace.commit(&message).await {
            return finish(
                board,
                &task_id,
                Some(&implementer_name),
                RunResult::NeedsAttention {
                    reason: format!("The work is done but could not be committed: {error}"),
                },
                revisions,
                changes,
            )
            .await;
        }

        if let Err(error) = workspace.integrate().await {
            return finish(
                board,
                &task_id,
                Some(&implementer_name),
                RunResult::NeedsAttention {
                    reason: format!(
                        "The work is committed on its branch but could not be integrated: {error}"
                    ),
                },
                revisions,
                changes,
            )
            .await;
        }

        let _ = progress
            .send(RunProgress::StageFinished {
                stage: RunStage::Integrating,
                ok: true,
                summary: None,
            })
            .await;
    }

    finish(
        board,
        &task_id,
        Some(&implementer_name),
        outcome_result,
        revisions,
        changes,
    )
    .await
}

enum StageResult {
    Ok(Option<String>),
    Failed(String),
    Cancelled,
}

/// Run one agent stage, streaming its output into the progress channel.
#[allow(clippy::too_many_arguments)]
async fn run_stage(
    stage: RunStage,
    role: Role,
    profile: &ModelProfile,
    prompt_text: &str,
    workspace: &dyn Workspace,
    executor: &dyn AgentExecutor,
    cancel: &CancelToken,
    progress: &mpsc::Sender<RunProgress>,
) -> StageResult {
    let agent_name = profile.agent_name(role);
    let _ = progress
        .send(RunProgress::StageStarted {
            stage,
            role: Some(role),
            agent: Some(agent_name),
        })
        .await;

    let _ = progress
        .send(RunProgress::Command {
            stage,
            command: crate::runner::build_command(profile, prompt_text).display(prompt_text),
        })
        .await;

    // Forward the agent's events into the run feed as they arrive.
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
    let feed = progress.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let _ = feed
                .send(RunProgress::Output(RunFeedItem {
                    stage,
                    role: Some(role),
                    event,
                }))
                .await;
        }
    });

    let result = executor
        .execute(
            profile,
            role,
            prompt_text,
            workspace.path(),
            cancel.clone(),
            tx,
        )
        .await;
    let _ = forwarder.await;

    match result {
        Err(error) => {
            let reason = describe_spawn_failure(&profile.runner_program(), &error);
            let _ = progress
                .send(RunProgress::StageFinished {
                    stage,
                    ok: false,
                    summary: Some(reason.clone()),
                })
                .await;
            StageResult::Failed(reason)
        }
        Ok(outcome) => {
            let summary = outcome.final_message().or_else(|| {
                let tail = outcome.tail(20);
                (!tail.trim().is_empty()).then_some(tail)
            });

            let stage_result = match outcome.completion {
                Completion::Succeeded => StageResult::Ok(summary.clone()),
                Completion::Cancelled => StageResult::Cancelled,
                Completion::TimedOut => {
                    StageResult::Failed(format!("{} ran past its time limit.", profile.name))
                }
                Completion::Failed => StageResult::Failed(format!(
                    "{} exited with code {}. Last output:\n{}",
                    profile.name,
                    outcome
                        .exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".into()),
                    outcome.tail(15)
                )),
            };

            let _ = progress
                .send(RunProgress::StageFinished {
                    stage,
                    ok: matches!(stage_result, StageResult::Ok(_)),
                    summary,
                })
                .await;

            stage_result
        }
    }
}

/// Spawn failures are almost always "the CLI is not installed", so say that
/// rather than surfacing a bare OS error.
fn describe_spawn_failure(program: &str, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("`{program}` was not found on PATH. Install it, or point this model profile at a different command.")
    } else {
        format!("Could not start `{program}`: {error}")
    }
}

fn describe_changes(changes: &ChangeSummary, summary: &str) -> String {
    let mut text = String::new();
    if !summary.trim().is_empty() {
        text.push_str(summary.trim());
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "Files touched ({}): {}",
        changes.files_changed,
        if changes.files.is_empty() {
            "none".to_string()
        } else {
            changes.files.join(", ")
        }
    ));
    text
}

async fn note(progress: &mpsc::Sender<RunProgress>, message: &str) {
    let _ = progress
        .send(RunProgress::Note {
            message: message.to_string(),
        })
        .await;
}

async fn cancelled(
    board: &dyn Board,
    task_id: &str,
    revisions: u32,
    workspace: &dyn Workspace,
) -> RunOutcome {
    let changes = workspace.summarise().await.unwrap_or_default();
    finish(
        board,
        task_id,
        None,
        RunResult::Cancelled,
        revisions,
        changes,
    )
    .await
}

/// Leave the board in a coherent state and produce the run's outcome.
///
/// On anything other than success the task returns to Todo with a blocker
/// recorded, which both explains the situation on the Flux board and stops the
/// auto loop from picking the same task straight back up.
async fn finish(
    board: &dyn Board,
    task_id: &str,
    agent_name: Option<&str>,
    result: RunResult,
    revisions: u32,
    changes: ChangeSummary,
) -> RunOutcome {
    let summary = compose_summary(&result, revisions, &changes);

    let _ = board.comment(task_id, &summary, agent_name).await;

    if result.succeeded() {
        let _ = board
            .move_status(task_id, TaskStatus::Done, agent_name)
            .await;
    } else {
        let _ = board
            .move_status(task_id, TaskStatus::Todo, agent_name)
            .await;
        let _ = board
            .set_blocked_reason(task_id, Some(&blocker_text(&result)))
            .await;
    }

    RunOutcome {
        result,
        revisions,
        changes,
        summary,
    }
}

fn blocker_text(result: &RunResult) -> String {
    let detail = match result {
        RunResult::Cancelled => "stopped by request".to_string(),
        RunResult::Failed { stage, .. } => {
            format!("failed during {}", stage.label().to_lowercase())
        }
        RunResult::NeedsAttention { reason } => first_sentence(reason),
        RunResult::Completed => "completed".to_string(),
    };
    format!("Heretic: {detail}")
}

fn first_sentence(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.find(". ") {
        Some(index) => trimmed[..=index].trim().to_string(),
        None => trimmed.chars().take(140).collect(),
    }
}

/// The comment written back to Flux — the durable record of what happened.
fn compose_summary(result: &RunResult, revisions: u32, changes: &ChangeSummary) -> String {
    let mut out = format!("**Heretic — {}**\n\n", result.describe());

    if changes.is_empty() {
        out.push_str("No files were changed.\n");
    } else {
        out.push_str(&format!(
            "{} file(s) changed, +{} −{}.\n",
            changes.files_changed, changes.insertions, changes.deletions
        ));
        let shown: Vec<&String> = changes.files.iter().take(20).collect();
        for file in &shown {
            out.push_str(&format!("- `{file}`\n"));
        }
        if changes.files.len() > shown.len() {
            out.push_str(&format!(
                "- …and {} more\n",
                changes.files.len() - shown.len()
            ));
        }
    }

    if revisions > 0 {
        out.push_str(&format!(
            "\nReview sent the work back {revisions} time(s) before this outcome.\n"
        ));
    }

    if let RunResult::Failed { reason, .. } | RunResult::NeedsAttention { reason } = result {
        out.push_str(&format!("\n{}\n", reason.trim()));
    }

    out
}

impl ModelProfile {
    /// The executable this profile invokes, for error messages.
    pub fn runner_program(&self) -> String {
        match &self.runner {
            crate::config::RunnerKind::ClaudeCode => "claude".into(),
            crate::config::RunnerKind::Codex | crate::config::RunnerKind::CodexOss { .. } => {
                "codex".into()
            }
            crate::config::RunnerKind::Custom { command, .. } => command.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RunnerKind;
    use crate::model::{Task, TaskStatus};
    use crate::runner::AgentOutcome;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // --- Fakes ---------------------------------------------------------------

    #[derive(Debug, Default)]
    struct BoardLog {
        statuses: Vec<(String, TaskStatus)>,
        comments: Vec<String>,
        blockers: Vec<Option<String>>,
    }

    #[derive(Clone, Default)]
    struct FakeBoard {
        log: Arc<Mutex<BoardLog>>,
        fail_claim: bool,
    }

    #[async_trait::async_trait]
    impl Board for FakeBoard {
        async fn move_status(
            &self,
            task_id: &str,
            status: TaskStatus,
            _agent: Option<&str>,
        ) -> Result<(), super::super::types::BoardError> {
            if self.fail_claim && status == TaskStatus::InProgress {
                return Err(super::super::types::BoardError("board unreachable".into()));
            }
            self.log
                .lock()
                .unwrap()
                .statuses
                .push((task_id.to_string(), status));
            Ok(())
        }

        async fn comment(
            &self,
            _task_id: &str,
            body: &str,
            _agent: Option<&str>,
        ) -> Result<(), super::super::types::BoardError> {
            self.log.lock().unwrap().comments.push(body.to_string());
            Ok(())
        }

        async fn set_blocked_reason(
            &self,
            _task_id: &str,
            reason: Option<&str>,
        ) -> Result<(), super::super::types::BoardError> {
            self.log
                .lock()
                .unwrap()
                .blockers
                .push(reason.map(str::to_string));
            Ok(())
        }
    }

    impl FakeBoard {
        fn statuses(&self) -> Vec<TaskStatus> {
            self.log
                .lock()
                .unwrap()
                .statuses
                .iter()
                .map(|(_, s)| *s)
                .collect()
        }
        fn comments(&self) -> Vec<String> {
            self.log.lock().unwrap().comments.clone()
        }
        /// Blockers actually set (ignoring the clear at claim time).
        fn blockers_set(&self) -> Vec<String> {
            self.log
                .lock()
                .unwrap()
                .blockers
                .iter()
                .filter_map(|b| b.clone())
                .collect()
        }
    }

    #[derive(Clone)]
    struct FakeWorkspace {
        path: PathBuf,
        changes: Arc<Mutex<ChangeSummary>>,
        commits: Arc<Mutex<Vec<String>>>,
        integrated: Arc<Mutex<bool>>,
        commit_fails: bool,
        integrate_fails: bool,
    }

    impl FakeWorkspace {
        fn with_changes() -> Self {
            Self {
                path: PathBuf::from("/tmp/heretic-fake"),
                changes: Arc::new(Mutex::new(ChangeSummary {
                    files_changed: 1,
                    insertions: 10,
                    deletions: 2,
                    files: vec!["src/lib.rs".into()],
                })),
                commits: Arc::new(Mutex::new(Vec::new())),
                integrated: Arc::new(Mutex::new(false)),
                commit_fails: false,
                integrate_fails: false,
            }
        }

        fn empty() -> Self {
            let ws = Self::with_changes();
            *ws.changes.lock().unwrap() = ChangeSummary::default();
            ws
        }
    }

    #[async_trait::async_trait]
    impl Workspace for FakeWorkspace {
        fn path(&self) -> &Path {
            &self.path
        }

        async fn summarise(&self) -> Result<ChangeSummary, super::super::types::BoardError> {
            Ok(self.changes.lock().unwrap().clone())
        }

        async fn diff(&self) -> Result<String, super::super::types::BoardError> {
            Ok("diff --git a/src/lib.rs b/src/lib.rs".into())
        }

        async fn commit(&self, message: &str) -> Result<bool, super::super::types::BoardError> {
            if self.commit_fails {
                return Err(super::super::types::BoardError("disk full".into()));
            }
            self.commits.lock().unwrap().push(message.to_string());
            Ok(true)
        }

        async fn integrate(&self) -> Result<(), super::super::types::BoardError> {
            if self.integrate_fails {
                return Err(super::super::types::BoardError("merge conflict".into()));
            }
            *self.integrated.lock().unwrap() = true;
            Ok(())
        }
    }

    /// An executor that replays canned outcomes per role and records prompts.
    #[derive(Clone, Default)]
    struct ScriptedExecutor {
        /// Queued outcomes per role, consumed in order.
        script: Arc<Mutex<HashMap<Role, Vec<StageScript>>>>,
        prompts: Arc<Mutex<Vec<(Role, String)>>>,
    }

    #[derive(Clone)]
    enum StageScript {
        Says(String),
        Exits(i32),
        NotInstalled,
        Hangs,
    }

    impl ScriptedExecutor {
        fn with(role: Role, steps: Vec<StageScript>) -> Self {
            let executor = Self::default();
            executor.script.lock().unwrap().insert(role, steps);
            executor
        }

        fn and(self, role: Role, steps: Vec<StageScript>) -> Self {
            self.script.lock().unwrap().insert(role, steps);
            self
        }

        fn prompts_for(&self, role: Role) -> Vec<String> {
            self.prompts
                .lock()
                .unwrap()
                .iter()
                .filter(|(r, _)| *r == role)
                .map(|(_, p)| p.clone())
                .collect()
        }

        fn call_count(&self, role: Role) -> usize {
            self.prompts_for(role).len()
        }
    }

    #[async_trait::async_trait]
    impl AgentExecutor for ScriptedExecutor {
        async fn execute(
            &self,
            _profile: &ModelProfile,
            role: Role,
            prompt: &str,
            _working_dir: &Path,
            cancel: CancelToken,
            sink: mpsc::Sender<AgentEvent>,
        ) -> std::io::Result<AgentOutcome> {
            self.prompts
                .lock()
                .unwrap()
                .push((role, prompt.to_string()));

            let step = {
                let mut script = self.script.lock().unwrap();
                let queue = script.entry(role).or_default();
                if queue.is_empty() {
                    StageScript::Says("done".into())
                } else {
                    queue.remove(0)
                }
            };

            match step {
                StageScript::NotInstalled => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no such file",
                )),
                StageScript::Hangs => {
                    cancel.cancelled().await;
                    Ok(AgentOutcome {
                        completion: Completion::Cancelled,
                        exit_code: None,
                        duration: Duration::from_millis(1),
                        transcript: Vec::new(),
                    })
                }
                StageScript::Exits(code) => Ok(AgentOutcome {
                    completion: Completion::Failed,
                    exit_code: Some(code),
                    duration: Duration::from_millis(1),
                    transcript: vec![AgentEvent::Raw {
                        text: "something broke".into(),
                    }],
                }),
                StageScript::Says(text) => {
                    let event = AgentEvent::Result {
                        text: Some(text.clone()),
                        is_error: false,
                        duration_ms: Some(1),
                        cost_usd: None,
                    };
                    let _ = sink.send(event.clone()).await;
                    Ok(AgentOutcome {
                        completion: Completion::Succeeded,
                        exit_code: Some(0),
                        duration: Duration::from_millis(1),
                        transcript: vec![event],
                    })
                }
            }
        }
    }

    // --- Harness -------------------------------------------------------------

    fn profile(id: &str) -> ModelProfile {
        ModelProfile {
            id: id.into(),
            name: id.into(),
            runner: RunnerKind::ClaudeCode,
            model: None,
            extra_args: Vec::new(),
            env: Default::default(),
            timeout_secs: None,
            context_window: None,
            autonomous: true,
        }
    }

    fn config(pipeline: Pipeline, roles: &[Role]) -> RunConfig {
        let mut profiles = BTreeMap::new();
        for role in roles {
            profiles.insert(*role, profile(role.as_str()));
        }
        RunConfig { pipeline, profiles }
    }

    fn context() -> TaskContext {
        TaskContext {
            task: Task {
                id: "t1".into(),
                title: "Add a limiter".into(),
                status: "todo".into(),
                notes: None,
                depends_on: Vec::new(),
                project_id: "p1".into(),
                epic_id: Some("e1".into()),
                blocked: false,
                blocked_reason: None,
                archived: false,
                priority: None,
                acceptance_criteria: Vec::new(),
                guardrails: Vec::new(),
                comments: Vec::new(),
                blob_ids: Vec::new(),
                workers: Vec::new(),
                agent: None,
                created_at: None,
                updated_at: None,
            },
            epic_title: "Epic".into(),
            epic_notes: String::new(),
            project_name: "Project".into(),
        }
    }

    /// Run the pipeline, draining progress into a vec.
    async fn run(
        config: RunConfig,
        board: &FakeBoard,
        workspace: &FakeWorkspace,
        executor: &ScriptedExecutor,
        cancel: CancelToken,
    ) -> (RunOutcome, Vec<RunProgress>) {
        let (tx, mut rx) = mpsc::channel(512);
        let collected = Arc::new(Mutex::new(Vec::new()));
        let sink = collected.clone();
        let pump = tokio::spawn(async move {
            while let Some(item) = rx.recv().await {
                sink.lock().unwrap().push(item);
            }
        });

        let outcome = execute_run(context(), config, board, workspace, executor, cancel, tx).await;
        let _ = pump.await;
        let progress = collected.lock().unwrap().clone();
        (outcome, progress)
    }

    // --- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn an_approved_run_completes_and_closes_the_task() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(
            Role::Implementer,
            vec![StageScript::Says("Added the limiter.".into())],
        )
        .and(
            Role::Reviewer,
            vec![StageScript::Says("Looks right.\nVERDICT: approve".into())],
        );

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(outcome.result, RunResult::Completed);
        assert_eq!(outcome.revisions, 0);
        assert_eq!(
            board.statuses(),
            vec![TaskStatus::InProgress, TaskStatus::Done]
        );
        assert!(board.blockers_set().is_empty());
        assert_eq!(workspace.commits.lock().unwrap().len(), 1);
        assert!(*workspace.integrated.lock().unwrap());
        assert!(board.comments()[0].contains("Completed"));
        assert!(board.comments()[0].contains("src/lib.rs"));
    }

    #[tokio::test]
    async fn requested_changes_send_the_work_back_with_the_reviewers_notes() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(
            Role::Implementer,
            vec![
                StageScript::Says("First attempt.".into()),
                StageScript::Says("Fixed it.".into()),
            ],
        )
        .and(
            Role::Reviewer,
            vec![
                StageScript::Says("The limit is off by one.\nVERDICT: request_changes".into()),
                StageScript::Says("Good now.\nVERDICT: approve".into()),
            ],
        );

        let (outcome, progress) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(outcome.result, RunResult::Completed);
        assert_eq!(outcome.revisions, 1);
        assert_eq!(executor.call_count(Role::Implementer), 2);

        // The second implementation prompt must carry the review verbatim.
        let second = &executor.prompts_for(Role::Implementer)[1];
        assert!(second.contains("The limit is off by one."));
        assert!(second.contains("Changes requested by review"));

        assert!(progress
            .iter()
            .any(|p| matches!(p, RunProgress::RevisionRequested { attempt: 1, .. })));
    }

    #[tokio::test]
    async fn a_run_that_never_satisfies_review_is_handed_to_a_human() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(
            Role::Implementer,
            vec![
                StageScript::Says("one".into()),
                StageScript::Says("two".into()),
                StageScript::Says("three".into()),
            ],
        )
        .and(
            Role::Reviewer,
            vec![
                StageScript::Says("VERDICT: request_changes".into()),
                StageScript::Says("VERDICT: request_changes".into()),
                StageScript::Says("VERDICT: request_changes".into()),
            ],
        );

        let pipeline = Pipeline {
            max_revisions: 2,
            ..Pipeline::default()
        };
        let (outcome, _) = run(
            config(pipeline, &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert!(matches!(outcome.result, RunResult::NeedsAttention { .. }));
        // Three attempts: the original plus two revisions.
        assert_eq!(executor.call_count(Role::Implementer), 3);
        // The task goes back to Todo with a blocker, not left mid-flight.
        assert_eq!(
            board.statuses(),
            vec![TaskStatus::InProgress, TaskStatus::Todo]
        );
        assert!(board.blockers_set()[0].starts_with("Heretic:"));
        // Nothing is merged when review never passed.
        assert!(!*workspace.integrated.lock().unwrap());
    }

    #[tokio::test]
    async fn an_unclear_verdict_is_never_treated_as_approval() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())]).and(
                Role::Reviewer,
                vec![StageScript::Says("Seems fine to me.".into())],
            );

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert!(matches!(outcome.result, RunResult::NeedsAttention { .. }));
        assert!(!*workspace.integrated.lock().unwrap());
    }

    #[tokio::test]
    async fn a_run_that_changed_nothing_does_not_pass_review() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::empty();
        let executor = ScriptedExecutor::with(
            Role::Implementer,
            vec![StageScript::Says("Nothing needed doing.".into())],
        );

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        match outcome.result {
            RunResult::NeedsAttention { ref reason } => {
                assert!(reason.contains("no changes"));
            }
            other => panic!("expected NeedsAttention, got {other:?}"),
        }
        // The reviewer is never troubled with an empty diff.
        assert_eq!(executor.call_count(Role::Reviewer), 0);
    }

    #[tokio::test]
    async fn a_failing_agent_returns_the_task_with_a_blocker() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(Role::Implementer, vec![StageScript::Exits(1)]);

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        match outcome.result {
            RunResult::Failed { stage, .. } => assert_eq!(stage, RunStage::Implementing),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(
            board.statuses(),
            vec![TaskStatus::InProgress, TaskStatus::Todo]
        );
        assert_eq!(
            board.blockers_set(),
            vec!["Heretic: failed during implementing"]
        );
    }

    #[tokio::test]
    async fn a_missing_cli_says_so_plainly() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(Role::Implementer, vec![StageScript::NotInstalled]);

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        match outcome.result {
            RunResult::Failed { ref reason, .. } => {
                assert!(reason.contains("PATH"), "unhelpful message: {reason}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stopping_a_run_leaves_the_task_ready_again() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(Role::Implementer, vec![StageScript::Hangs]);

        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            trigger.cancel();
        });

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer]),
            &board,
            &workspace,
            &executor,
            cancel,
        )
        .await;

        assert_eq!(outcome.result, RunResult::Cancelled);
        assert_eq!(
            board.statuses(),
            vec![TaskStatus::InProgress, TaskStatus::Todo]
        );
        assert_eq!(board.blockers_set(), vec!["Heretic: stopped by request"]);
    }

    #[tokio::test]
    async fn planning_produces_a_brief_the_implementer_receives() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(
            Role::Orchestrator,
            vec![StageScript::Says(
                "Edit src/limiter.rs and add a bucket.".into(),
            )],
        )
        .and(Role::Implementer, vec![StageScript::Says("done".into())]);

        let pipeline = Pipeline {
            plan: true,
            review: false,
            ..Pipeline::default()
        };
        let (outcome, _) = run(
            config(pipeline, &[Role::Orchestrator, Role::Implementer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(outcome.result, RunResult::Completed);
        let prompt = &executor.prompts_for(Role::Implementer)[0];
        assert!(prompt.contains("Edit src/limiter.rs and add a bucket."));
        assert!(prompt.contains("Implementation brief"));
    }

    #[tokio::test]
    async fn a_failed_plan_does_not_stop_the_work() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::with(Role::Orchestrator, vec![StageScript::Exits(2)])
            .and(Role::Implementer, vec![StageScript::Says("done".into())]);

        let pipeline = Pipeline {
            plan: true,
            review: false,
            ..Pipeline::default()
        };
        let (outcome, _) = run(
            config(pipeline, &[Role::Orchestrator, Role::Implementer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(outcome.result, RunResult::Completed);
        assert_eq!(executor.call_count(Role::Implementer), 1);
    }

    #[tokio::test]
    async fn a_documentation_pass_runs_only_after_approval() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())])
                .and(
                    Role::Reviewer,
                    vec![StageScript::Says("VERDICT: approve".into())],
                )
                .and(
                    Role::Documenter,
                    vec![StageScript::Says("README updated".into())],
                );

        let pipeline = Pipeline {
            document: true,
            ..Pipeline::default()
        };
        let (outcome, _) = run(
            config(
                pipeline,
                &[Role::Implementer, Role::Reviewer, Role::Documenter],
            ),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(outcome.result, RunResult::Completed);
        assert_eq!(executor.call_count(Role::Documenter), 1);
        let prompt = &executor.prompts_for(Role::Documenter)[0];
        assert!(prompt.contains("src/lib.rs"));
    }

    #[tokio::test]
    async fn documentation_is_skipped_when_the_work_was_rejected() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())])
                .and(
                    Role::Reviewer,
                    vec![StageScript::Says("VERDICT: request_changes".into())],
                )
                .and(Role::Documenter, vec![StageScript::Says("docs".into())]);

        let pipeline = Pipeline {
            document: true,
            max_revisions: 0,
            ..Pipeline::default()
        };
        let (_, _) = run(
            config(
                pipeline,
                &[Role::Implementer, Role::Reviewer, Role::Documenter],
            ),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(executor.call_count(Role::Documenter), 0);
    }

    #[tokio::test]
    async fn review_can_be_switched_off_entirely() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())]);

        let pipeline = Pipeline {
            review: false,
            ..Pipeline::default()
        };
        let (outcome, _) = run(
            config(pipeline, &[Role::Implementer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert_eq!(outcome.result, RunResult::Completed);
        assert_eq!(executor.call_count(Role::Reviewer), 0);
        assert!(*workspace.integrated.lock().unwrap());
    }

    #[tokio::test]
    async fn a_run_cannot_start_without_an_implementer() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor = ScriptedExecutor::default();

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        match outcome.result {
            RunResult::Failed { stage, ref reason } => {
                assert_eq!(stage, RunStage::Preparing);
                assert!(reason.contains("Implementer"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        // The task is never claimed, and the board is told why nothing ran so
        // the auto loop does not retry it forever.
        assert!(!board.statuses().contains(&TaskStatus::InProgress));
        assert!(board.blockers_set()[0].contains("Heretic"));
    }

    #[tokio::test]
    async fn a_task_that_cannot_be_claimed_is_not_worked() {
        let board = FakeBoard {
            fail_claim: true,
            ..Default::default()
        };
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())]);

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        assert!(matches!(outcome.result, RunResult::Failed { .. }));
        assert_eq!(executor.call_count(Role::Implementer), 0);
    }

    #[tokio::test]
    async fn work_that_cannot_be_integrated_is_kept_not_marked_done() {
        let board = FakeBoard::default();
        let mut workspace = FakeWorkspace::with_changes();
        workspace.integrate_fails = true;
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())]).and(
                Role::Reviewer,
                vec![StageScript::Says("VERDICT: approve".into())],
            );

        let (outcome, _) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        match outcome.result {
            RunResult::NeedsAttention { ref reason } => {
                assert!(reason.contains("committed"), "{reason}");
            }
            other => panic!("expected NeedsAttention, got {other:?}"),
        }
        // The task must not be closed when its work never landed.
        assert!(!board.statuses().contains(&TaskStatus::Done));
    }

    #[tokio::test]
    async fn the_command_is_recorded_once_per_stage() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())]);

        let pipeline = Pipeline {
            review: false,
            ..Pipeline::default()
        };
        let (_, progress) = run(
            config(pipeline, &[Role::Implementer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        let commands: Vec<&String> = progress
            .iter()
            .filter_map(|p| match p {
                RunProgress::Command { command, .. } => Some(command),
                _ => None,
            })
            .collect();

        // Exactly one per agent stage, and it must not leak the prompt.
        assert_eq!(commands.len(), 1, "{commands:?}");
        assert!(commands[0].starts_with("claude "), "{}", commands[0]);
        assert!(commands[0].contains("'<prompt>'"), "{}", commands[0]);
    }

    #[tokio::test]
    async fn the_activity_feed_reports_every_stage() {
        let board = FakeBoard::default();
        let workspace = FakeWorkspace::with_changes();
        let executor =
            ScriptedExecutor::with(Role::Implementer, vec![StageScript::Says("done".into())]).and(
                Role::Reviewer,
                vec![StageScript::Says("VERDICT: approve".into())],
            );

        let (_, progress) = run(
            config(Pipeline::default(), &[Role::Implementer, Role::Reviewer]),
            &board,
            &workspace,
            &executor,
            CancelToken::new(),
        )
        .await;

        let started: Vec<RunStage> = progress
            .iter()
            .filter_map(|p| match p {
                RunProgress::StageStarted { stage, .. } => Some(*stage),
                _ => None,
            })
            .collect();

        assert_eq!(
            started,
            vec![
                RunStage::Implementing,
                RunStage::Reviewing,
                RunStage::Integrating
            ]
        );
        assert!(progress.iter().any(|p| matches!(p, RunProgress::Output(_))));
    }
}
