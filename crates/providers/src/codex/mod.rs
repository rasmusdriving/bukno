//! The Codex app-server adapter (specification section 9).

pub mod adapter;
pub mod presets;
pub mod protocol;
pub mod transport;

pub use adapter::{CodexAdapter, Resolve, ShutdownReport, Status, StatusFn};
pub use transport::Launch;
