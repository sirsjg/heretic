//! Everything that talks to a Flux server.

pub mod client;
pub mod events;

pub use client::{AuthStatus, FluxClient, FluxConfig, FluxError};
pub use events::{BoardChange, FluxEvent, FluxWatcher};
