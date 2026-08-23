//! Running agent CLIs: what to invoke, how to supervise it, and how to read
//! what it prints.

pub mod command;
pub mod process;
pub mod stream;

pub use command::{build_command, AgentCommand};
pub use process::{run_agent, AgentOutcome, CancelToken, Completion};
pub use stream::{AgentEvent, ModelTokenUsage, OutputFormat, TokenUsage};
