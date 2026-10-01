//! Inputs the coordinator reads from its one inbox, and the effects it asks
//! the runtime to perform.

use crate::ids::{ItemId, MessageId, RunId, TaskId};
use crate::message::{DeliveryState, Provider, TranscriptItem};
use crate::run::{RunOutcome, RunState};

/// Everything the coordinator reacts to arrives as one of these, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Command(Command),
    Engine(EngineEvent),
    Stored(StorageResult),
}

/// Commands from the interface. Identifiers are captured by the runtime when
/// the user acts, so a command never looks up the currently selected chat.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    SubmitMessage {
        task: TaskId,
        provider: Provider,
        message: MessageId,
        /// The run this submission will start if accepted.
        run: RunId,
        /// The transcript item that shows the user's message.
        item: ItemId,
        draft_revision: u64,
        body: String,
    },
    InterruptRun {
        task: TaskId,
        run: RunId,
    },
}

/// A normalized event from an engine connection.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineEvent {
    /// Changes every time the engine process restarts. Events from an older
    /// connection are ignored.
    pub connection_generation: u64,
    pub kind: EngineEventKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEventKind {
    /// A connection to the engine is ready. Turns are written only on a
    /// connection the coordinator has seen established.
    Connected,
    RunAccepted {
        run: RunId,
    },
    TextDelta {
        run: RunId,
        item: ItemId,
        delta: String,
    },
    ItemCompleted {
        run: RunId,
        item: ItemId,
    },
    RunEnded {
        run: RunId,
        outcome: RunOutcome,
    },
    ConnectionLost,
}

/// Results reported back by the storage worker.
#[derive(Clone, Debug, PartialEq)]
pub enum StorageResult {
    /// The "about to send" delivery record and run intent were committed.
    DeliveryRecorded { message: MessageId },
    /// The write failed. Nothing may be sent to the engine for this message.
    DeliveryFailed { message: MessageId, reason: String },
    /// A page of transcript items for a chat, oldest first.
    HistoryLoaded { task: TaskId, items: Vec<TranscriptItem> },
}

/// Work the runtime performs on the coordinator's behalf.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Persist(PersistRequest),
    Engine(EngineRequest),
    Publish(ViewUpdate),
    /// A command was refused locally. The draft stays editable.
    Rejected {
        task: TaskId,
        reason: RejectReason,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum PersistRequest {
    /// Record the message as "about to send" together with the run intent, in one transaction.
    RecordDelivery {
        task: TaskId,
        run: RunId,
        message: MessageId,
        draft_revision: u64,
        body: String,
    },
    DeliveryState {
        message: MessageId,
        state: DeliveryState,
    },
    RunState {
        run: RunId,
        state: RunState,
    },
    /// Upsert a transcript item. Sent in batches while streaming, not per token.
    Item(TranscriptItem),
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineRequest {
    StartTurn { task: TaskId, run: RunId, message: MessageId, body: String },
    Interrupt { run: RunId },
}

/// Prepared view state for the interface. No transport objects.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewUpdate {
    ConversationLoaded { task: TaskId, items: Vec<TranscriptItem> },
    ItemUpserted(TranscriptItem),
    RunStateChanged { task: TaskId, run: RunId, state: RunState },
    DeliveryChanged { task: TaskId, message: MessageId, state: DeliveryState },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    EmptyMessage,
    /// The same message ID was already accepted (a repeated click or retry).
    DuplicateMessage,
    /// The chat already has active work. Steering and queueing land in Pass 1.
    RunActive,
    UnknownRun,
    /// The local delivery record could not be saved, so nothing was sent.
    NotSaved(String),
}
