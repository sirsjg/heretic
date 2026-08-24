//! The vocabulary of a run, and the seams that make it testable.
//!
//! The pipeline talks to the outside world through three traits — the board, the
//! agent executor and the workspace — so the whole state machine can be driven in
//! tests without a Flux server, a real agent, or a git repository.

use crate::config::{ModelProfile, Role};
use crate::model::TaskStatus;
use crate::runner::{AgentEvent, AgentOutcome, CancelToken, ModelTokenUsage, TokenUsage};
use crate::worktree::ChangeSummary;
use serde::Serialize;
use std::path::Path;
use tokio::sync::mpsc;

/// Which part of the pipeline is happening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStage {
    /// Setting up the worktree and claiming the task on the board.
    Preparing,
    Planning,
    Implementing,
    Reviewing,
    Documenting,
    /// Committing and, when configured, merging back.
    Integrating,
}

impl RunStage {
    pub fn label(self) -> &'static str {
        match self {
            RunStage::Preparing => "Preparing",
            RunStage::Planning => "Planning",
            RunStage::Implementing => "Implementing",
            RunStage::Reviewing => "Reviewing",
            RunStage::Documenting => "Documenting",
            RunStage::Integrating => "Integrating",
        }
    }

    /// The role that performs this stage, if any.
    pub fn role(self) -> Option<Role> {
        match self {
            RunStage::Planning => Some(Role::Orchestrator),
            RunStage::Implementing => Some(Role::Implementer),
            RunStage::Reviewing => Some(Role::Reviewer),
            RunStage::Documenting => Some(Role::Documenter),
            RunStage::Preparing | RunStage::Integrating => None,
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunResult {
    /// Implemented, reviewed and integrated.
    Completed,
    /// An agent failed, or the pipeline could not continue.
    Failed { stage: RunStage, reason: String },
    /// Stopped on request.
    Cancelled,
    /// Finished without a clear outcome — review kept rejecting, or the reviewer
    /// never gave a readable verdict. A human needs to look.
    NeedsAttention { reason: String },
}

impl RunResult {
    pub fn succeeded(&self) -> bool {
        matches!(self, RunResult::Completed)
    }

    /// One-line explanation for the board and the UI.
    pub fn describe(&self) -> String {
        match self {
            RunResult::Completed => "Completed".into(),
            RunResult::Failed { stage, reason } => {
                format!("Failed during {}: {reason}", stage.label().to_lowercase())
            }
            RunResult::Cancelled => "Stopped".into(),
            RunResult::NeedsAttention { reason } => format!("Needs attention: {reason}"),
        }
    }
}

/// Live status of a run, as the UI sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    NeedsAttention,
}

/// One entry in a run's activity feed.
#[derive(Debug, Clone, Serialize)]
pub struct RunFeedItem {
    pub stage: RunStage,
    pub role: Option<Role>,
    pub event: AgentEvent,
}

/// What one agent invocation consumed: tokens, time, and money.
///
/// A run collects one of these per stage — including each trip round the
/// review loop — so the finished run can show where the tokens went.
#[derive(Debug, Clone, Serialize)]
pub struct StageStats {
    pub stage: RunStage,
    pub role: Option<Role>,
    /// The profile that ran, as shown in the feed ("Claude Code · Reviewer").
    pub agent: Option<String>,
    /// Wall-clock time of the agent process.
    pub duration_ms: u64,
    pub usage: TokenUsage,
    pub cost_usd: Option<f64>,
    /// Per-model breakdown when the backend reports one; otherwise a single
    /// entry under the profile's configured model.
    pub models: Vec<ModelTokenUsage>,
}

/// Progress reported while a run is in flight.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunProgress {
    /// A stage began.
    StageStarted {
        stage: RunStage,
        role: Option<Role>,
        agent: Option<String>,
    },
    /// An agent said something.
    Output(RunFeedItem),
    /// A stage ended.
    StageFinished {
        stage: RunStage,
        ok: bool,
        summary: Option<String>,
    },
    /// What a finished agent stage consumed.
    StageStats(StageStats),
    /// The reviewer sent the work back; the implementer is going round again.
    RevisionRequested { attempt: u32, notes: String },
    /// A note from the engine itself, not from an agent.
    Note { message: String },
    /// The command a stage is about to run.
    ///
    /// Worth recording: when a backend rejects an argument, the argument list is
    /// the whole answer, and without it the failure is a guessing game.
    Command { stage: RunStage, command: String },
    /// The prompt a stage was handed, generated by Heretic from the task and
    /// whatever the earlier stages produced.
    ///
    /// The command log deliberately elides it — it would drown the flags — so
    /// this carries it separately, and the UI keeps it folded away until asked.
    Prompt {
        stage: RunStage,
        role: Option<Role>,
        text: String,
    },
}

/// What a finished run produced.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub result: RunResult,
    /// How many times review sent the work back.
    pub revisions: u32,
    pub changes: ChangeSummary,
    /// What to write on the board.
    pub summary: String,
}

/// A run as stored and shown in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub task_id: String,
    pub task_title: String,
    pub epic_title: String,
    pub status: RunStatus,
    pub stage: RunStage,
    /// Agent currently working, for the header of the run panel.
    pub agent: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub revisions: u32,
    pub branch: Option<String>,
    /// The branch the work forked from, and would merge back into.
    pub base_branch: Option<String>,
    pub worktree_path: Option<String>,
    /// What has become of the work since the agents finished.
    pub landing: Landing,
    pub changes: ChangeSummary,
    pub result: Option<RunResult>,
    /// Tokens, time and spend per agent stage, in the order the stages ran.
    pub stats: Vec<StageStats>,
    /// Capped activity feed — the full transcript lives on disk.
    pub feed: Vec<RunFeedItem>,
}

impl RunRecord {
    /// Keep the in-memory feed bounded; a long run can emit thousands of events.
    pub const FEED_LIMIT: usize = 500;

    pub fn push_feed(&mut self, item: RunFeedItem) {
        if self.feed.len() >= Self::FEED_LIMIT {
            self.feed.remove(0);
        }
        self.feed.push(item);
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, RunStatus::Queued | RunStatus::Running)
    }
}

/// Where a run's work ended up.
///
/// A run that finishes is not the end of the story: unless the project merges
/// automatically, the work sits on its own branch waiting for a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Landing {
    /// Nothing to land — the run made no changes, or worked in place.
    Nothing,
    /// Committed to its branch, waiting to be merged or discarded.
    OnBranch,
    /// Merged into the base branch.
    Merged,
    /// Branch and worktree deleted.
    Discarded,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct BoardError(pub String);

/// The task board a run reports to. Implemented against Flux, faked in tests.
#[async_trait::async_trait]
pub trait Board: Send + Sync {
    /// Move a task, attributing the change to `agent_name` on the board.
    async fn move_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        agent_name: Option<&str>,
    ) -> Result<(), BoardError>;

    /// Leave a comment — the memory the next agent inherits.
    async fn comment(
        &self,
        task_id: &str,
        body: &str,
        agent_name: Option<&str>,
    ) -> Result<(), BoardError>;

    /// Record or clear an external blocker.
    ///
    /// Heretic sets one when a run fails, which both explains the failure on
    /// the board and stops the auto loop from immediately retrying it.
    async fn set_blocked_reason(
        &self,
        task_id: &str,
        reason: Option<&str>,
    ) -> Result<(), BoardError>;
}

/// The checkout a run works in.
#[async_trait::async_trait]
pub trait Workspace: Send + Sync {
    /// Where agents run.
    fn path(&self) -> &Path;

    /// What has changed so far.
    async fn summarise(&self) -> Result<ChangeSummary, BoardError>;

    /// The diff to hand a reviewer.
    async fn diff(&self) -> Result<String, BoardError>;

    /// Commit the work. Returns false when there was nothing to commit.
    async fn commit(&self, message: &str) -> Result<bool, BoardError>;

    /// Integrate the work according to the project's setting. A no-op when the
    /// project is configured to leave branches alone.
    async fn integrate(&self) -> Result<(), BoardError>;
}

/// Runs an agent. The real implementation spawns a CLI; tests substitute canned
/// outcomes.
#[async_trait::async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn execute(
        &self,
        profile: &ModelProfile,
        role: Role,
        prompt: &str,
        working_dir: &Path,
        cancel: CancelToken,
        sink: mpsc::Sender<AgentEvent>,
    ) -> std::io::Result<AgentOutcome>;
}
