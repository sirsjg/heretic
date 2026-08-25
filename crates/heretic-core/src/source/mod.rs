//! The seam between the engine and whichever tracker holds the work.
//!
//! Heretic started as a Flux companion, but the engine only ever needed a
//! board it can read tasks from and write outcomes to. [`TaskSource`] is that
//! contract: Flux implements it natively, and other trackers (Linear first)
//! implement it by mapping their own shapes into the canonical model in
//! [`crate::model`]. Everything downstream — selection, prompts, the run
//! pipeline — works on the canonical model and never learns which tracker a
//! task came from.

use crate::model::{Epic, Project, SourceKind, Task, TaskStatus};
use crate::orchestrator::{Board, BoardError};
use std::sync::Arc;

/// What went wrong talking to a source, flattened so the engine and UI can
/// react the same way whichever tracker it was.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SourceError {
    pub message: String,
    pub kind: SourceErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceErrorKind {
    /// The credential was missing, rejected, or lacks access.
    Auth,
    NotFound,
    /// The server could not be reached at all.
    Transport,
    /// The server answered with something Heretic could not parse.
    Decode,
    /// The server accepted the request but reported a failure.
    Api,
    /// The source is not configured — e.g. no Linear API key saved.
    Unconfigured,
}

impl SourceError {
    pub fn new(kind: SourceErrorKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind,
        }
    }

    pub fn is_auth(&self) -> bool {
        self.kind == SourceErrorKind::Auth
    }

    pub fn is_not_found(&self) -> bool {
        self.kind == SourceErrorKind::NotFound
    }
}

pub type Result<T> = std::result::Result<T, SourceError>;

/// A task tracker Heretic can work from.
///
/// The vocabulary is Flux's — projects hold epics hold tasks — because that is
/// the canonical model the rest of the engine speaks. Sources with different
/// shapes map into it: Linear's teams become projects and its projects become
/// epics.
///
/// Write operations are best-effort translations. A source without a native
/// equivalent (Linear has no `blocked_reason` field, for instance) must still
/// honour the *intent*: after `set_blocked_reason(Some(..))` the task must not
/// be picked up again by the auto loop, however the source records that.
#[async_trait::async_trait]
pub trait TaskSource: Send + Sync {
    /// Which tracker this is, stamped onto everything it returns.
    fn kind(&self) -> SourceKind;

    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn get_project(&self, project_id: &str) -> Result<Project>;

    async fn list_epics(&self, project_id: &str) -> Result<Vec<Epic>>;
    async fn get_epic(&self, epic_id: &str) -> Result<Epic>;

    /// Flip the switch that lets an epic's tasks run unattended.
    ///
    /// Sources with no such concept return `Unsupported`-flavoured errors or
    /// are handled a layer up (Linear's flag lives in Heretic's own settings).
    async fn set_epic_auto(&self, epic_id: &str, auto: bool) -> Result<()>;

    async fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>>;
    async fn get_task(&self, task_id: &str) -> Result<Task>;

    /// Move a task, attributing the change to `agent_name` where the source
    /// can record that.
    async fn move_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        agent_name: Option<&str>,
    ) -> Result<()>;

    /// Leave a comment — the memory the next agent inherits.
    async fn comment(&self, task_id: &str, body: &str, agent_name: Option<&str>) -> Result<()>;

    /// Record or clear an external blocker. Whatever form it takes on the
    /// source, a recorded blocker must stop the auto loop retrying the task.
    async fn set_blocked_reason(&self, task_id: &str, reason: Option<&str>) -> Result<()>;
}

/// Adapts a [`TaskSource`] to the pipeline's narrower [`Board`] trait, so the
/// run state machine stays exactly as testable as it was.
pub struct SourceBoard(pub Arc<dyn TaskSource>);

#[async_trait::async_trait]
impl Board for SourceBoard {
    async fn move_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        agent_name: Option<&str>,
    ) -> std::result::Result<(), BoardError> {
        self.0
            .move_status(task_id, status, agent_name)
            .await
            .map_err(|e| BoardError(e.to_string()))
    }

    async fn comment(
        &self,
        task_id: &str,
        body: &str,
        agent_name: Option<&str>,
    ) -> std::result::Result<(), BoardError> {
        self.0
            .comment(task_id, body, agent_name)
            .await
            .map_err(|e| BoardError(e.to_string()))
    }

    async fn set_blocked_reason(
        &self,
        task_id: &str,
        reason: Option<&str>,
    ) -> std::result::Result<(), BoardError> {
        self.0
            .set_blocked_reason(task_id, reason)
            .await
            .map_err(|e| BoardError(e.to_string()))
    }
}
