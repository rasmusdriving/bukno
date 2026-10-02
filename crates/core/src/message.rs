//! Messages, their delivery record, and transcript items.

use crate::ids::{ItemId, RunId, TaskId};

/// Outbox state of one user message (section 6, Message delivery).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    /// Recorded locally; the engine write has not been confirmed yet.
    AboutToSend,
    /// Written to the engine, no acknowledgement yet.
    Sent,
    Acknowledged,
    Rejected,
    /// The connection dropped before the outcome was known. Never resent automatically.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Codex,
    Claude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    UserMessage,
    AgentMessage { provider: Provider },
}

/// One entry in Bukno's own transcript store, the display source for a chat.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptItem {
    pub id: ItemId,
    pub task: TaskId,
    pub run: Option<RunId>,
    pub kind: ItemKind,
    /// Plain text for user messages, Markdown source for agent replies.
    pub text: String,
    /// Optional meta shown beside the provider name, such as the model.
    pub meta: Option<String>,
    pub completed: bool,
    /// Increases on every change so views can skip stale or repeated updates.
    pub revision: u64,
}
