//! Pure Bukno domain types and the coordinator's decision function.
//!
//! Nothing in this crate performs I/O, reads a clock, generates random IDs or
//! depends on egui. The runtime supplies identifiers and results as inputs and
//! executes the effects that [`machine::Machine::step`] returns, so the same
//! logic runs unchanged in the app, in replay and in the end-to-end harness.

pub mod event;
pub mod ids;
pub mod machine;
pub mod message;
pub mod run;
