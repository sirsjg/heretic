//! Everything that talks to a Flux server.

pub mod client;
pub mod events;

pub use client::{FluxClient, FluxConfig, FluxError};
pub use events::{BoardChange, FluxEvent, FluxWatcher};
