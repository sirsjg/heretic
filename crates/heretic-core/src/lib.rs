//! Core engine for Heretic: a desktop companion that runs Flux boards with
//! a team of AI agents.
//!
//! Everything here is free of any UI framework so the orchestration logic can be
//! exercised without launching a desktop app. The Tauri shell is a thin layer over
//! this crate.

pub mod config;
pub mod detect;
pub mod flux;
pub mod history;
pub mod linear;
pub mod model;
pub mod orchestrator;
pub mod paths;
pub mod prompt;
pub mod runner;
pub mod selection;
pub mod source;
pub mod store;
pub mod worktree;

pub use config::{Isolation, ModelProfile, Pipeline, ProjectBinding, Role, RunnerKind, Settings};
pub use flux::{FluxClient, FluxConfig, FluxError, FluxEvent, FluxWatcher};
pub use history::RunHistory;
pub use linear::{LinearClient, LinearConfig};
pub use model::{Epic, Priority, Project, SourceKind, Task, TaskStatus};
pub use orchestrator::{Engine, EngineEvent, RunRecord, RunStage, RunStatus};
pub use source::{SourceError, TaskSource};
