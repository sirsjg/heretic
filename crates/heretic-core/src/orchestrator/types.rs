//! The vocabulary of a run, and the seams that make it testable.
//!
//! The pipeline talks to the outside world through three traits — the board, the
//! agent executor and the workspace — so the whole state machine can be driven in
//! tests without a Flux server, a real agent, or a git repository.

use crate::config::{ModelProfile, Role};
use crate::model::TaskStatus;
use crate::runner::{AgentEvent, AgentOutcome, CancelToken, ModelTokenUsage, TokenUsage};
use crate::worktree::ChangeSummary;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::sync::mpsc;

/// Which part of the pipeline is happening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    /// Paused on a question an agent asked; nothing moves until the user answers.
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    NeedsAttention,
}

/// A question a run is paused on, shown in the run header until it is answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingQuestion {
    pub stage: RunStage,
    #[serde(default)]
    pub role: Option<Role>,
    pub question: String,
}

/// One entry in a run's activity feed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFeedItem {
    pub stage: RunStage,
    #[serde(default)]
    pub role: Option<Role>,
    pub event: AgentEvent,
}

/// What one agent invocation consumed: tokens, time, and money.
///
/// A run collects one of these per stage — including each trip round the
/// review loop — so the finished run can show where the tokens went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageStats {
    pub stage: RunStage,
    #[serde(default)]
    pub role: Option<Role>,
    /// The profile that ran, as shown in the feed ("Claude Code · Reviewer").
    #[serde(default)]
    pub agent: Option<String>,
    /// Wall-clock time of the agent process.
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub usage: TokenUsage,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Per-model breakdown when the backend reports one; otherwise a single
    /// entry under the profile's configured model.
    #[serde(default)]
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
    /// An agent asked the user something; the run is paused until the answer.
    QuestionAsked {
        stage: RunStage,
        role: Option<Role>,
        question: String,
    },
    /// The user answered; the stage runs again with the answer in hand.
    QuestionAnswered { stage: RunStage, answer: String },
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
///
/// This is also what a run's history journal replays, so every field that is
/// not needed to identify the run is optional on the way in: a record written
/// by an earlier build should come back as a slightly emptier run, never as a
/// parse error that loses it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    pub task_id: String,
    #[serde(default)]
    pub task_title: String,
    #[serde(default)]
    pub epic_title: String,
    pub status: RunStatus,
    pub stage: RunStage,
    /// Agent currently working, for the header of the run panel.
    #[serde(default)]
    pub agent: Option<String>,
    pub started_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub revisions: u32,
    #[serde(default)]
    pub branch: Option<String>,
    /// The branch the work forked from, and would merge back into.
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    /// The question the run is paused on, when its status is `Waiting`.
    #[serde(default)]
    pub question: Option<PendingQuestion>,
    /// What has become of the work since the agents finished.
    #[serde(default)]
    pub landing: Landing,
    #[serde(default)]
    pub changes: ChangeSummary,
    #[serde(default)]
    pub result: Option<RunResult>,
    /// Tokens, time and spend per agent stage, in the order the stages ran.
    #[serde(default)]
    pub stats: Vec<StageStats>,
    /// Capped activity feed. The full transcript lives on disk, in the run's
    /// history journal, and is replayed into this on the way back in.
    #[serde(default)]
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
        matches!(
            self.status,
            RunStatus::Queued | RunStatus::Running | RunStatus::Waiting
        )
    }
}

/// Where a run's work ended up.
///
/// A run that finishes is not the end of the story: unless the project merges
/// automatically, the work sits on its own branch waiting for a decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Landing {
    /// Nothing to land — the run made no changes, or worked in place.
    #[default]
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
