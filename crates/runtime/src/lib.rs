//! The I/O shell around the coordinator: it runs the pure state machine from
//! `bukno-core` on a Tokio runtime, executes its effects against the store,
//! the engine adapters and the platform, and publishes view updates to the
//! interface.
//!
//! The synthetic scenario mode in [`synthetic`] uses the same state machine
//! with a fake engine and no storage, for visual and input checks.

pub mod config;
pub mod coordinator;
pub mod engines;
pub mod synthetic;
pub mod workspace;

pub use coordinator::{Coordinator, EngineAction, QuitReport, SetupView, UiCommand, UiEvent};
