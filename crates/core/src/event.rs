//! Inputs the coordinator reads from its one inbox, and the effects it asks
//! the runtime to perform.

use crate::decision::{DecisionAnswer, DecisionKind, DecisionState, DecisionView};
use crate::ids::{DecisionId, ItemId, MessageId, ProjectId, RunId, TaskId, WorkspaceId};
use crate::message::{DeliveryState, Provider, TranscriptItem};
use crate::run::{RunOutcome, RunState};
use crate::task::{ProjectInfo, RunSettings, SessionInfo, TaskInfo, WorkspaceInfo};

/// Everything the coordinator reacts to arrives as one of these, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Command(Command),
    Engine(EngineEvent),
    Stored(StorageResult),
    /// A timer the coordinator asked for with [`Effect::Timer`] has fired.
    Timer(TimerKey),
}

/// Commands from the interface. Identifiers are captured by the runtime when
/// the user acts, so a command never looks up the currently selected chat.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Create a chat. The runtime has already prepared its folder.
    CreateChat {
        task: TaskInfo,
        workspace: WorkspaceInfo,
    },
    AddProject {
        project: ProjectInfo,
        workspace: WorkspaceInfo,
    },
    SelectChat {
        task: TaskId,
    },
    SaveDraft {
        task: TaskId,
        revision: u64,
        text: String,
    },
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
        settings: RunSettings,
    },
    AnswerDecision {
        decision: DecisionId,
        run: RunId,
        generation: u64,
        answer: DecisionAnswer,
    },
    InterruptRun {
        task: TaskId,
        run: RunId,
    },
    /// Stop is taking too long: end the engine process. Affects every chat on it.
    ForceStop {
        task: TaskId,
        run: RunId,
    },
    /// Run although another chat holds the workspace. Both runs are labeled.
    RunAnyway {
        run: RunId,
    },
    /// Send a queued message that was paused (after a restart or a failure).
    SendQueued {
        run: RunId,
    },
    /// Resend a message whose outcome is unknown. May duplicate work; the
    /// interface explains that before sending this.
    Resend {
        unknown: RunId,
        message: MessageId,
        run: RunId,
        item: ItemId,
        settings: RunSettings,
    },
    /// Stop waiting for an unknown outcome and release its workspace.
    Dismiss {
        run: RunId,
    },
    /// Quit. With `stop`, active runs are stopped first.
    Quit {
        stop: bool,
    },
}

/// A normalized event from an engine connection.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineEvent {
    pub provider: Provider,
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
    /// The engine could not be started or did not complete its handshake.
    ConnectFailed {
        reason: String,
    },
    /// The chat's provider session is known (new, or a new ID after resume).
    SessionReady {
        task: TaskId,
        thread: String,
        model: Option<String>,
    },
    RunAccepted {
        run: RunId,
        turn: String,
    },
    /// The engine refused the turn before doing any work.
    RunRejected {
        run: RunId,
        reason: String,
    },
    TextDelta {
        run: RunId,
        item: ItemId,
        provider_item: String,
        delta: String,
    },
    ItemCompleted {
        run: RunId,
        item: ItemId,
        provider_item: String,
        text: Option<String>,
    },
    /// What the agent is doing right now, in words. Never invented.
    Activity {
        run: RunId,
        text: String,
    },
    DecisionRequested {
        run: RunId,
        decision: DecisionId,
        kind: DecisionKind,
    },
    /// The engine confirmed it has the answer.
    DecisionResolved {
        decision: DecisionId,
    },
    /// The engine did not confirm a turn it was sent. Its outcome is unknown.
    RunLost {
        run: RunId,
        reason: String,
    },
    RunEnded {
        run: RunId,
        outcome: RunOutcome,
        reason: Option<String>,
    },
    /// Result of looking for a run's message in the provider's history.
    Reconciled {
        run: RunId,
        result: Reconciliation,
    },
    /// Result of comparing the provider's history with what Bukno last saw.
    /// `items` holds turns Bukno missed (the chat continued outside Bukno);
    /// empty when nothing changed.
    OutsideChecked {
        task: TaskId,
        latest_turn: Option<String>,
        items: Vec<ImportedItem>,
    },
    /// The history could not be read, so nobody knows whether the chat
    /// continued outside Bukno. Never treated as "nothing changed".
    OutsideCheckFailed {
        task: TaskId,
        reason: String,
    },
    /// Something the user should know about a run, such as a request Bukno
    /// could not answer.
    Notice {
        run: RunId,
        text: String,
    },
    ConnectionLost {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reconciliation {
    /// The engine has the message; the turn ended with `outcome` (None while
    /// it is still in progress).
    Found {
        turn: String,
        outcome: Option<RunOutcome>,
        items: Vec<ImportedItem>,
    },
    NotFound,
    /// History could not be read, for example because the thread is gone.
    Unavailable {
        reason: String,
    },
}

/// A message read back from provider history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedItem {
    pub provider_item: String,
    pub user: bool,
    pub text: String,
}

/// Results reported back by the storage worker.
#[derive(Clone, Debug, PartialEq)]
pub enum StorageResult {
    /// Everything needed to resume after launch.
    Restored(Snapshot),
    /// A chat or project was saved; it can be shown.
    ChatSaved {
        task: TaskId,
    },
    ProjectSaved {
        project: ProjectId,
    },
    /// The delivery record for this message, in the state the coordinator
    /// asked for, was committed.
    DeliveryRecorded {
        message: MessageId,
    },
    /// The write failed. Nothing may be sent to the engine for this message.
    DeliveryFailed {
        message: MessageId,
        reason: String,
    },
    DraftSaved {
        task: TaskId,
        revision: u64,
    },
    DraftFailed {
        task: TaskId,
        reason: String,
    },
    /// A chat's transcript, oldest first, and its saved draft.
    HistoryLoaded {
        task: TaskId,
        items: Vec<TranscriptItem>,
        draft: Option<(String, u64)>,
    },
    /// Some other write failed. Dispatch stops until the store recovers.
    WriteFailed {
        reason: String,
    },
}

/// Durable state loaded at launch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub workspaces: Vec<WorkspaceInfo>,
    pub projects: Vec<ProjectInfo>,
    pub tasks: Vec<TaskInfo>,
    /// Runs that had not settled, and unknown outcomes not yet dismissed.
    pub open_runs: Vec<OpenRun>,
    /// Decisions still marked pending. They expire at launch.
    pub decisions: Vec<DecisionId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenRun {
    pub run: RunId,
    pub task: TaskId,
    pub state: RunState,
    pub settings: RunSettings,
    pub turn: Option<String>,
    pub message: MessageId,
    pub delivery: DeliveryState,
    pub body: String,
}

/// Timers the coordinator can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimerKey {
    /// Stop has not settled within the grace period.
    StopGrace(RunId),
    /// Quit has not settled within the grace period.
    QuitGrace,
}

/// Work the runtime performs on the coordinator's behalf.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Persist(PersistRequest),
    Load(LoadRequest),
    Engine(Provider, EngineRequest),
    Publish(ViewUpdate),
    Timer {
        key: TimerKey,
        after_ms: u64,
    },
    /// A command was refused locally. The draft stays editable.
    Rejected {
        task: TaskId,
        reason: RejectReason,
    },
    /// Every run has settled after Quit. The runtime may close engines and exit.
    QuitReady,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PersistRequest {
    Workspace(WorkspaceInfo),
    Project(ProjectInfo),
    Chat(TaskInfo),
    Title {
        task: TaskId,
        title: String,
    },
    Session {
        task: TaskId,
        session: SessionInfo,
    },
    /// In one transaction: the run and its settings, the delivery record in
    /// `state`, the user's transcript item, and clearing the draft up to
    /// `draft_revision`.
    RecordDelivery {
        task: TaskId,
        run: RunId,
        message: MessageId,
        state: DeliveryState,
        draft_revision: u64,
        body: String,
        settings: RunSettings,
        item: TranscriptItem,
    },
    /// Move a queued delivery to "about to send". Acknowledged with
    /// [`StorageResult::DeliveryRecorded`]; the engine write waits for it.
    MarkAboutToSend {
        message: MessageId,
    },
    DeliveryState {
        message: MessageId,
        state: DeliveryState,
    },
    RunState {
        run: RunId,
        state: RunState,
    },
    RunTurn {
        run: RunId,
        turn: String,
    },
    /// The user stopped waiting for this unknown outcome.
    RunDismissed {
        run: RunId,
    },
    /// Upsert a transcript item. Sent in batches while streaming, not per token.
    Item {
        item: TranscriptItem,
        provider_item: Option<String>,
    },
    /// Add or update items read from provider history, matched by provider
    /// item ID. New items go just before `before` when it is set, otherwise
    /// at the end.
    ImportItems {
        task: TaskId,
        run: Option<RunId>,
        items: Vec<ImportedItem>,
        before: Option<ItemId>,
    },
    Draft {
        task: TaskId,
        revision: u64,
        text: String,
    },
    Decision {
        decision: DecisionView,
    },
    DecisionState {
        decision: DecisionId,
        state: DecisionState,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadRequest {
    History { task: TaskId },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineRequest {
    /// Start the engine if it is not running. Answered with Connected or ConnectFailed.
    Connect,
    StartTurn {
        task: TaskId,
        run: RunId,
        message: MessageId,
        body: String,
        /// The chat's existing provider session, resumed if this connection has not loaded it.
        session: Option<SessionInfo>,
        cwd: String,
        settings: RunSettings,
    },
    Interrupt {
        run: RunId,
    },
    Answer {
        decision: DecisionId,
        answer: DecisionAnswer,
    },
    /// Look for `message` in the session's history and report how its turn ended.
    Reconcile {
        run: RunId,
        message: MessageId,
        session: SessionInfo,
        cwd: String,
    },
    /// Compare the provider's latest turn with what Bukno last saw.
    CheckOutside {
        task: TaskId,
        session: SessionInfo,
        cwd: String,
    },
    /// End the engine process and its tools now.
    Kill,
}

/// Prepared view state for the interface. No transport objects.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewUpdate {
    /// The sidebar: projects and chats, newest chat first.
    Chats {
        projects: Vec<ProjectView>,
        chats: Vec<ChatSummary>,
    },
    ConversationLoaded {
        task: TaskId,
        items: Vec<TranscriptItem>,
    },
    DraftLoaded {
        task: TaskId,
        text: String,
        revision: u64,
    },
    DraftSaved {
        task: TaskId,
        revision: u64,
    },
    DraftFailed {
        task: TaskId,
        reason: String,
    },
    ItemUpserted(TranscriptItem),
    RunStateChanged {
        task: TaskId,
        run: RunId,
        state: RunState,
    },
    /// Why a submitted message has not been sent yet. None once it is on its way.
    RunWaiting {
        task: TaskId,
        run: RunId,
        reason: Option<WaitReason>,
    },
    DeliveryChanged {
        task: TaskId,
        message: MessageId,
        state: DeliveryState,
    },
    Activity {
        task: TaskId,
        run: RunId,
        text: String,
    },
    Decisions {
        task: TaskId,
        decisions: Vec<DecisionView>,
    },
    /// Model and settings the engine reported for this chat.
    SessionModel {
        task: TaskId,
        model: String,
    },
    /// Something to say in the chat, such as why a run failed.
    Notice {
        task: TaskId,
        notice: Notice,
    },
    /// Stop has not settled after the grace period. `shared` names the other
    /// chats a force stop would also end.
    StopSlow {
        task: TaskId,
        run: RunId,
        shared: Vec<String>,
    },
    /// A run whose outcome is unknown, with what can be done about it.
    Unknown {
        task: TaskId,
        run: RunId,
        body: String,
        explanation: String,
    },
    UnknownCleared {
        task: TaskId,
        run: RunId,
    },
    /// Durable state could not be saved; sending is paused.
    StorageProblem {
        reason: String,
    },
    QuitReady,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectView {
    pub id: ProjectId,
    pub name: String,
    pub path: String,
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSummary {
    pub task: TaskId,
    pub title: String,
    pub provider: Provider,
    pub project: Option<ProjectId>,
    pub workspace: WorkspaceId,
    pub path: String,
    pub available: bool,
    pub activity: ChatActivity,
    /// Shown when the chat runs alongside another writer in the same folder.
    pub shared_workspace: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatActivity {
    Idle,
    Queued,
    Working,
    NeedsYou,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitReason {
    /// Another chat is changing files in the same folder.
    Workspace { holder: TaskId, holder_title: String },
    /// The limit of active runs is reached.
    Slots { limit: usize },
    /// Paused after a restart or failure; the user sends it explicitly.
    Paused { why: String },
    /// Waiting for the engine to start.
    Engine,
    /// Checking whether the chat continued outside Bukno before sending.
    Checking,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub tone: NoticeTone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTone {
    Info,
    Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    EmptyMessage,
    /// The same message ID was already accepted (a repeated click or retry).
    DuplicateMessage,
    /// The chat already has active work. Mid-run direction lands later.
    RunActive,
    UnknownRun,
    UnknownChat,
    /// The chat's folder is missing.
    Unavailable,
    /// The chat belongs to the other provider (fixed after the first message).
    WrongProvider,
    /// The decision was already answered, expired, or came from an older connection.
    StaleDecision,
    /// The local delivery record could not be saved, so nothing was sent.
    NotSaved(String),
    /// Saving is failing, so nothing new is sent until it works again.
    StorageUnsafe,
    Quitting,
}
