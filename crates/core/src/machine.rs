//! The coordinator's decision function: current state plus one input gives
//! the new state and a list of effects.
//!
//! All lifecycle, delivery and recovery rules from sections 8, 11 and 12 of
//! the specification live here: the outbox, the run queue and its limit,
//! the writer lease, decisions, Stop, connection loss, restart and
//! reconciliation. Adapters and widgets only report and display.

use std::collections::{HashMap, HashSet};

use crate::decision::{DecisionKind, DecisionState, DecisionView};
use crate::event::{
    ChatActivity, ChatSummary, Command, Effect, EngineEvent, EngineEventKind, EngineRequest, ImportedItem, Input,
    LoadRequest, Notice, NoticeTone, OpenRun, PersistRequest, ProjectView, Reconciliation, RejectReason, Snapshot,
    StorageResult, TimerKey, ViewUpdate, WaitReason,
};
use crate::ids::{DecisionId, ItemId, MessageId, RunId, TaskId, WorkspaceId};
use crate::message::{DeliveryState, ItemKind, Provider, TranscriptItem};
use crate::run::{RunOutcome, RunState};
use crate::task::{ProjectInfo, RunSettings, SessionInfo, TaskInfo, WorkspaceInfo, Writes};

/// Default number of top-level runs that may be active at once (section 12).
pub const DEFAULT_RUN_LIMIT: usize = 2;
/// How long Stop may take before the chat offers Force stop (section 16).
pub const STOP_GRACE_MS: u64 = 5_000;
/// How long Quit waits for runs to stop before ending the engines.
pub const QUIT_GRACE_MS: u64 = 5_000;
/// Reconnects after a lost connection before Bukno waits for the user.
const AUTO_RECONNECTS: u32 = 2;
const TITLE_CHARS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lease {
    None,
    Held,
    /// Held alongside another writer after Run anyway.
    Shared,
}

#[derive(Debug)]
struct Task {
    info: TaskInfo,
    /// Committed to storage, so it can be shown.
    saved: bool,
    /// The newest run, which decides the chat's state.
    latest: Option<RunId>,
    /// Connection generation on which outside continuation was last checked.
    outside_checked: Option<u64>,
    /// Connection generation on which a check was asked for and not yet answered.
    outside_pending: Option<u64>,
}

#[derive(Debug)]
struct Run {
    task: TaskId,
    provider: Provider,
    message: MessageId,
    settings: RunSettings,
    state: RunState,
    /// Connection the turn was written to, once it was written.
    generation: Option<u64>,
    turn: Option<String>,
    /// Reply items being streamed. Dropped when the run settles; history
    /// stays in storage, not in the coordinator.
    reply_items: Vec<ItemId>,
    lease: Lease,
    run_anyway: bool,
    /// Queued and waiting for the user's go-ahead, with the reason in words.
    paused: Option<String>,
    /// Stop arrived before the turn was written.
    cancel_requested: bool,
    /// Last waiting reason published, to avoid repeating it.
    wait: Option<WaitReason>,
    /// The user stopped waiting for this unknown outcome.
    dismissed: bool,
    needs_reconcile: bool,
}

impl Run {
    fn holds_lease(&self) -> bool {
        self.lease != Lease::None
            && (!self.state.is_terminal() || (self.state == RunState::OutcomeUnknown && !self.dismissed))
    }
}

#[derive(Debug)]
struct Delivery {
    task: TaskId,
    run: RunId,
    state: DeliveryState,
    body: String,
    /// The record in its current state is committed; the turn may be written.
    recorded: bool,
    /// The user's transcript item, published once its record commits.
    item: Option<TranscriptItem>,
    item_id: Option<ItemId>,
}

#[derive(Debug)]
struct Streaming {
    item: TranscriptItem,
    provider_item: String,
}

#[derive(Debug)]
pub struct Machine {
    /// The live connection per engine, set by `Connected` and cleared by
    /// `ConnectionLost`. Nothing is written to an engine without one.
    connections: HashMap<Provider, u64>,
    /// The newest generation ever connected, kept while disconnected.
    /// Generations only increase, so a `Connected` at or below it is late.
    newest: HashMap<Provider, u64>,
    connecting: HashSet<Provider>,
    reconnects: HashMap<Provider, u32>,
    workspaces: HashMap<WorkspaceId, WorkspaceInfo>,
    projects: Vec<(ProjectInfo, bool)>,
    tasks: HashMap<TaskId, Task>,
    runs: HashMap<RunId, Run>,
    /// Runs not yet sent, in submission order.
    queue: Vec<RunId>,
    deliveries: HashMap<MessageId, Delivery>,
    decisions: HashMap<DecisionId, DecisionView>,
    items: HashMap<ItemId, Streaming>,
    selected: Option<TaskId>,
    limit: usize,
    storage_ok: bool,
    quitting: bool,
    quit_ready: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_RUN_LIMIT)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            connections: HashMap::new(),
            newest: HashMap::new(),
            connecting: HashSet::new(),
            reconnects: HashMap::new(),
            workspaces: HashMap::new(),
            projects: Vec::new(),
            tasks: HashMap::new(),
            runs: HashMap::new(),
            queue: Vec::new(),
            deliveries: HashMap::new(),
            decisions: HashMap::new(),
            items: HashMap::new(),
            selected: None,
            limit: limit.max(1),
            storage_ok: true,
            quitting: false,
            quit_ready: false,
        }
    }

    /// The state of a run, if the coordinator knows it.
    pub fn run_state(&self, run: RunId) -> Option<RunState> {
        self.runs.get(&run).map(|r| r.state)
    }

    pub fn delivery_state(&self, message: MessageId) -> Option<DeliveryState> {
        self.deliveries.get(&message).map(|d| d.state)
    }

    pub fn decision_state(&self, decision: DecisionId) -> Option<DecisionState> {
        self.decisions.get(&decision).map(|d| d.state)
    }

    /// Whether a run currently holds its workspace's writer lease.
    pub fn holds_lease(&self, run: RunId) -> bool {
        self.runs.get(&run).is_some_and(Run::holds_lease)
    }

    pub fn task(&self, task: TaskId) -> Option<&TaskInfo> {
        self.tasks.get(&task).map(|t| &t.info)
    }

    /// A project and its workspace.
    pub fn project(&self, project: crate::ids::ProjectId) -> Option<(&ProjectInfo, &WorkspaceInfo)> {
        let (info, _) = self.projects.iter().find(|(p, _)| p.id == project)?;
        Some((info, self.workspaces.get(&info.workspace)?))
    }

    /// A known workspace for the same folder: same normalized path or same
    /// filesystem identity (a second path to one folder).
    pub fn same_workspace(&self, folder: &WorkspaceInfo) -> Option<&WorkspaceInfo> {
        self.workspaces
            .values()
            .find(|w| w.key == folder.key || (w.identity.is_some() && w.identity == folder.identity))
    }

    /// Handle one input to completion.
    pub fn step(&mut self, input: Input) -> Vec<Effect> {
        let mut fx = Vec::new();
        match input {
            Input::Command(command) => self.on_command(command, &mut fx),
            Input::Engine(event) => self.on_engine(event, &mut fx),
            Input::Stored(result) => self.on_stored(result, &mut fx),
            Input::Timer(key) => self.on_timer(key, &mut fx),
        }
        fx
    }

    // ----- Commands -------------------------------------------------------

    fn on_command(&mut self, command: Command, fx: &mut Vec<Effect>) {
        match command {
            Command::CreateChat { task, workspace } => {
                self.add_workspace(workspace, fx);
                fx.push(Effect::Persist(PersistRequest::Chat(task.clone())));
                self.tasks.insert(
                    task.id,
                    Task { info: task, saved: false, latest: None, outside_checked: None, outside_pending: None },
                );
            }
            Command::AddProject { project, workspace } => {
                self.add_workspace(workspace, fx);
                fx.push(Effect::Persist(PersistRequest::Project(project.clone())));
                self.projects.push((project, false));
            }
            Command::SelectChat { task } => {
                self.selected = Some(task);
                fx.push(Effect::Load(LoadRequest::History { task }));
                self.republish_task(task, fx);
                self.check_outside(task, false, fx);
            }
            Command::SaveDraft { task, revision, text } => {
                fx.push(Effect::Persist(PersistRequest::Draft { task, revision, text }));
            }
            Command::SubmitMessage { task, provider, message, run, item, draft_revision, body, settings } => {
                self.submit(task, provider, message, run, item, draft_revision, body, settings, fx);
            }
            Command::AnswerDecision { decision, run, generation, answer } => {
                let Some(view) = self.decisions.get_mut(&decision) else {
                    return;
                };
                let task = view.task;
                let live = self.runs.get(&run).filter(|r| !r.state.is_terminal() && r.state != RunState::Cancelling);
                let current = live.and_then(|r| self.connections.get(&r.provider)).copied();
                if view.run != run
                    || view.generation != generation
                    || view.state != DecisionState::Pending
                    || current != Some(generation)
                {
                    fx.push(Effect::Rejected { task, reason: RejectReason::StaleDecision });
                    return;
                }
                let provider = live.map(|r| r.provider).unwrap_or(Provider::Codex);
                view.state = DecisionState::Sending;
                fx.push(Effect::Engine(provider, EngineRequest::Answer { decision, answer }));
                fx.push(Effect::Persist(PersistRequest::DecisionState { decision, state: DecisionState::Sending }));
                self.publish_decisions(task, fx);
            }
            Command::InterruptRun { task, run } => {
                if self.runs.get(&run).is_none_or(|r| r.task != task) {
                    fx.push(Effect::Rejected { task, reason: RejectReason::UnknownRun });
                    return;
                }
                self.interrupt(run, fx);
            }
            Command::ForceStop { task, run } => {
                let Some(record) = self.runs.get(&run).filter(|r| r.task == task && !r.state.is_terminal()) else {
                    return;
                };
                fx.push(Effect::Engine(record.provider, EngineRequest::Kill));
            }
            Command::RunAnyway { run } => {
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state != RunState::Preparing {
                    return;
                }
                record.run_anyway = true;
                self.dispatch_queue(fx);
                self.publish_chats(fx);
            }
            Command::SendQueued { run } => {
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state != RunState::Preparing || record.paused.is_none() {
                    return;
                }
                if !self.storage_ok {
                    fx.push(Effect::Rejected { task: record.task, reason: RejectReason::StorageUnsafe });
                    return;
                }
                record.paused = None;
                self.dispatch_queue(fx);
            }
            Command::Resend { unknown, message, run, item, settings } => {
                let Some(old) = self.runs.get(&unknown).filter(|r| r.state == RunState::OutcomeUnknown) else {
                    return;
                };
                let (task, provider) = (old.task, old.provider);
                let Some(body) = self.deliveries.get(&old.message).map(|d| d.body.clone()) else {
                    return;
                };
                self.dismiss(unknown, fx);
                self.submit(task, provider, message, run, item, 0, body, settings, fx);
            }
            Command::Dismiss { run } => {
                if self.runs.get(&run).is_some_and(|r| r.state == RunState::OutcomeUnknown && !r.dismissed) {
                    self.dismiss(run, fx);
                    self.dispatch_queue(fx);
                    self.publish_chats(fx);
                }
            }
            Command::Quit { stop } => {
                self.quitting = true;
                if stop {
                    let active: Vec<RunId> =
                        self.runs.iter().filter(|(_, r)| is_active(r, &self.deliveries)).map(|(id, _)| *id).collect();
                    for run in active {
                        self.interrupt(run, fx);
                    }
                }
                if self.active_count(None) == 0 {
                    self.quit_ready(fx);
                } else {
                    fx.push(Effect::Timer { key: TimerKey::QuitGrace, after_ms: QUIT_GRACE_MS });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn submit(
        &mut self,
        task: TaskId,
        provider: Provider,
        message: MessageId,
        run: RunId,
        item: ItemId,
        draft_revision: u64,
        body: String,
        settings: RunSettings,
        fx: &mut Vec<Effect>,
    ) {
        if body.trim().is_empty() {
            fx.push(Effect::Rejected { task, reason: RejectReason::EmptyMessage });
            return;
        }
        // Deduplicate repeated clicks and retries by message ID before transport.
        if self.deliveries.contains_key(&message) || self.runs.contains_key(&run) {
            fx.push(Effect::Rejected { task, reason: RejectReason::DuplicateMessage });
            return;
        }
        if self.quitting {
            fx.push(Effect::Rejected { task, reason: RejectReason::Quitting });
            return;
        }
        if !self.storage_ok {
            fx.push(Effect::Rejected { task, reason: RejectReason::StorageUnsafe });
            return;
        }
        let Some(record) = self.tasks.get(&task) else {
            fx.push(Effect::Rejected { task, reason: RejectReason::UnknownChat });
            return;
        };
        if record.info.provider != provider {
            fx.push(Effect::Rejected { task, reason: RejectReason::WrongProvider });
            return;
        }
        if !self.workspaces.get(&record.info.workspace).is_some_and(|w| w.available) {
            fx.push(Effect::Rejected { task, reason: RejectReason::Unavailable });
            return;
        }
        if let Some(latest) = record.latest.and_then(|id| self.runs.get(&id).map(|r| (id, r))) {
            if !latest.1.state.is_terminal() {
                fx.push(Effect::Rejected { task, reason: RejectReason::RunActive });
                return;
            }
            // A new message supersedes this chat's own unknown outcome.
            if latest.1.state == RunState::OutcomeUnknown && !latest.1.dismissed {
                self.dismiss(latest.0, fx);
            }
        }

        if let Some(entry) = self.tasks.get_mut(&task)
            && entry.info.title.trim().is_empty()
        {
            entry.info.title = title_from(&body);
            fx.push(Effect::Persist(PersistRequest::Title { task, title: entry.info.title.clone() }));
        }
        if let Some(entry) = self.tasks.get_mut(&task) {
            entry.latest = Some(run);
        }
        self.runs.insert(
            run,
            Run {
                task,
                provider,
                message,
                settings: settings.clone(),
                state: RunState::Preparing,
                generation: None,
                turn: None,
                reply_items: Vec::new(),
                lease: Lease::None,
                run_anyway: false,
                paused: None,
                cancel_requested: false,
                wait: None,
                dismissed: false,
                needs_reconcile: false,
            },
        );
        let user_item = TranscriptItem {
            id: item,
            task,
            run: Some(run),
            kind: ItemKind::UserMessage,
            text: body.clone(),
            meta: None,
            completed: true,
            revision: 1,
        };
        let wait = self.wait_reason(run);
        let state = if wait.is_none() { DeliveryState::AboutToSend } else { DeliveryState::Queued };
        if wait.is_none() {
            self.take_lease(run);
        } else {
            self.queue.push(run);
        }
        self.deliveries.insert(
            message,
            Delivery {
                task,
                run,
                state,
                body: body.clone(),
                recorded: false,
                item_id: Some(item),
                item: Some(user_item.clone()),
            },
        );
        // Outbox: the delivery is recorded before anything goes to the engine.
        fx.push(Effect::Persist(PersistRequest::RecordDelivery {
            task,
            run,
            message,
            state,
            draft_revision,
            body,
            settings,
            item: user_item,
        }));
        fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Preparing }));
        self.publish_wait(run, wait, fx);
        self.publish_chats(fx);
    }

    fn interrupt(&mut self, run: RunId, fx: &mut Vec<Effect>) {
        let Some(record) = self.runs.get_mut(&run) else {
            return;
        };
        let task = record.task;
        match record.state {
            RunState::Preparing => {
                let delivery = self.deliveries.get(&record.message);
                let written = delivery.is_some_and(|d| d.state == DeliveryState::Sent);
                let recorded = delivery.is_some_and(|d| d.recorded);
                if written {
                    return;
                }
                if delivery.is_some_and(|d| d.state == DeliveryState::AboutToSend) && !recorded {
                    // The commit is in flight; it settles when it lands.
                    record.cancel_requested = true;
                    fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Cancelling }));
                    return;
                }
                // Queued, or recorded and waiting for the engine: never written.
                let message = record.message;
                self.withdraw(run, message, "Stopped before it was sent.", fx);
            }
            RunState::Starting | RunState::Running | RunState::WaitingForApproval | RunState::WaitingForInput => {
                // Enter Cancelling immediately; the engine's acknowledgement alone
                // does not prove the work stopped, so the run settles on RunEnded.
                record.state = RunState::Cancelling;
                let provider = record.provider;
                fx.push(Effect::Engine(provider, EngineRequest::Interrupt { run }));
                fx.push(Effect::Persist(PersistRequest::RunState { run, state: RunState::Cancelling }));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Cancelling }));
                fx.push(Effect::Timer { key: TimerKey::StopGrace(run), after_ms: STOP_GRACE_MS });
                self.expire_decisions(run, fx);
                self.publish_chats(fx);
            }
            _ => {}
        }
    }

    /// Settle a run that was never written to an engine.
    fn withdraw(&mut self, run: RunId, message: MessageId, why: &str, fx: &mut Vec<Effect>) {
        self.queue.retain(|r| *r != run);
        if let Some(delivery) = self.deliveries.get_mut(&message) {
            delivery.state = DeliveryState::Rejected;
            let task = delivery.task;
            fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Rejected }));
            fx.push(Effect::Publish(ViewUpdate::DeliveryChanged { task, message, state: DeliveryState::Rejected }));
            fx.push(Effect::Publish(ViewUpdate::Notice {
                task,
                notice: Notice { text: why.to_owned(), tone: NoticeTone::Info },
            }));
        }
        self.settle(run, RunState::Interrupted, fx);
    }

    fn dismiss(&mut self, run: RunId, fx: &mut Vec<Effect>) {
        let Some(record) = self.runs.get_mut(&run) else {
            return;
        };
        record.dismissed = true;
        record.needs_reconcile = false;
        let task = record.task;
        fx.push(Effect::Persist(PersistRequest::RunDismissed { run }));
        fx.push(Effect::Publish(ViewUpdate::UnknownCleared { task, run }));
    }

    // ----- Storage --------------------------------------------------------

    fn on_stored(&mut self, result: StorageResult, fx: &mut Vec<Effect>) {
        match result {
            StorageResult::Restored(snapshot) => self.restore(snapshot, fx),
            StorageResult::ChatSaved { task } => {
                if let Some(entry) = self.tasks.get_mut(&task) {
                    entry.saved = true;
                }
                self.publish_chats(fx);
            }
            StorageResult::ProjectSaved { project } => {
                if let Some(entry) = self.projects.iter_mut().find(|(p, _)| p.id == project) {
                    entry.1 = true;
                }
                self.publish_chats(fx);
            }
            StorageResult::DeliveryRecorded { message } => {
                let Some(delivery) = self.deliveries.get_mut(&message) else {
                    return;
                };
                delivery.recorded = true;
                let (task, run, state) = (delivery.task, delivery.run, delivery.state);
                if let Some(item) = delivery.item.take() {
                    // Durable now, so the interface may show it (and clear the draft).
                    fx.push(Effect::Publish(ViewUpdate::ItemUpserted(item)));
                    fx.push(Effect::Publish(ViewUpdate::DeliveryChanged { task, message, state }));
                }
                if state != DeliveryState::AboutToSend {
                    return;
                }
                let Some(record) = self.runs.get(&run) else {
                    return;
                };
                if record.cancel_requested {
                    self.withdraw(run, message, "Stopped before it was sent.", fx);
                    return;
                }
                if record.state != RunState::Preparing {
                    return;
                }
                let provider = record.provider;
                match self.connections.get(&provider).copied() {
                    Some(generation) => self.start_turn(message, generation, fx),
                    None => {
                        self.connect(provider, fx);
                        self.publish_wait(run, Some(WaitReason::Engine), fx);
                    }
                }
            }
            StorageResult::DeliveryFailed { message, reason } => {
                let Some(delivery) = self.deliveries.get(&message) else {
                    return;
                };
                let (task, run, shown) = (delivery.task, delivery.run, delivery.item.is_none());
                self.queue.retain(|r| *r != run);
                if shown {
                    // A queued message could not move on. Nothing was sent.
                    self.storage_ok = false;
                    fx.push(Effect::Publish(ViewUpdate::StorageProblem { reason }));
                    self.settle(run, RunState::Failed, fx);
                    return;
                }
                // Never shown: forget it, so the draft is all that remains.
                self.deliveries.remove(&message);
                self.runs.remove(&run);
                if let Some(entry) = self.tasks.get_mut(&task)
                    && entry.latest == Some(run)
                {
                    entry.latest = None;
                }
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Failed }));
                fx.push(Effect::Rejected { task, reason: RejectReason::NotSaved(reason) });
                self.dispatch_queue(fx);
                self.publish_chats(fx);
            }
            StorageResult::DraftSaved { task, revision } => {
                fx.push(Effect::Publish(ViewUpdate::DraftSaved { task, revision }));
            }
            StorageResult::DraftFailed { task, reason } => {
                fx.push(Effect::Publish(ViewUpdate::DraftFailed { task, reason }));
            }
            StorageResult::HistoryLoaded { task, items, draft } => {
                fx.push(Effect::Publish(ViewUpdate::ConversationLoaded { task, items }));
                // Streaming text not yet written to storage is newer than what loaded.
                for streaming in self.items.values().filter(|s| s.item.task == task) {
                    fx.push(Effect::Publish(ViewUpdate::ItemUpserted(streaming.item.clone())));
                }
                if let Some((text, revision)) = draft {
                    fx.push(Effect::Publish(ViewUpdate::DraftLoaded { task, text, revision }));
                }
            }
            StorageResult::WriteFailed { reason } => {
                self.storage_ok = false;
                fx.push(Effect::Publish(ViewUpdate::StorageProblem { reason }));
            }
        }
    }

    fn restore(&mut self, snapshot: Snapshot, fx: &mut Vec<Effect>) {
        for workspace in snapshot.workspaces {
            self.workspaces.insert(workspace.id, workspace);
        }
        self.projects = snapshot.projects.into_iter().map(|p| (p, true)).collect();
        for task in snapshot.tasks {
            self.tasks.insert(
                task.id,
                Task { info: task, saved: true, latest: None, outside_checked: None, outside_pending: None },
            );
        }
        for decision in snapshot.decisions {
            fx.push(Effect::Persist(PersistRequest::DecisionState { decision, state: DecisionState::Expired }));
        }
        let mut reconnect = HashSet::new();
        for open in snapshot.open_runs {
            let OpenRun { run, task, state, settings, turn, message, delivery, body } = open;
            let Some(entry) = self.tasks.get_mut(&task) else {
                continue;
            };
            entry.latest = Some(run);
            let provider = entry.info.provider;
            let has_session = entry.info.session.is_some();
            let mut record = Run {
                task,
                provider,
                message,
                settings,
                state,
                generation: None,
                turn,
                reply_items: Vec::new(),
                lease: Lease::None,
                run_anyway: false,
                paused: None,
                cancel_requested: false,
                wait: None,
                dismissed: false,
                needs_reconcile: false,
            };
            let mut delivery_state = delivery;
            if delivery == DeliveryState::Queued {
                // Queued messages never replay just because Bukno relaunched.
                record.state = RunState::Preparing;
                record.paused = Some("Bukno was restarted before this was sent.".into());
                self.queue.push(run);
            } else {
                // Anything that may have reached the engine is unknown until
                // the provider's history says otherwise.
                if record.state != RunState::OutcomeUnknown {
                    record.state = RunState::OutcomeUnknown;
                    fx.push(Effect::Persist(PersistRequest::RunState { run, state: RunState::OutcomeUnknown }));
                }
                if matches!(delivery, DeliveryState::AboutToSend | DeliveryState::Sent) {
                    delivery_state = DeliveryState::Unknown;
                    fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Unknown }));
                }
                if record.settings.writes != Writes::Never {
                    record.lease = Lease::Held;
                }
                record.needs_reconcile = has_session;
                if has_session {
                    reconnect.insert(provider);
                }
            }
            let wait = record.paused.clone().map(|why| WaitReason::Paused { why });
            let is_unknown = record.state == RunState::OutcomeUnknown;
            self.runs.insert(run, record);
            self.deliveries.insert(
                message,
                Delivery {
                    task,
                    run,
                    state: delivery_state,
                    body: body.clone(),
                    recorded: true,
                    item: None,
                    item_id: None,
                },
            );
            fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: self.runs[&run].state }));
            if is_unknown {
                fx.push(Effect::Publish(ViewUpdate::Unknown {
                    task,
                    run,
                    body,
                    explanation: if has_session {
                        "Bukno closed while this was running. Checking what Codex finished…".into()
                    } else {
                        "Bukno closed before Codex confirmed this message. It may not have been delivered.".into()
                    },
                }));
            } else {
                self.publish_wait(run, wait, fx);
            }
        }
        for provider in reconnect {
            self.connect(provider, fx);
        }
        self.publish_chats(fx);
    }

    // ----- Engine events --------------------------------------------------

    fn on_engine(&mut self, event: EngineEvent, fx: &mut Vec<Effect>) {
        let EngineEvent { provider, connection_generation: generation, kind } = event;
        match kind {
            EngineEventKind::Connected => {
                // A repeat, or a late notice from a replaced or lost connection.
                if self.newest.get(&provider).is_some_and(|newest| generation <= *newest) {
                    return;
                }
                // A newer connection means the old one is gone.
                if let Some(current) = self.connections.get(&provider).copied() {
                    self.lose_connection(provider, current, "The engine was replaced.", fx);
                }
                self.connections.insert(provider, generation);
                self.newest.insert(provider, generation);
                self.connecting.remove(&provider);
                let waiting: Vec<MessageId> = self
                    .deliveries
                    .iter()
                    .filter(|(_, d)| d.recorded && d.state == DeliveryState::AboutToSend)
                    .filter(|(_, d)| self.runs.get(&d.run).is_some_and(|r| r.provider == provider))
                    .map(|(id, _)| *id)
                    .collect();
                for message in waiting {
                    self.start_turn(message, generation, fx);
                }
                self.reconcile_all(provider, fx);
                if let Some(task) = self.selected {
                    self.check_outside(task, false, fx);
                }
            }
            EngineEventKind::ConnectFailed { reason } => {
                self.connecting.remove(&provider);
                // Messages waiting for this engine go back to the queue, paused.
                let waiting: Vec<(RunId, MessageId)> = self
                    .runs
                    .iter()
                    .filter(|(_, r)| r.provider == provider && r.state == RunState::Preparing)
                    .filter(|(_, r)| {
                        self.deliveries.get(&r.message).is_some_and(|d| d.state == DeliveryState::AboutToSend)
                    })
                    .map(|(id, r)| (*id, r.message))
                    .collect();
                for (run, message) in waiting {
                    if let Some(record) = self.runs.get_mut(&run) {
                        record.lease = Lease::None;
                        record.paused = Some(reason.clone());
                    }
                    if let Some(delivery) = self.deliveries.get_mut(&message) {
                        delivery.state = DeliveryState::Queued;
                    }
                    self.queue.push(run);
                    fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Queued }));
                    self.publish_wait(run, Some(WaitReason::Paused { why: reason.clone() }), fx);
                }
                self.dispatch_queue(fx);
                self.publish_chats(fx);
            }
            EngineEventKind::ConnectionLost { reason } => {
                if self.connections.get(&provider) != Some(&generation) {
                    return; // Not the live connection: nothing of ours was on it.
                }
                self.connections.remove(&provider);
                self.connecting.remove(&provider);
                self.lose_connection(provider, generation, &reason, fx);
                // Reconnect to find out what happened, a bounded number of times.
                let needs = self.runs.values().any(|r| r.provider == provider && r.needs_reconcile);
                let count = self.reconnects.entry(provider).or_default();
                if needs && *count < AUTO_RECONNECTS && !self.quitting {
                    *count += 1;
                    self.connect(provider, fx);
                }
                self.publish_chats(fx);
            }
            EngineEventKind::SessionReady { task, thread, model } => {
                let Some(entry) = self.tasks.get_mut(&task) else {
                    return;
                };
                let latest_turn =
                    entry.info.session.as_ref().filter(|s| s.thread == thread).and_then(|s| s.latest_turn.clone());
                let session = SessionInfo { thread, latest_turn };
                entry.info.session = Some(session.clone());
                entry.outside_checked = Some(generation);
                fx.push(Effect::Persist(PersistRequest::Session { task, session }));
                if let Some(model) = model {
                    fx.push(Effect::Publish(ViewUpdate::SessionModel { task, model }));
                }
            }
            EngineEventKind::RunAccepted { run, turn } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state != RunState::Starting && record.state != RunState::Cancelling {
                    return;
                }
                if record.state == RunState::Starting {
                    record.state = RunState::Running;
                }
                record.turn = Some(turn.clone());
                let (task, message, state) = (record.task, record.message, record.state);
                if let Some(delivery) = self.deliveries.get_mut(&message) {
                    delivery.state = DeliveryState::Acknowledged;
                }
                self.set_latest_turn(task, turn.clone(), fx);
                fx.push(Effect::Persist(PersistRequest::RunTurn { run, turn }));
                fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Acknowledged }));
                fx.push(Effect::Persist(PersistRequest::RunState { run, state }));
                fx.push(Effect::Publish(ViewUpdate::DeliveryChanged {
                    task,
                    message,
                    state: DeliveryState::Acknowledged,
                }));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state }));
                self.reconnects.remove(&provider);
            }
            EngineEventKind::RunRejected { run, reason } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get(&run) else {
                    return;
                };
                let (task, message) = (record.task, record.message);
                if let Some(delivery) = self.deliveries.get_mut(&message) {
                    delivery.state = DeliveryState::Rejected;
                }
                fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Rejected }));
                fx.push(Effect::Publish(ViewUpdate::DeliveryChanged { task, message, state: DeliveryState::Rejected }));
                fx.push(Effect::Publish(ViewUpdate::Notice {
                    task,
                    notice: Notice {
                        text: format!("Codex did not accept this message: {reason}"),
                        tone: NoticeTone::Problem,
                    },
                }));
                self.settle(run, RunState::Failed, fx);
            }
            EngineEventKind::TextDelta { run, item, provider_item, delta } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state.is_terminal() {
                    return;
                }
                let entry = self.items.entry(item).or_insert_with(|| {
                    record.reply_items.push(item);
                    Streaming {
                        item: TranscriptItem {
                            id: item,
                            task: record.task,
                            run: Some(run),
                            kind: ItemKind::AgentMessage { provider: record.provider },
                            text: String::new(),
                            meta: None,
                            completed: false,
                            revision: 0,
                        },
                        provider_item,
                    }
                });
                if entry.item.completed {
                    return;
                }
                entry.item.text.push_str(&delta);
                entry.item.revision += 1;
                // The runtime coalesces both for display and storage; tokens are not
                // written one by one.
                fx.push(Effect::Persist(PersistRequest::Item {
                    item: entry.item.clone(),
                    provider_item: Some(entry.provider_item.clone()),
                }));
                fx.push(Effect::Publish(ViewUpdate::ItemUpserted(entry.item.clone())));
            }
            EngineEventKind::ItemCompleted { run, item, provider_item, text } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state.is_terminal() {
                    return;
                }
                if let std::collections::hash_map::Entry::Vacant(slot) = self.items.entry(item) {
                    let Some(text) = text.clone() else {
                        return;
                    };
                    record.reply_items.push(item);
                    slot.insert(Streaming {
                        item: TranscriptItem {
                            id: item,
                            task: record.task,
                            run: Some(run),
                            kind: ItemKind::AgentMessage { provider: record.provider },
                            text,
                            meta: None,
                            completed: false,
                            revision: 0,
                        },
                        provider_item: provider_item.clone(),
                    });
                }
                let Some(entry) = self.items.get_mut(&item).filter(|s| s.item.run == Some(run)) else {
                    return;
                };
                if entry.item.completed {
                    return;
                }
                // The completed item carries the full text; prefer it over the
                // deltas in case one was missed.
                if let Some(text) = text
                    && !text.is_empty()
                {
                    entry.item.text = text;
                }
                entry.item.completed = true;
                entry.item.revision += 1;
                let streaming = self.items.remove(&item).expect("present");
                if let Some(record) = self.runs.get_mut(&run) {
                    record.reply_items.retain(|i| *i != item);
                }
                fx.push(Effect::Persist(PersistRequest::Item {
                    item: streaming.item.clone(),
                    provider_item: Some(streaming.provider_item),
                }));
                fx.push(Effect::Publish(ViewUpdate::ItemUpserted(streaming.item)));
            }
            EngineEventKind::Activity { run, text } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                if let Some(record) = self.runs.get(&run).filter(|r| !r.state.is_terminal()) {
                    fx.push(Effect::Publish(ViewUpdate::Activity { task: record.task, run, text }));
                }
            }
            EngineEventKind::DecisionRequested { run, decision, kind } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state.is_terminal() {
                    return;
                }
                let task = record.task;
                if record.state == RunState::Cancelling {
                    // Stopping: decline rather than leave the engine waiting.
                    fx.push(Effect::Engine(
                        provider,
                        EngineRequest::Answer { decision, answer: crate::decision::DecisionAnswer::Decline },
                    ));
                    return;
                }
                let waiting = match kind {
                    DecisionKind::Question { .. } => RunState::WaitingForInput,
                    _ => RunState::WaitingForApproval,
                };
                record.state = waiting;
                let view = DecisionView { id: decision, task, run, generation, kind, state: DecisionState::Pending };
                self.decisions.insert(decision, view.clone());
                fx.push(Effect::Persist(PersistRequest::Decision { decision: view }));
                fx.push(Effect::Persist(PersistRequest::RunState { run, state: waiting }));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: waiting }));
                self.publish_decisions(task, fx);
                self.publish_chats(fx);
            }
            EngineEventKind::DecisionResolved { decision } => {
                let Some(view) = self.decisions.get_mut(&decision) else {
                    return;
                };
                if view.generation != generation
                    || !matches!(view.state, DecisionState::Pending | DecisionState::Sending)
                {
                    return;
                }
                view.state = DecisionState::Resolved;
                let (task, run) = (view.task, view.run);
                fx.push(Effect::Persist(PersistRequest::DecisionState { decision, state: DecisionState::Resolved }));
                let open = self
                    .decisions
                    .values()
                    .any(|d| d.run == run && matches!(d.state, DecisionState::Pending | DecisionState::Sending));
                if let Some(record) = self.runs.get_mut(&run)
                    && !open
                    && matches!(record.state, RunState::WaitingForApproval | RunState::WaitingForInput)
                {
                    record.state = RunState::Running;
                    fx.push(Effect::Persist(PersistRequest::RunState { run, state: RunState::Running }));
                    fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Running }));
                }
                self.publish_decisions(task, fx);
                self.publish_chats(fx);
            }
            EngineEventKind::RunLost { run, reason } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get_mut(&run).filter(|r| !r.state.is_terminal()) else {
                    return;
                };
                let (task, message) = (record.task, record.message);
                record.needs_reconcile = self.tasks.get(&task).is_some_and(|t| t.info.session.is_some());
                let body = self.deliveries.get(&message).map(|d| d.body.clone()).unwrap_or_default();
                if let Some(delivery) = self.deliveries.get_mut(&message)
                    && delivery.state == DeliveryState::Sent
                {
                    delivery.state = DeliveryState::Unknown;
                    fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Unknown }));
                    fx.push(Effect::Publish(ViewUpdate::DeliveryChanged {
                        task,
                        message,
                        state: DeliveryState::Unknown,
                    }));
                }
                self.settle(run, RunState::OutcomeUnknown, fx);
                fx.push(Effect::Publish(ViewUpdate::Unknown {
                    task,
                    run,
                    body,
                    explanation: format!("{reason}. Checking what it did…"),
                }));
                // The connection is still up, so look now.
                self.reconcile_all(provider, fx);
            }
            EngineEventKind::RunEnded { run, outcome, reason } => {
                if !self.is_on_run_connection(run, provider, generation) {
                    return;
                }
                let Some(record) = self.runs.get(&run) else {
                    return;
                };
                let task = record.task;
                let state = match outcome {
                    RunOutcome::Completed => RunState::Completed,
                    RunOutcome::Failed => RunState::Failed,
                    RunOutcome::Interrupted => RunState::Interrupted,
                };
                if let (RunState::Failed, Some(reason)) = (state, reason) {
                    fx.push(Effect::Publish(ViewUpdate::Notice {
                        task,
                        notice: Notice {
                            text: format!("Codex stopped with an error: {reason}"),
                            tone: NoticeTone::Problem,
                        },
                    }));
                }
                self.reconnects.remove(&provider);
                self.settle(run, state, fx);
            }
            EngineEventKind::Reconciled { run, result } => {
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state != RunState::OutcomeUnknown || record.dismissed {
                    return;
                }
                record.needs_reconcile = false;
                let (task, message) = (record.task, record.message);
                let body = self.deliveries.get(&message).map(|d| d.body.clone()).unwrap_or_default();
                match result {
                    Reconciliation::Found { turn, outcome, items } => {
                        if let Some(delivery) = self.deliveries.get_mut(&message)
                            && delivery.state != DeliveryState::Acknowledged
                        {
                            delivery.state = DeliveryState::Acknowledged;
                            fx.push(Effect::Persist(PersistRequest::DeliveryState {
                                message,
                                state: DeliveryState::Acknowledged,
                            }));
                            fx.push(Effect::Publish(ViewUpdate::DeliveryChanged {
                                task,
                                message,
                                state: DeliveryState::Acknowledged,
                            }));
                        }
                        record.turn = Some(turn.clone());
                        fx.push(Effect::Persist(PersistRequest::RunTurn { run, turn: turn.clone() }));
                        self.import(task, Some(run), items, None, fx);
                        self.set_latest_turn(task, turn, fx);
                        match outcome {
                            Some(outcome) => {
                                let state = match outcome {
                                    RunOutcome::Completed => RunState::Completed,
                                    RunOutcome::Failed => RunState::Failed,
                                    RunOutcome::Interrupted => RunState::Interrupted,
                                };
                                self.resolve_unknown(run, state, fx);
                            }
                            None => fx.push(Effect::Publish(ViewUpdate::Unknown {
                                task,
                                run,
                                body,
                                explanation: "Codex received this message but its turn never finished. Its work may be incomplete.".into(),
                            })),
                        }
                    }
                    Reconciliation::NotFound => fx.push(Effect::Publish(ViewUpdate::Unknown {
                        task,
                        run,
                        body,
                        explanation: "Codex has no record of this message, so it was most likely not delivered. You can send it again.".into(),
                    })),
                    Reconciliation::Unavailable { reason } => fx.push(Effect::Publish(ViewUpdate::Unknown {
                        task,
                        run,
                        body,
                        explanation: format!("Bukno could not check what happened: {reason}"),
                    })),
                }
                self.dispatch_queue(fx);
                self.publish_chats(fx);
            }
            EngineEventKind::OutsideChecked { task, latest_turn, items } => {
                let Some(entry) = self.tasks.get_mut(&task) else {
                    return;
                };
                if self.connections.get(&entry.info.provider) != Some(&generation) {
                    return;
                }
                entry.outside_checked = Some(generation);
                entry.outside_pending = None;
                // A message waiting on this check goes after the missed turns.
                let waiting: Vec<MessageId> = self
                    .deliveries
                    .iter()
                    .filter(|(_, d)| d.task == task && d.recorded && d.state == DeliveryState::AboutToSend)
                    .map(|(id, _)| *id)
                    .collect();
                let before = waiting.first().and_then(|m| self.deliveries.get(m)).and_then(|d| d.item_id);
                if !items.is_empty() {
                    let count = items.len();
                    self.import(task, None, items, before, fx);
                    fx.push(Effect::Publish(ViewUpdate::Notice {
                        task,
                        notice: Notice {
                            text: format!(
                                "This chat continued outside Bukno. {count} message{} from there {} now shown.",
                                if count == 1 { "" } else { "s" },
                                if count == 1 { "is" } else { "are" }
                            ),
                            tone: NoticeTone::Info,
                        },
                    }));
                }
                if let Some(turn) = latest_turn {
                    self.set_latest_turn(task, turn, fx);
                }
                for message in waiting {
                    self.start_turn(message, generation, fx);
                }
            }
            EngineEventKind::Notice { run, text } => {
                if let Some(record) = self.runs.get(&run) {
                    fx.push(Effect::Publish(ViewUpdate::Notice {
                        task: record.task,
                        notice: Notice { text, tone: NoticeTone::Problem },
                    }));
                }
            }
        }
    }

    fn on_timer(&mut self, key: TimerKey, fx: &mut Vec<Effect>) {
        match key {
            TimerKey::StopGrace(run) => {
                let Some(record) = self.runs.get(&run).filter(|r| r.state == RunState::Cancelling) else {
                    return;
                };
                let (task, provider) = (record.task, record.provider);
                let shared = self
                    .runs
                    .iter()
                    .filter(|(id, r)| **id != run && r.provider == provider && is_active(r, &self.deliveries))
                    .filter_map(|(_, r)| self.tasks.get(&r.task).map(|t| t.info.title.clone()))
                    .collect();
                fx.push(Effect::Publish(ViewUpdate::StopSlow { task, run, shared }));
            }
            TimerKey::QuitGrace => {
                if !self.quitting {
                    return;
                }
                let providers: HashSet<Provider> =
                    self.runs.values().filter(|r| is_active(r, &self.deliveries)).map(|r| r.provider).collect();
                for provider in providers {
                    fx.push(Effect::Engine(provider, EngineRequest::Kill));
                    if let Some(generation) = self.connections.remove(&provider) {
                        self.lose_connection(provider, generation, "Bukno quit before the engine stopped.", fx);
                    }
                }
                self.quit_ready(fx);
            }
        }
    }

    // ----- Rules ----------------------------------------------------------

    /// Why `run` cannot be dispatched now, if anything stops it.
    fn wait_reason(&self, run: RunId) -> Option<WaitReason> {
        let record = self.runs.get(&run)?;
        if let Some(why) = &record.paused {
            return Some(WaitReason::Paused { why: why.clone() });
        }
        if self.active_count(Some(run)) >= self.limit {
            return Some(WaitReason::Slots { limit: self.limit });
        }
        if record.settings.writes != Writes::Never && !record.run_anyway {
            let mine = self.tasks.get(&record.task).and_then(|t| self.workspaces.get(&t.info.workspace))?;
            for (id, other) in &self.runs {
                if *id == run || !other.holds_lease() {
                    continue;
                }
                let Some(theirs) = self.tasks.get(&other.task).and_then(|t| self.workspaces.get(&t.info.workspace))
                else {
                    continue;
                };
                if mine.overlaps(theirs) {
                    let title = self.tasks.get(&other.task).map(|t| t.info.title.clone()).unwrap_or_default();
                    return Some(WaitReason::Workspace { holder: other.task, holder_title: title });
                }
            }
        }
        None
    }

    /// Runs occupying an execution slot, optionally ignoring one.
    fn active_count(&self, except: Option<RunId>) -> usize {
        self.runs.iter().filter(|(id, r)| Some(**id) != except && is_active(r, &self.deliveries)).count()
    }

    fn take_lease(&mut self, run: RunId) {
        let Some(record) = self.runs.get(&run) else {
            return;
        };
        if record.settings.writes == Writes::Never {
            return;
        }
        let shared = record.run_anyway;
        let workspace = self.tasks.get(&record.task).and_then(|t| self.workspaces.get(&t.info.workspace)).cloned();
        if shared && let Some(mine) = &workspace {
            // Label every overlapping holder as shared too.
            let holders: Vec<RunId> = self
                .runs
                .iter()
                .filter(|(id, r)| **id != run && r.holds_lease())
                .filter(|(_, r)| {
                    self.tasks
                        .get(&r.task)
                        .and_then(|t| self.workspaces.get(&t.info.workspace))
                        .is_some_and(|w| w.overlaps(mine))
                })
                .map(|(id, _)| *id)
                .collect();
            for holder in holders {
                if let Some(other) = self.runs.get_mut(&holder) {
                    other.lease = Lease::Shared;
                }
            }
        }
        if let Some(record) = self.runs.get_mut(&run) {
            record.lease = if shared { Lease::Shared } else { Lease::Held };
        }
    }

    /// Send every queued run that can go now, oldest first.
    fn dispatch_queue(&mut self, fx: &mut Vec<Effect>) {
        let queued = self.queue.clone();
        for run in queued {
            let Some(record) = self.runs.get(&run) else {
                self.queue.retain(|r| *r != run);
                continue;
            };
            if record.state != RunState::Preparing {
                self.queue.retain(|r| *r != run);
                continue;
            }
            let wait = self.wait_reason(run);
            if wait.is_some() {
                self.publish_wait(run, wait, fx);
                continue;
            }
            if !self.storage_ok {
                continue;
            }
            self.queue.retain(|r| *r != run);
            self.take_lease(run);
            let message = self.runs[&run].message;
            if let Some(delivery) = self.deliveries.get_mut(&message) {
                delivery.state = DeliveryState::AboutToSend;
                delivery.recorded = false;
            }
            fx.push(Effect::Persist(PersistRequest::MarkAboutToSend { message }));
            self.publish_wait(run, None, fx);
        }
    }

    /// Write a recorded delivery's turn to the engine on `generation`.
    fn start_turn(&mut self, message: MessageId, generation: u64, fx: &mut Vec<Effect>) {
        let Some(delivery) = self.deliveries.get_mut(&message) else {
            return;
        };
        if delivery.state != DeliveryState::AboutToSend || !delivery.recorded {
            return;
        }
        let Some(run) = self.runs.get_mut(&delivery.run) else {
            return;
        };
        if run.state != RunState::Preparing {
            return;
        }
        let Some(task) = self.tasks.get(&run.task) else {
            return;
        };
        let Some(workspace) = self.workspaces.get(&task.info.workspace) else {
            return;
        };
        // Missed turns from outside Bukno load before a send (section 6).
        if task.info.session.is_some() && task.outside_checked != Some(generation) {
            let (task_id, run_id) = (delivery.task, delivery.run);
            self.check_outside(task_id, true, fx);
            self.publish_wait(run_id, Some(WaitReason::Checking), fx);
            return;
        }
        // Only after the commit, and only on a known connection, may the frame be written.
        delivery.state = DeliveryState::Sent;
        run.state = RunState::Starting;
        run.generation = Some(generation);
        let (task_id, run_id) = (delivery.task, delivery.run);
        fx.push(Effect::Engine(
            run.provider,
            EngineRequest::StartTurn {
                task: task_id,
                run: run_id,
                message,
                body: delivery.body.clone(),
                session: task.info.session.clone(),
                cwd: workspace.path.clone(),
                settings: run.settings.clone(),
            },
        ));
        fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Sent }));
        fx.push(Effect::Persist(PersistRequest::RunState { run: run_id, state: RunState::Starting }));
        fx.push(Effect::Publish(ViewUpdate::DeliveryChanged { task: task_id, message, state: DeliveryState::Sent }));
        fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task: task_id, run: run_id, state: RunState::Starting }));
        self.publish_wait(run_id, None, fx);
        self.publish_chats(fx);
    }

    /// Every unsettled run written to `generation` becomes OutcomeUnknown and
    /// keeps its lease until reconciled.
    fn lose_connection(&mut self, provider: Provider, generation: u64, reason: &str, fx: &mut Vec<Effect>) {
        let affected: Vec<RunId> = self
            .runs
            .iter()
            .filter(|(_, r)| r.provider == provider && !r.state.is_terminal() && r.generation == Some(generation))
            .map(|(id, _)| *id)
            .collect();
        for run in affected {
            let Some(record) = self.runs.get_mut(&run) else {
                continue;
            };
            let (task, message) = (record.task, record.message);
            record.needs_reconcile = self.tasks.get(&task).is_some_and(|t| t.info.session.is_some());
            let body = self.deliveries.get(&message).map(|d| d.body.clone()).unwrap_or_default();
            if let Some(delivery) = self.deliveries.get_mut(&message)
                && delivery.state == DeliveryState::Sent
            {
                // Written but never acknowledged: unknown, and never resent automatically.
                delivery.state = DeliveryState::Unknown;
                fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Unknown }));
                fx.push(Effect::Publish(ViewUpdate::DeliveryChanged { task, message, state: DeliveryState::Unknown }));
            }
            self.settle(run, RunState::OutcomeUnknown, fx);
            fx.push(Effect::Publish(ViewUpdate::Unknown {
                task,
                run,
                body,
                explanation: format!("The connection to Codex was lost ({reason}). Checking what it finished…"),
            }));
        }
    }

    fn reconcile_all(&mut self, provider: Provider, fx: &mut Vec<Effect>) {
        let pending: Vec<RunId> = self
            .runs
            .iter()
            .filter(|(_, r)| r.provider == provider && r.needs_reconcile && !r.dismissed)
            .map(|(id, _)| *id)
            .collect();
        for run in pending {
            let record = &self.runs[&run];
            let Some(task) = self.tasks.get(&record.task) else {
                continue;
            };
            let (Some(session), Some(workspace)) =
                (task.info.session.clone(), self.workspaces.get(&task.info.workspace))
            else {
                continue;
            };
            fx.push(Effect::Engine(
                provider,
                EngineRequest::Reconcile { run, message: record.message, session, cwd: workspace.path.clone() },
            ));
        }
    }

    /// Ask the engine whether the chat continued outside Bukno, once per
    /// connection. When opening a chat this is skipped while it is busy;
    /// before a send (`for_send`) it always runs.
    fn check_outside(&mut self, task: TaskId, for_send: bool, fx: &mut Vec<Effect>) {
        let Some(entry) = self.tasks.get(&task) else {
            return;
        };
        let Some(generation) = self.connections.get(&entry.info.provider).copied() else {
            return;
        };
        if entry.outside_checked == Some(generation) || entry.outside_pending == Some(generation) {
            return;
        }
        let busy = entry.latest.and_then(|r| self.runs.get(&r)).is_some_and(|r| !r.state.is_terminal());
        if busy && !for_send {
            return;
        }
        let (Some(session), Some(workspace)) = (entry.info.session.clone(), self.workspaces.get(&entry.info.workspace))
        else {
            return;
        };
        let (provider, cwd) = (entry.info.provider, workspace.path.clone());
        if let Some(entry) = self.tasks.get_mut(&task) {
            entry.outside_pending = Some(generation);
        }
        fx.push(Effect::Engine(provider, EngineRequest::CheckOutside { task, session, cwd }));
    }

    fn import(
        &mut self,
        task: TaskId,
        run: Option<RunId>,
        items: Vec<ImportedItem>,
        before: Option<ItemId>,
        fx: &mut Vec<Effect>,
    ) {
        if items.is_empty() {
            return;
        }
        fx.push(Effect::Persist(PersistRequest::ImportItems { task, run, items, before }));
        if self.selected == Some(task) {
            fx.push(Effect::Load(LoadRequest::History { task }));
        }
    }

    fn set_latest_turn(&mut self, task: TaskId, turn: String, fx: &mut Vec<Effect>) {
        if let Some(entry) = self.tasks.get_mut(&task)
            && let Some(session) = entry.info.session.as_mut()
        {
            session.latest_turn = Some(turn);
            fx.push(Effect::Persist(PersistRequest::Session { task, session: session.clone() }));
        }
    }

    fn connect(&mut self, provider: Provider, fx: &mut Vec<Effect>) {
        if self.connections.contains_key(&provider) || !self.connecting.insert(provider) {
            return;
        }
        fx.push(Effect::Engine(provider, EngineRequest::Connect));
    }

    /// True when a run-scoped event comes from the connection its run was written to.
    fn is_on_run_connection(&self, run: RunId, provider: Provider, generation: u64) -> bool {
        self.connections.get(&provider) == Some(&generation)
            && self.runs.get(&run).is_some_and(|r| r.generation == Some(generation))
    }

    fn expire_decisions(&mut self, run: RunId, fx: &mut Vec<Effect>) {
        let mut task = None;
        for view in self.decisions.values_mut().filter(|d| d.run == run) {
            if matches!(view.state, DecisionState::Pending | DecisionState::Sending) {
                view.state = DecisionState::Expired;
                task = Some(view.task);
                fx.push(Effect::Persist(PersistRequest::DecisionState {
                    decision: view.id,
                    state: DecisionState::Expired,
                }));
            }
        }
        self.decisions
            .retain(|_, d| d.run != run || !matches!(d.state, DecisionState::Expired | DecisionState::Resolved));
        if let Some(task) = task {
            self.publish_decisions(task, fx);
        }
    }

    /// Move a run to a terminal state and release what the coordinator held for it.
    fn settle(&mut self, run: RunId, state: RunState, fx: &mut Vec<Effect>) {
        let Some(record) = self.runs.get_mut(&run) else {
            return;
        };
        if record.state.is_terminal() {
            return;
        }
        record.state = state;
        let task = record.task;
        for item in std::mem::take(&mut record.reply_items) {
            if let Some(entry) = self.items.remove(&item) {
                // Keep partial output; it is the user's content.
                fx.push(Effect::Persist(PersistRequest::Item {
                    item: entry.item,
                    provider_item: Some(entry.provider_item),
                }));
            }
        }
        self.queue.retain(|r| *r != run);
        self.expire_decisions(run, fx);
        fx.push(Effect::Persist(PersistRequest::RunState { run, state }));
        fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state }));
        self.after_settle(fx);
    }

    /// A previously unknown run turned out to have ended with `state`.
    fn resolve_unknown(&mut self, run: RunId, state: RunState, fx: &mut Vec<Effect>) {
        let Some(record) = self.runs.get_mut(&run) else {
            return;
        };
        record.state = state;
        record.needs_reconcile = false;
        let task = record.task;
        fx.push(Effect::Persist(PersistRequest::RunState { run, state }));
        fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state }));
        fx.push(Effect::Publish(ViewUpdate::UnknownCleared { task, run }));
        self.after_settle(fx);
    }

    fn after_settle(&mut self, fx: &mut Vec<Effect>) {
        self.dispatch_queue(fx);
        self.publish_chats(fx);
        if self.quitting && self.active_count(None) == 0 {
            self.quit_ready(fx);
        }
    }

    fn quit_ready(&mut self, fx: &mut Vec<Effect>) {
        if !self.quit_ready {
            self.quit_ready = true;
            fx.push(Effect::QuitReady);
            fx.push(Effect::Publish(ViewUpdate::QuitReady));
        }
    }

    fn add_workspace(&mut self, workspace: WorkspaceInfo, fx: &mut Vec<Effect>) {
        if self.workspaces.get(&workspace.id) != Some(&workspace) {
            fx.push(Effect::Persist(PersistRequest::Workspace(workspace.clone())));
            self.workspaces.insert(workspace.id, workspace);
        }
    }

    // ----- Views ----------------------------------------------------------

    fn publish_wait(&mut self, run: RunId, reason: Option<WaitReason>, fx: &mut Vec<Effect>) {
        let Some(record) = self.runs.get_mut(&run) else {
            return;
        };
        if record.wait == reason {
            return;
        }
        record.wait = reason.clone();
        fx.push(Effect::Publish(ViewUpdate::RunWaiting { task: record.task, run, reason }));
    }

    fn publish_decisions(&self, task: TaskId, fx: &mut Vec<Effect>) {
        let mut decisions: Vec<DecisionView> = self
            .decisions
            .values()
            .filter(|d| d.task == task && matches!(d.state, DecisionState::Pending | DecisionState::Sending))
            .cloned()
            .collect();
        decisions.sort_by_key(|d| d.id);
        fx.push(Effect::Publish(ViewUpdate::Decisions { task, decisions }));
    }

    /// Bring a freshly opened chat's view up to date.
    fn republish_task(&self, task: TaskId, fx: &mut Vec<Effect>) {
        let Some(entry) = self.tasks.get(&task) else {
            return;
        };
        if let Some((run, record)) = entry.latest.and_then(|r| self.runs.get(&r).map(|rec| (r, rec))) {
            fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: record.state }));
            if record.state == RunState::Preparing {
                fx.push(Effect::Publish(ViewUpdate::RunWaiting { task, run, reason: record.wait.clone() }));
            }
        }
        self.publish_decisions(task, fx);
    }

    fn publish_chats(&self, fx: &mut Vec<Effect>) {
        let projects = self
            .projects
            .iter()
            .filter(|(_, saved)| *saved)
            .map(|(p, _)| {
                let workspace = self.workspaces.get(&p.workspace);
                ProjectView {
                    id: p.id,
                    name: p.name.clone(),
                    path: workspace.map(|w| w.path.clone()).unwrap_or_default(),
                    available: workspace.is_some_and(|w| w.available),
                }
            })
            .collect();
        let mut chats: Vec<(i64, ChatSummary)> = self
            .tasks
            .values()
            .filter(|t| t.saved)
            .map(|t| {
                let workspace = self.workspaces.get(&t.info.workspace);
                let latest = t.latest.and_then(|r| self.runs.get(&r));
                let needs_you =
                    self.decisions.values().any(|d| d.task == t.info.id && d.state == DecisionState::Pending);
                let activity = match latest {
                    _ if needs_you => ChatActivity::NeedsYou,
                    Some(r) if matches!(r.state, RunState::WaitingForApproval | RunState::WaitingForInput) => {
                        ChatActivity::NeedsYou
                    }
                    Some(r) if r.state == RunState::Preparing && !is_active(r, &self.deliveries) => {
                        ChatActivity::Queued
                    }
                    Some(r) if !r.state.is_terminal() => ChatActivity::Working,
                    Some(r) if r.state == RunState::OutcomeUnknown && !r.dismissed => ChatActivity::Unknown,
                    _ => ChatActivity::Idle,
                };
                let title = if t.info.title.is_empty() { "New chat".to_owned() } else { t.info.title.clone() };
                (
                    t.info.created,
                    ChatSummary {
                        task: t.info.id,
                        title,
                        provider: t.info.provider,
                        project: t.info.project,
                        workspace: t.info.workspace,
                        path: workspace.map(|w| w.path.clone()).unwrap_or_default(),
                        available: workspace.is_some_and(|w| w.available),
                        activity,
                        shared_workspace: latest.is_some_and(|r| r.lease == Lease::Shared && r.holds_lease()),
                    },
                )
            })
            .collect();
        chats.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.task.cmp(&b.1.task)));
        fx.push(Effect::Publish(ViewUpdate::Chats { projects, chats: chats.into_iter().map(|(_, c)| c).collect() }));
    }
}

/// Occupies an execution slot: written or about to be written, not yet settled.
fn is_active(run: &Run, deliveries: &HashMap<MessageId, Delivery>) -> bool {
    match run.state {
        RunState::Preparing => deliveries.get(&run.message).is_some_and(|d| d.state == DeliveryState::AboutToSend),
        state => !state.is_terminal(),
    }
}

/// A chat title from its first message: the first line, shortened.
fn title_from(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("New chat");
    if line.chars().count() <= TITLE_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(TITLE_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}
