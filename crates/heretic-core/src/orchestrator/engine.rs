//! The engine: real implementations of the run traits, the run registry, and the
//! loop that picks up auto-enabled work.

use super::pipeline::{execute_run, RunConfig};
use super::types::{
    AgentExecutor, Board, BoardError, Landing, PendingQuestion, RunFeedItem, RunProgress,
    RunRecord, RunResult, RunStage, RunStatus, Workspace,
};
use crate::config::{Integration, Isolation, ModelProfile, ProjectBinding, Role, Settings};
use crate::flux::{FluxClient, FluxError};
use crate::history::RunHistory;
use crate::model::{Task, TaskStatus};
use crate::paths;
use crate::prompt::TaskContext;
use crate::runner::{build_command, run_agent, AgentEvent, AgentOutcome, CancelToken};
use crate::selection::BoardSnapshot;
use crate::worktree::{self, ChangeSummary, Worktree};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, RwLock};

impl From<FluxError> for BoardError {
    fn from(error: FluxError) -> Self {
        BoardError(error.to_string())
    }
}

impl From<worktree::GitError> for BoardError {
    fn from(error: worktree::GitError) -> Self {
        BoardError(error.to_string())
    }
}

// --- Real trait implementations ---------------------------------------------

/// A Flux-backed board.
struct FluxBoard {
    client: FluxClient,
}

#[async_trait::async_trait]
impl Board for FluxBoard {
    async fn move_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        agent_name: Option<&str>,
    ) -> Result<(), BoardError> {
        self.client
            .move_task_status(task_id, status, agent_name)
            .await?;
        Ok(())
    }

    async fn comment(
        &self,
        task_id: &str,
        body: &str,
        agent_name: Option<&str>,
    ) -> Result<(), BoardError> {
        self.client.add_comment(task_id, body, agent_name).await?;
        Ok(())
    }

    async fn set_blocked_reason(
        &self,
        task_id: &str,
        reason: Option<&str>,
    ) -> Result<(), BoardError> {
        self.client.set_blocked_reason(task_id, reason).await?;
        Ok(())
    }
}

/// A checkout on disk — either a dedicated worktree or the project directory itself.
struct GitWorkspace {
    /// Where agents run.
    path: PathBuf,
    /// The project's main checkout, used for merging.
    repo_path: PathBuf,
    base_branch: String,
    branch: Option<String>,
    integration: Integration,
}

#[async_trait::async_trait]
impl Workspace for GitWorkspace {
    fn path(&self) -> &Path {
        &self.path
    }

    async fn summarise(&self) -> Result<ChangeSummary, BoardError> {
        Ok(worktree::summarise_changes(&self.path, &self.base_branch).await?)
    }

    async fn diff(&self) -> Result<String, BoardError> {
        // 200 KB is enough for a task-sized change and still fits a review prompt.
        Ok(worktree::full_diff(&self.path, &self.base_branch, 200_000).await?)
    }

    async fn commit(&self, message: &str) -> Result<bool, BoardError> {
        // In-place runs are left uncommitted: the user is working in that
        // checkout and it is not ours to write history into.
        if self.branch.is_none() {
            return Ok(false);
        }
        Ok(worktree::commit_all(&self.path, message).await?)
    }

    async fn integrate(&self) -> Result<(), BoardError> {
        let (Some(branch), Integration::Merge) = (self.branch.as_deref(), self.integration) else {
            return Ok(());
        };
        worktree::merge_branch(&self.repo_path, branch, &self.base_branch).await?;
        Ok(())
    }
}

/// Spawns real agent CLIs.
struct CliExecutor;

#[async_trait::async_trait]
impl AgentExecutor for CliExecutor {
    async fn execute(
        &self,
        profile: &ModelProfile,
        _role: Role,
        prompt: &str,
        working_dir: &Path,
        cancel: CancelToken,
        sink: mpsc::Sender<AgentEvent>,
    ) -> std::io::Result<AgentOutcome> {
        let command = build_command(profile, prompt);
        let timeout = profile.timeout_secs.map(Duration::from_secs);
        run_agent(command, prompt, working_dir, timeout, cancel, sink).await
    }
}

// --- Engine ------------------------------------------------------------------

/// What the UI listens to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineEvent {
    /// A run's summary changed — created, stage moved, or finished.
    ///
    /// Boxed: this is much larger than the output events that dominate the
    /// channel, and every broadcast pays for the largest variant.
    RunUpdated { run: Box<RunRecord> },
    /// An agent produced output.
    RunOutput { run_id: String, item: RunFeedItem },
    /// Something worth telling the user that is not tied to one run.
    Notice { level: String, message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no project binding is configured for {0} — choose a folder for it first")]
    NotBound(String),

    #[error("{0}")]
    Flux(#[from] FluxError),

    #[error("{0}")]
    Git(#[from] worktree::GitError),

    #[error("no model is assigned to the {0} role")]
    NoProfile(&'static str),

    #[error("this task is already running")]
    AlreadyRunning,

    #[error("this run left nothing on a branch to land")]
    NothingToLand,
}

/// Settle a run that was still in flight when Heretic last closed.
///
/// Its agent died with the process, so nothing is coming to finish it — but the
/// worktree it was working in is still on disk. If the run had a branch, the
/// record is left offering the same merge-or-discard choice a finished run
/// gets: that decision is the only route back to the leftover checkout, and
/// without it the worktree is stranded with nothing in the interface pointing
/// at it. What is on the branch may well be half a task, which is why this
/// asks for attention rather than reporting a failure.
///
/// Returns whether anything changed.
fn interrupt(run: &mut RunRecord) -> bool {
    if !run.is_active() {
        return false;
    }

    run.status = RunStatus::NeedsAttention;
    run.result = Some(RunResult::NeedsAttention {
        reason: "Heretic closed while this run was working".into(),
    });
    // A question the run was paused on died with the process; nothing can
    // answer it now.
    run.question = None;
    run.finished_at = Some(chrono::Utc::now().to_rfc3339());
    if run.branch.is_some() && run.landing == Landing::Nothing {
        run.landing = Landing::OnBranch;
    }

    true
}

/// Owns settings, the Flux connection and every run in flight.
pub struct Engine {
    settings: Arc<RwLock<Settings>>,
    runs: Arc<RwLock<HashMap<String, RunRecord>>>,
    cancels: Arc<RwLock<HashMap<String, CancelToken>>>,
    /// Where a paused run's answer is delivered, per run in flight.
    answers: Arc<RwLock<HashMap<String, mpsc::Sender<String>>>>,
    events: broadcast::Sender<EngineEvent>,
    history: RunHistory,
}

impl Engine {
    /// An engine that forgets its runs when the process ends.
    pub fn new(settings: Settings) -> Self {
        Self::with_history(settings, RunHistory::disabled())
    }

    /// An engine that starts from the runs already in `history`, and journals
    /// everything after that back to it.
    pub fn with_history(settings: Settings, history: RunHistory) -> Self {
        let (events, _) = broadcast::channel(1024);

        let mut runs = HashMap::new();
        for mut run in history.load() {
            if interrupt(&mut run) {
                history.record(&run);
            }
            runs.insert(run.id.clone(), run);
        }

        Self {
            settings: Arc::new(RwLock::new(settings)),
            runs: Arc::new(RwLock::new(runs)),
            cancels: Arc::new(RwLock::new(HashMap::new())),
            answers: Arc::new(RwLock::new(HashMap::new())),
            events,
            history,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub async fn settings(&self) -> Settings {
        self.settings.read().await.clone()
    }

    pub async fn set_settings(&self, settings: Settings) {
        *self.settings.write().await = settings;
    }

    pub async fn flux_client(&self) -> Result<FluxClient, FluxError> {
        FluxClient::new(self.settings.read().await.flux.clone())
    }

    /// Every run, newest first.
    pub async fn runs(&self) -> Vec<RunRecord> {
        let mut runs: Vec<RunRecord> = self.runs.read().await.values().cloned().collect();
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs
    }

    pub async fn run(&self, run_id: &str) -> Option<RunRecord> {
        self.runs.read().await.get(run_id).cloned()
    }

    /// Task ids currently being worked.
    pub async fn running_task_ids(&self) -> HashSet<String> {
        self.runs
            .read()
            .await
            .values()
            .filter(|run| run.is_active())
            .map(|run| run.task_id.clone())
            .collect()
    }

    async fn active_runs_for_project(&self, project_id: &str) -> usize {
        self.runs
            .read()
            .await
            .values()
            .filter(|run| run.is_active() && run.project_id == project_id)
            .count()
    }

    /// Ask a run to stop. The agent is signalled; the run finishes as cancelled.
    pub async fn stop_run(&self, run_id: &str) -> bool {
        match self.cancels.read().await.get(run_id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Discard a finished run from the list. Active runs are kept — stop them first.
    pub async fn dismiss_run(&self, run_id: &str) -> bool {
        let removed = {
            let mut runs = self.runs.write().await;
            match runs.get(run_id) {
                Some(run) if !run.is_active() => runs.remove(run_id).is_some(),
                _ => false,
            }
        };
        if removed {
            self.cancels.write().await.remove(run_id);
            self.answers.write().await.remove(run_id);
            self.history.forget(run_id);
        }
        removed
    }

    /// Answer the question a run is paused on. Returns false when the run is
    /// not actually waiting — the question may have been answered elsewhere, or
    /// the run stopped in the meantime.
    pub async fn answer_question(&self, run_id: &str, answer: String) -> bool {
        let waiting = self
            .run(run_id)
            .await
            .is_some_and(|run| run.status == RunStatus::Waiting);
        if !waiting {
            return false;
        }
        match self.answers.read().await.get(run_id) {
            Some(sender) => sender.send(answer).await.is_ok(),
            None => false,
        }
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }

    async fn publish(&self, run: &RunRecord) {
        self.emit(EngineEvent::RunUpdated {
            run: Box::new(run.clone()),
        });
    }

    /// Start one task, whether or not its epic has auto enabled — this is the
    /// path a user takes by pressing Run on the board.
    pub async fn start_task(
        self: &Arc<Self>,
        project_id: &str,
        task_id: &str,
    ) -> Result<String, EngineError> {
        if self.running_task_ids().await.contains(task_id) {
            return Err(EngineError::AlreadyRunning);
        }

        let settings = self.settings.read().await.clone();
        let binding = settings
            .binding(project_id)
            .cloned()
            .ok_or_else(|| EngineError::NotBound(project_id.to_string()))?;

        let client = FluxClient::new(settings.flux.clone())?;
        let task = client.get_task(task_id).await?;
        let project = client.get_project(project_id).await?;

        let (epic_title, epic_notes) = match task.epic_id.as_deref().filter(|id| !id.is_empty()) {
            Some(epic_id) => match client.get_epic(epic_id).await {
                Ok(epic) => (epic.title, epic.notes),
                Err(_) => (String::new(), String::new()),
            },
            None => (String::new(), String::new()),
        };

        // Bind roles now so a mid-run settings change cannot alter this run.
        let mut profiles: BTreeMap<Role, ModelProfile> = BTreeMap::new();
        for role in Role::ALL {
            if let Some(profile) = settings.resolve_profile(project_id, role) {
                profiles.insert(role, profile.clone());
            }
        }
        if !profiles.contains_key(&Role::Implementer) {
            return Err(EngineError::NoProfile("Implementer"));
        }

        let run_id = uuid::Uuid::new_v4().to_string();
        let context = TaskContext {
            task: task.clone(),
            epic_title: epic_title.clone(),
            epic_notes,
            project_name: project.name.clone(),
        };

        let record = RunRecord {
            id: run_id.clone(),
            project_id: project_id.to_string(),
            project_name: project.name,
            task_id: task_id.to_string(),
            task_title: task.title.clone(),
            epic_title,
            status: RunStatus::Queued,
            stage: RunStage::Preparing,
            agent: profiles
                .get(&Role::Implementer)
                .map(|p| p.agent_name(Role::Implementer)),
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
            revisions: 0,
            branch: None,
            base_branch: None,
            worktree_path: None,
            question: None,
            landing: Landing::Nothing,
            changes: ChangeSummary::default(),
            result: None,
            stats: Vec::new(),
            feed: Vec::new(),
        };

        self.runs
            .write()
            .await
            .insert(run_id.clone(), record.clone());
        self.history.record(&record);
        let cancel = CancelToken::new();
        self.cancels
            .write()
            .await
            .insert(run_id.clone(), cancel.clone());
        // Room for a few answers so a send never blocks the UI; the pipeline
        // reads one per question asked.
        let (answer_tx, answer_rx) = mpsc::channel(8);
        self.answers
            .write()
            .await
            .insert(run_id.clone(), answer_tx);
        self.publish(&record).await;

        let engine = Arc::clone(self);
        let config = RunConfig {
            pipeline: binding.pipeline.clone(),
            profiles,
        };
        let run_id_for_task = run_id.clone();
        tokio::spawn(async move {
            engine
                .drive_run(
                    run_id_for_task,
                    context,
                    binding,
                    config,
                    client,
                    cancel,
                    answer_rx,
                )
                .await;
        });

        Ok(run_id)
    }

    /// Prepare the workspace, run the pipeline, and record the outcome.
    #[allow(clippy::too_many_arguments)]
    async fn drive_run(
        self: Arc<Self>,
        run_id: String,
        context: TaskContext,
        binding: ProjectBinding,
        config: RunConfig,
        client: FluxClient,
        cancel: CancelToken,
        answers: mpsc::Receiver<String>,
    ) {
        let task_id = context.task.id.clone();

        // --- Workspace ---
        let workspace = match self
            .prepare_workspace(&run_id, &binding, &context.task)
            .await
        {
            Ok(workspace) => workspace,
            Err(error) => {
                self.conclude(
                    &run_id,
                    RunResult::Failed {
                        stage: RunStage::Preparing,
                        reason: error.to_string(),
                    },
                    0,
                    ChangeSummary::default(),
                )
                .await;
                // The task was never claimed, so only the blocker needs recording.
                let _ = client
                    .set_blocked_reason(&task_id, Some(&format!("Heretic: {error}")))
                    .await;
                return;
            }
        };

        self.update(&run_id, |run| {
            run.status = RunStatus::Running;
            run.stage = RunStage::Preparing;
            run.branch = workspace.branch.clone();
            run.base_branch = Some(workspace.base_branch.clone());
            run.worktree_path = Some(workspace.path.display().to_string());
        })
        .await;

        // --- Progress relay ---
        let (tx, mut rx) = mpsc::channel::<RunProgress>(512);
        let engine = Arc::clone(&self);
        let relay_run_id = run_id.clone();
        let relay = tokio::spawn(async move {
            while let Some(progress) = rx.recv().await {
                engine.apply_progress(&relay_run_id, progress).await;
            }
        });

        let board = FluxBoard { client };
        let executor = CliExecutor;
        let outcome = execute_run(
            context, config, &board, &workspace, &executor, cancel, tx, answers,
        )
        .await;
        let _ = relay.await;

        // Tidy up the worktree once the work has been merged; otherwise leave it
        // so the user can inspect what happened.
        let merged = outcome.result.succeeded()
            && binding.integration == Integration::Merge
            && workspace.branch.is_some();
        if merged {
            let _ = worktree::remove_worktree(&binding.repo_path, &workspace.path).await;
        }

        let landing = if merged {
            Landing::Merged
        } else if workspace.branch.is_some() && !outcome.changes.is_empty() {
            // The work is committed on its own branch and needs a decision.
            Landing::OnBranch
        } else {
            Landing::Nothing
        };

        self.update(&run_id, |run| run.landing = landing).await;
        self.conclude(&run_id, outcome.result, outcome.revisions, outcome.changes)
            .await;
    }

    async fn prepare_workspace(
        &self,
        run_id: &str,
        binding: &ProjectBinding,
        task: &Task,
    ) -> Result<GitWorkspace, EngineError> {
        let _ = run_id;

        if !worktree::is_repository(&binding.repo_path).await {
            return Err(EngineError::Git(worktree::GitError::NotARepository {
                path: binding.repo_path.display().to_string(),
            }));
        }

        let base_branch = match binding.base_branch.clone() {
            Some(branch) if !branch.trim().is_empty() => branch,
            _ => worktree::current_branch(&binding.repo_path).await?,
        };

        match binding.isolation {
            Isolation::InPlace => Ok(GitWorkspace {
                path: binding.repo_path.clone(),
                repo_path: binding.repo_path.clone(),
                base_branch,
                branch: None,
                integration: Integration::Leave,
            }),
            Isolation::Worktree => {
                let root = paths::worktree_root(&binding.project_id);
                let Worktree {
                    path,
                    branch,
                    base_branch,
                } = worktree::create_worktree(
                    &binding.repo_path,
                    &root,
                    &task.id,
                    &task.title,
                    Some(&base_branch),
                )
                .await?;

                Ok(GitWorkspace {
                    path,
                    repo_path: binding.repo_path.clone(),
                    base_branch,
                    branch: Some(branch),
                    integration: binding.integration,
                })
            }
        }
    }

    async fn apply_progress(&self, run_id: &str, progress: RunProgress) {
        match progress {
            RunProgress::StageStarted { stage, agent, .. } => {
                self.update(run_id, |run| {
                    run.stage = stage;
                    if let Some(agent) = agent.clone() {
                        run.agent = Some(agent);
                    }
                })
                .await;
            }
            RunProgress::Output(item) => self.push_feed(run_id, item).await,
            RunProgress::RevisionRequested { attempt, .. } => {
                self.update(run_id, |run| run.revisions = attempt).await;
            }
            RunProgress::QuestionAsked {
                stage,
                role,
                question,
            } => {
                self.update(run_id, |run| {
                    run.status = RunStatus::Waiting;
                    run.question = Some(PendingQuestion {
                        stage,
                        role,
                        question: question.clone(),
                    });
                })
                .await;
                self.push_feed(
                    run_id,
                    RunFeedItem {
                        stage,
                        role,
                        event: AgentEvent::Question { text: question },
                    },
                )
                .await;
            }
            RunProgress::QuestionAnswered { stage, answer } => {
                self.update(run_id, |run| {
                    run.status = RunStatus::Running;
                    run.question = None;
                })
                .await;
                self.push_feed(
                    run_id,
                    RunFeedItem {
                        stage,
                        // The answer is the user's, not an agent's.
                        role: None,
                        event: AgentEvent::Answer { text: answer },
                    },
                )
                .await;
            }
            RunProgress::Command { stage, command } => {
                self.push_feed(
                    run_id,
                    RunFeedItem {
                        stage,
                        role: None,
                        event: AgentEvent::Raw {
                            text: format!("$ {command}"),
                        },
                    },
                )
                .await;
            }
            RunProgress::Prompt { stage, role, text } => {
                self.push_feed(
                    run_id,
                    RunFeedItem {
                        stage,
                        role,
                        event: AgentEvent::Prompt { text },
                    },
                )
                .await;
            }
            RunProgress::Note { message } => {
                let stage = self
                    .run(run_id)
                    .await
                    .map(|r| r.stage)
                    .unwrap_or(RunStage::Preparing);
                self.push_feed(
                    run_id,
                    RunFeedItem {
                        stage,
                        role: None,
                        event: AgentEvent::Raw { text: message },
                    },
                )
                .await;
            }
            RunProgress::StageStats(stats) => {
                self.update(run_id, |run| run.stats.push(stats)).await;
            }
            RunProgress::StageFinished { .. } => {}
        }
    }

    /// Record one entry in a run's feed and push it to the UI.
    async fn push_feed(&self, run_id: &str, item: RunFeedItem) {
        {
            let mut runs = self.runs.write().await;
            if let Some(run) = runs.get_mut(run_id) {
                run.push_feed(item.clone());
            }
        }
        // The journal takes every event, including the ones the in-memory feed
        // has had to drop off the front.
        self.history.feed(run_id, &item);
        self.emit(EngineEvent::RunOutput {
            run_id: run_id.to_string(),
            item,
        });
    }

    async fn update(&self, run_id: &str, mutate: impl FnOnce(&mut RunRecord)) {
        let updated = {
            let mut runs = self.runs.write().await;
            match runs.get_mut(run_id) {
                Some(run) => {
                    mutate(run);
                    Some(run.clone())
                }
                None => None,
            }
        };
        if let Some(run) = updated {
            self.history.record(&run);
            self.publish(&run).await;
        }
    }

    async fn conclude(
        &self,
        run_id: &str,
        result: RunResult,
        revisions: u32,
        changes: ChangeSummary,
    ) {
        let status = match &result {
            RunResult::Completed => RunStatus::Succeeded,
            RunResult::Failed { .. } => RunStatus::Failed,
            RunResult::Cancelled => RunStatus::Cancelled,
            RunResult::NeedsAttention { .. } => RunStatus::NeedsAttention,
        };

        self.update(run_id, |run| {
            run.status = status;
            run.result = Some(result);
            run.revisions = revisions;
            run.changes = changes;
            // A run stopped mid-question must not keep offering the answer box.
            run.question = None;
            run.finished_at = Some(chrono::Utc::now().to_rfc3339());
        })
        .await;

        self.cancels.write().await.remove(run_id);
        self.answers.write().await.remove(run_id);
    }

    /// Merge a finished run's branch into the branch it came from, then remove
    /// its worktree.
    ///
    /// This is what a project set to leave branches alone needs afterwards:
    /// the work is committed but nothing has decided its fate.
    pub async fn integrate_run(&self, run_id: &str) -> Result<(), EngineError> {
        let (run, binding) = self.run_with_binding(run_id).await?;

        let (Some(branch), Some(base)) = (run.branch.as_deref(), run.base_branch.as_deref()) else {
            return Err(EngineError::NothingToLand);
        };

        worktree::merge_branch(&binding.repo_path, branch, base).await?;

        if let Some(path) = run.worktree_path.as_deref() {
            let _ = worktree::remove_worktree(&binding.repo_path, std::path::Path::new(path)).await;
        }

        self.update(run_id, |run| run.landing = Landing::Merged)
            .await;
        Ok(())
    }

    /// Throw a run's work away: remove its worktree and delete its branch.
    pub async fn discard_run_work(&self, run_id: &str) -> Result<(), EngineError> {
        let (run, binding) = self.run_with_binding(run_id).await?;
        let Some(branch) = run.branch.as_deref() else {
            return Err(EngineError::NothingToLand);
        };

        // The worktree has the branch checked out, so it has to go first.
        if let Some(path) = run.worktree_path.as_deref() {
            worktree::remove_worktree(&binding.repo_path, std::path::Path::new(path)).await?;
        }
        worktree::delete_branch(&binding.repo_path, branch).await?;

        self.update(run_id, |run| run.landing = Landing::Discarded)
            .await;
        Ok(())
    }

    async fn run_with_binding(
        &self,
        run_id: &str,
    ) -> Result<(RunRecord, ProjectBinding), EngineError> {
        let run = self
            .run(run_id)
            .await
            .ok_or_else(|| EngineError::NotBound(run_id.to_string()))?;
        let binding = self
            .settings
            .read()
            .await
            .binding(&run.project_id)
            .cloned()
            .ok_or_else(|| EngineError::NotBound(run.project_id.clone()))?;
        Ok((run, binding))
    }

    /// Look at every auto-enabled project and start whatever work is ready,
    /// respecting each project's parallelism. Returns the run ids started.
    pub async fn tick_auto(self: &Arc<Self>) -> Vec<String> {
        let settings = self.settings.read().await.clone();
        let Ok(client) = FluxClient::new(settings.flux.clone()) else {
            return Vec::new();
        };

        let mut started = Vec::new();

        for binding in settings.bindings.iter().filter(|b| b.auto_run) {
            let capacity = binding
                .effective_parallelism()
                .saturating_sub(self.active_runs_for_project(&binding.project_id).await as u32);
            if capacity == 0 {
                continue;
            }

            let epics = match client.list_epics(&binding.project_id).await {
                Ok(epics) => epics,
                Err(error) => {
                    // Say why. "Could not read the board" on its own leaves the
                    // user guessing between a proxy challenge, a bad key and a
                    // server that is simply down.
                    self.emit(EngineEvent::Notice {
                        level: "warn".into(),
                        message: format!("Could not read the board: {error}"),
                    });
                    continue;
                }
            };
            let tasks = match client.list_tasks(&binding.project_id).await {
                Ok(tasks) => tasks,
                Err(error) => {
                    self.emit(EngineEvent::Notice {
                        level: "warn".into(),
                        message: format!("Could not read the board: {error}"),
                    });
                    continue;
                }
            };

            let board = BoardSnapshot {
                epics: &epics,
                tasks: &tasks,
            };
            let running = self.running_task_ids().await;

            for candidate in board
                .candidates(&running)
                .into_iter()
                .take(capacity as usize)
            {
                match self
                    .start_task(&binding.project_id, &candidate.task.id)
                    .await
                {
                    Ok(run_id) => started.push(run_id),
                    Err(EngineError::AlreadyRunning) => {}
                    Err(error) => self.emit(EngineEvent::Notice {
                        level: "error".into(),
                        message: format!("Could not start {}: {error}", candidate.task.title),
                    }),
                }
            }
        }

        started
    }

    /// Poll for auto-enabled work on an interval, forever.
    ///
    /// Returns the loop as a future rather than spawning it, so the caller
    /// decides which runtime owns it — a desktop shell's setup hook does not
    /// necessarily run inside one.
    pub async fn auto_loop(self: Arc<Self>, interval: Duration) {
        loop {
            tokio::time::sleep(interval).await;
            self.tick_auto().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "heretic-engine-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A run as it would have been journalled mid-flight.
    fn a_run_in_flight(id: &str) -> RunRecord {
        RunRecord {
            id: id.into(),
            project_id: "proj1".into(),
            project_name: "Heretic".into(),
            task_id: "task1".into(),
            task_title: "Persist run history".into(),
            epic_title: "Storage".into(),
            status: RunStatus::Running,
            stage: RunStage::Implementing,
            agent: Some("Claude Code · Implementer".into()),
            started_at: "2026-08-24T10:00:00Z".into(),
            finished_at: None,
            revisions: 0,
            branch: Some("heretic/task1".into()),
            base_branch: Some("main".into()),
            worktree_path: Some("/tmp/worktree".into()),
            question: None,
            landing: Landing::Nothing,
            changes: ChangeSummary::default(),
            result: None,
            stats: Vec::new(),
            feed: Vec::new(),
        }
    }

    #[tokio::test]
    async fn runs_come_back_after_a_restart() {
        let dir = TempDir::new("restart");
        let run = a_run_in_flight("run-1");

        {
            let history = RunHistory::new(&dir.0);
            history.record(&run);
            history.feed(
                &run.id,
                &RunFeedItem {
                    stage: RunStage::Implementing,
                    role: Some(Role::Implementer),
                    event: AgentEvent::Text {
                        text: "editing store.rs".into(),
                    },
                },
            );
        }

        let engine =
            Engine::with_history(Settings::with_starter_profiles(), RunHistory::new(&dir.0));
        let runs = engine.runs().await;

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run-1");
        assert_eq!(runs[0].task_title, "Persist run history");
        assert_eq!(runs[0].feed.len(), 1);
    }

    #[tokio::test]
    async fn a_run_cut_short_by_a_restart_asks_for_attention() {
        let dir = TempDir::new("interrupted");

        let history = RunHistory::new(&dir.0);
        history.record(&a_run_in_flight("run-1"));

        let engine =
            Engine::with_history(Settings::with_starter_profiles(), RunHistory::new(&dir.0));
        let runs = engine.runs().await;

        assert_eq!(runs[0].status, RunStatus::NeedsAttention);
        assert!(matches!(
            runs[0].result,
            Some(RunResult::NeedsAttention { .. })
        ));
        assert!(runs[0].finished_at.is_some());
        // Its worktree is still on disk, so the record has to offer the way out.
        assert_eq!(runs[0].landing, Landing::OnBranch);

        // And the verdict was written down, so the next start agrees with it
        // rather than deciding all over again.
        let reloaded = RunHistory::new(&dir.0).load();
        assert_eq!(reloaded[0].status, RunStatus::NeedsAttention);
        assert_eq!(reloaded[0].finished_at, runs[0].finished_at);
    }

    #[tokio::test]
    async fn a_run_waiting_on_a_question_is_interrupted_like_any_other() {
        let dir = TempDir::new("waiting");

        let mut run = a_run_in_flight("run-1");
        run.status = RunStatus::Waiting;
        run.question = Some(PendingQuestion {
            stage: RunStage::Implementing,
            role: Some(Role::Implementer),
            question: "Which port?".into(),
        });
        RunHistory::new(&dir.0).record(&run);

        let engine =
            Engine::with_history(Settings::with_starter_profiles(), RunHistory::new(&dir.0));
        let runs = engine.runs().await;

        // The agent that asked died with the process; the question with it.
        assert_eq!(runs[0].status, RunStatus::NeedsAttention);
        assert!(runs[0].question.is_none());

        // And nothing is listening, so an answer is refused rather than lost.
        assert!(!engine.answer_question("run-1", "3000".into()).await);
    }

    #[tokio::test]
    async fn an_answer_for_a_run_that_is_not_waiting_is_refused() {
        let engine = Engine::new(Settings::with_starter_profiles());
        assert!(!engine.answer_question("no-such-run", "hello".into()).await);
    }

    #[tokio::test]
    async fn a_run_that_finished_before_the_restart_is_left_alone() {
        let dir = TempDir::new("finished");

        let mut run = a_run_in_flight("run-1");
        run.status = RunStatus::Succeeded;
        run.result = Some(RunResult::Completed);
        run.finished_at = Some("2026-08-24T10:30:00Z".into());
        run.landing = Landing::Merged;
        RunHistory::new(&dir.0).record(&run);

        let engine =
            Engine::with_history(Settings::with_starter_profiles(), RunHistory::new(&dir.0));
        let runs = engine.runs().await;

        assert_eq!(runs[0].status, RunStatus::Succeeded);
        assert_eq!(runs[0].landing, Landing::Merged);
        assert_eq!(runs[0].finished_at.as_deref(), Some("2026-08-24T10:30:00Z"));
    }

    #[tokio::test]
    async fn dismissing_a_run_forgets_it_for_good() {
        let dir = TempDir::new("dismiss");

        let mut run = a_run_in_flight("run-1");
        run.status = RunStatus::Succeeded;
        run.result = Some(RunResult::Completed);
        RunHistory::new(&dir.0).record(&run);

        let engine =
            Engine::with_history(Settings::with_starter_profiles(), RunHistory::new(&dir.0));
        assert!(engine.dismiss_run("run-1").await);
        assert!(engine.runs().await.is_empty());

        let restarted =
            Engine::with_history(Settings::with_starter_profiles(), RunHistory::new(&dir.0));
        assert!(restarted.runs().await.is_empty());
    }

    #[tokio::test]
    async fn an_engine_without_history_starts_empty_and_writes_nothing() {
        let dir = TempDir::new("disabled");
        RunHistory::new(&dir.0).record(&a_run_in_flight("run-1"));

        let engine = Engine::new(Settings::with_starter_profiles());
        assert!(engine.runs().await.is_empty());
    }
}
