//! Everything that talks to Linear.
//!
//! Linear is the first tracker besides Flux that Heretic can work from. The
//! mapping is deliberately one level shifted: a Linear team (which owns the
//! issues and the workflow) is presented as a project, a Linear project (which
//! groups issues toward an outcome) as an epic, and an issue as a task.

mod client;
pub(crate) mod map;

pub use client::{LinearClient, LinearConfig};
