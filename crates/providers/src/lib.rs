//! Engine adapters: the Codex app-server adapter (Pass 1) and, in Pass 2,
//! the Claude bridge adapter.
//!
//! Adapters report what the engine says and do what the coordinator asks.
//! They never choose which chat is selected or substitute another model or
//! provider after an error.

pub mod codex;
