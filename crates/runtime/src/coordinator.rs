//! The coordinator task: one inbox, one state machine, effects executed in order.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bukno_core::event::{
    Command, Effect, EngineRequest, Input, PersistRequest, RejectReason, StorageResult, ViewUpdate,
};
use bukno_core::ids::{ItemId, MessageId, RunId, TaskId};
use bukno_core::machine::Machine;
use bukno_core::message::{Provider, TranscriptItem};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::synthetic::{self, Scenario, SyntheticEngine};

/// Longest a streamed text change waits before it is shown (specification section 21).
pub const TEXT_BATCH: Duration = Duration::from_millis(50);
/// Bound on queued engine and storage results.
const INBOX_CAPACITY: usize = 1_024;

/// What the interface asks for. The runtime turns these into core commands
/// with freshly captured identifiers.
#[derive(Clone, Debug)]
pub enum UiCommand {
    Submit { task: TaskId, provider: Provider, draft_revision: u64, body: String },
    Interrupt { task: TaskId, run: RunId },
}

/// What the interface receives.
#[derive(Clone, Debug)]
pub enum UiEvent {
    View(ViewUpdate),
    Rejected { task: TaskId, reason: RejectReason },
}

pub struct Coordinator {
    commands: mpsc::UnboundedSender<UiCommand>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Coordinator {
    /// Start the coordinator for a synthetic scenario. `wake` is called after
    /// events are queued, so the interface can schedule a repaint.
    pub fn start_synthetic(
        scenario: Scenario,
        events: std::sync::mpsc::Sender<UiEvent>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("bukno-coordinator")
            .enable_time()
            .build()
            .expect("start the Tokio runtime");
        let (commands, command_rx) = mpsc::unbounded_channel();
        runtime.spawn(run(Arc::new(scenario), command_rx, events, wake));
        Self { commands, runtime: Some(runtime) }
    }

    pub fn send(&self, command: UiCommand) {
        // The loop only ends when the coordinator is dropped.
        let _ = self.commands.send(command);
    }
}

impl Drop for Coordinator {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            // Do not block the UI thread on in-flight synthetic streams.
            thread::spawn(move || runtime.shutdown_timeout(Duration::from_millis(200)));
        }
    }
}

fn new_id() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

async fn run(
    scenario: Arc<Scenario>,
    mut commands: mpsc::UnboundedReceiver<UiCommand>,
    events: std::sync::mpsc::Sender<UiEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let (inbox_tx, mut inbox) = mpsc::channel::<Input>(INBOX_CAPACITY);
    let engine = SyntheticEngine::new(inbox_tx, scenario.clone());
    let mut machine = Machine::new();
    let mut publisher = Publisher { events, wake, pending: HashMap::new(), flush_at: None };

    // Results the coordinator produces for itself (the synthetic store's
    // answers). They are handled before the inbox is read again, so the loop
    // never waits on its own bounded channel.
    let mut local = VecDeque::new();
    local.push_back(Input::Stored(StorageResult::HistoryLoaded { task: synthetic::TASK, items: scenario.history() }));
    if let Some(body) = scenario.auto_submit.clone() {
        local.push_back(to_input(UiCommand::Submit {
            task: synthetic::TASK,
            provider: Provider::Codex,
            draft_revision: 0,
            body,
        }));
    }

    loop {
        let flush_at = publisher.flush_at;
        let input = if let Some(input) = local.pop_front() {
            input
        } else {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => to_input(command),
                    None => break,
                },
                Some(input) = inbox.recv() => input,
                _ = sleep_until(flush_at) => {
                    publisher.flush();
                    continue;
                }
            }
        };

        for effect in machine.step(input) {
            match effect {
                Effect::Persist(request) => {
                    // Synthetic mode persists nothing. The outbox commit is
                    // simulated so the send path runs through the real machine.
                    if let PersistRequest::RecordDelivery { message, .. } = request {
                        local.push_back(Input::Stored(StorageResult::DeliveryRecorded { message }));
                    }
                }
                Effect::Engine(EngineRequest::StartTurn { run, .. }) => {
                    engine.start_turn(run, ItemId(new_id()));
                }
                Effect::Engine(EngineRequest::Interrupt { run }) => engine.interrupt(run),
                Effect::Publish(ViewUpdate::ItemUpserted(item)) if !item.completed => {
                    publisher.queue_text(item);
                }
                Effect::Publish(update) => publisher.send(UiEvent::View(update)),
                Effect::Rejected { task, reason } => {
                    publisher.send(UiEvent::Rejected { task, reason });
                }
            }
        }
    }
}

fn to_input(command: UiCommand) -> Input {
    Input::Command(match command {
        UiCommand::Submit { task, provider, draft_revision, body } => Command::SubmitMessage {
            task,
            provider,
            message: MessageId(new_id()),
            run: RunId(new_id()),
            item: ItemId(new_id()),
            draft_revision,
            body,
        },
        UiCommand::Interrupt { task, run } => Command::InterruptRun { task, run },
    })
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Sends view updates to the interface. Streaming text is coalesced per item
/// for up to [`TEXT_BATCH`]; every other update flushes pending text first so
/// ordering is kept and control events are never dropped.
struct Publisher {
    events: std::sync::mpsc::Sender<UiEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
    pending: HashMap<ItemId, TranscriptItem>,
    flush_at: Option<Instant>,
}

impl Publisher {
    fn queue_text(&mut self, item: TranscriptItem) {
        self.pending.insert(item.id, item);
        self.flush_at.get_or_insert_with(|| Instant::now() + TEXT_BATCH);
    }

    fn flush(&mut self) {
        self.flush_at = None;
        if self.pending.is_empty() {
            return;
        }
        for (_, item) in self.pending.drain() {
            let _ = self.events.send(UiEvent::View(ViewUpdate::ItemUpserted(item)));
        }
        (self.wake)();
    }

    fn send(&mut self, event: UiEvent) {
        if let UiEvent::View(ViewUpdate::ItemUpserted(item)) = &event {
            self.pending.remove(&item.id);
        }
        self.flush();
        let _ = self.events.send(event);
        (self.wake)();
    }
}
