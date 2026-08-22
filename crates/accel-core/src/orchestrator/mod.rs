//! Running a task from start to finish with a team of agents.

mod pipeline;
mod types;

pub mod engine;

pub use engine::{Engine, EngineEvent};
pub use pipeline::{execute_run, RunConfig};
pub use types::{
    AgentExecutor, Board, BoardError, RunFeedItem, RunOutcome, RunProgress, RunRecord, RunResult,
    RunStage, RunStatus, Workspace,
};
