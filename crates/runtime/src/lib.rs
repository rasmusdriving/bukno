//! The I/O shell around the coordinator: it runs the pure state machine from
//! `bukno-core` on a Tokio runtime, executes its effects, and publishes view
//! updates to the interface.
//!
//! Pass 0 has no storage or engine connections. The only engine and store
//! are the synthetic ones in [`synthetic`], used by the labeled synthetic
//! scenario mode.

pub mod coordinator;
pub mod synthetic;

pub use coordinator::{Coordinator, UiCommand, UiEvent};
