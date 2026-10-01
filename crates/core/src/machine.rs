//! The coordinator's decision function: current state plus one input gives
//! the new state and a list of effects.
//!
//! Pass 0 skeleton. It implements the send path through the outbox, streaming
//! into a reply, Stop, and connection loss. Approvals, steering, queues,
//! leases and restart reconciliation land with Pass 1.

use std::collections::HashMap;

use crate::event::{
    Command, Effect, EngineEvent, EngineEventKind, EngineRequest, Input, PersistRequest, RejectReason, StorageResult,
    ViewUpdate,
};
use crate::ids::{ItemId, MessageId, RunId, TaskId};
use crate::message::{DeliveryState, ItemKind, Provider, TranscriptItem};
use crate::run::{RunOutcome, RunState};

#[derive(Debug)]
struct Run {
    task: TaskId,
    provider: Provider,
    message: MessageId,
    state: RunState,
    /// Connection the turn was written to, once it was written.
    generation: Option<u64>,
    /// Reply items being streamed. Dropped when the run settles; history
    /// stays in storage, not in the coordinator.
    reply_items: Vec<ItemId>,
}

#[derive(Debug)]
struct Delivery {
    task: TaskId,
    run: RunId,
    state: DeliveryState,
    body: String,
}

#[derive(Debug, Default)]
pub struct Machine {
    generation: u64,
    runs: HashMap<RunId, Run>,
    active_run: HashMap<TaskId, RunId>,
    deliveries: HashMap<MessageId, Delivery>,
    items: HashMap<ItemId, TranscriptItem>,
}

impl Machine {
    pub fn new() -> Self {
        Self::default()
    }

    /// The state of a run, if the coordinator knows it.
    pub fn run_state(&self, run: RunId) -> Option<RunState> {
        self.runs.get(&run).map(|r| r.state)
    }

    pub fn delivery_state(&self, message: MessageId) -> Option<DeliveryState> {
        self.deliveries.get(&message).map(|d| d.state)
    }

    /// Handle one input to completion.
    pub fn step(&mut self, input: Input) -> Vec<Effect> {
        let mut effects = Vec::new();
        match input {
            Input::Command(command) => self.on_command(command, &mut effects),
            Input::Engine(event) => self.on_engine(event, &mut effects),
            Input::Stored(result) => self.on_stored(result, &mut effects),
        }
        effects
    }

    fn on_command(&mut self, command: Command, fx: &mut Vec<Effect>) {
        match command {
            Command::SubmitMessage { task, provider, message, run, item, draft_revision, body } => {
                if body.trim().is_empty() {
                    fx.push(Effect::Rejected { task, reason: RejectReason::EmptyMessage });
                    return;
                }
                // Deduplicate repeated clicks and retries by message ID before transport.
                if self.deliveries.contains_key(&message) {
                    fx.push(Effect::Rejected { task, reason: RejectReason::DuplicateMessage });
                    return;
                }
                if let Some(active) = self.active_run.get(&task)
                    && self.runs.get(active).is_some_and(|r| !r.state.is_terminal())
                {
                    fx.push(Effect::Rejected { task, reason: RejectReason::RunActive });
                    return;
                }

                self.runs.insert(
                    run,
                    Run {
                        task,
                        provider,
                        message,
                        state: RunState::Preparing,
                        generation: None,
                        reply_items: Vec::new(),
                    },
                );
                self.active_run.insert(task, run);
                self.deliveries
                    .insert(message, Delivery { task, run, state: DeliveryState::AboutToSend, body: body.clone() });
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

                // Outbox: the delivery is recorded before anything goes to the engine.
                fx.push(Effect::Persist(PersistRequest::RecordDelivery { task, run, message, draft_revision, body }));
                fx.push(Effect::Persist(PersistRequest::Item(user_item.clone())));
                fx.push(Effect::Publish(ViewUpdate::ItemUpserted(user_item)));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Preparing }));
            }
            Command::InterruptRun { task, run } => {
                let Some(record) = self.runs.get_mut(&run).filter(|r| r.task == task) else {
                    fx.push(Effect::Rejected { task, reason: RejectReason::UnknownRun });
                    return;
                };
                if !record.state.can_interrupt() {
                    return;
                }
                // Enter Cancelling immediately; the engine's acknowledgement alone
                // does not prove the work stopped, so the run settles on RunEnded.
                record.state = RunState::Cancelling;
                fx.push(Effect::Engine(EngineRequest::Interrupt { run }));
                fx.push(Effect::Persist(PersistRequest::RunState { run, state: RunState::Cancelling }));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Cancelling }));
            }
        }
    }

    fn on_stored(&mut self, result: StorageResult, fx: &mut Vec<Effect>) {
        match result {
            StorageResult::DeliveryRecorded { message } => {
                let Some(delivery) = self.deliveries.get_mut(&message) else {
                    return;
                };
                if delivery.state != DeliveryState::AboutToSend {
                    return;
                }
                let Some(run) = self.runs.get_mut(&delivery.run) else {
                    return;
                };
                if run.state != RunState::Preparing {
                    return;
                }
                // Only after the commit may the frame be written to the engine.
                delivery.state = DeliveryState::Sent;
                run.state = RunState::Starting;
                run.generation = Some(self.generation);
                fx.push(Effect::Engine(EngineRequest::StartTurn {
                    task: delivery.task,
                    run: delivery.run,
                    message,
                    body: delivery.body.clone(),
                }));
                fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Sent }));
                fx.push(Effect::Publish(ViewUpdate::DeliveryChanged {
                    task: delivery.task,
                    message,
                    state: DeliveryState::Sent,
                }));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged {
                    task: delivery.task,
                    run: delivery.run,
                    state: RunState::Starting,
                }));
            }
            StorageResult::DeliveryFailed { message, reason } => {
                let Some(delivery) = self.deliveries.remove(&message) else {
                    return;
                };
                if let Some(run) = self.runs.remove(&delivery.run) {
                    self.active_run.remove(&run.task);
                }
                fx.push(Effect::Rejected { task: delivery.task, reason: RejectReason::NotSaved(reason) });
            }
            StorageResult::HistoryLoaded { task, items } => {
                fx.push(Effect::Publish(ViewUpdate::ConversationLoaded { task, items }));
            }
        }
    }

    fn on_engine(&mut self, event: EngineEvent, fx: &mut Vec<Effect>) {
        if event.connection_generation < self.generation {
            return; // Late event from a connection that has since been replaced.
        }
        self.generation = event.connection_generation;

        match event.kind {
            EngineEventKind::RunAccepted { run } => {
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state != RunState::Starting {
                    return;
                }
                record.state = RunState::Running;
                let (task, message) = (record.task, record.message);
                if let Some(delivery) = self.deliveries.get_mut(&message) {
                    delivery.state = DeliveryState::Acknowledged;
                }
                fx.push(Effect::Persist(PersistRequest::DeliveryState { message, state: DeliveryState::Acknowledged }));
                fx.push(Effect::Persist(PersistRequest::RunState { run, state: RunState::Running }));
                fx.push(Effect::Publish(ViewUpdate::DeliveryChanged {
                    task,
                    message,
                    state: DeliveryState::Acknowledged,
                }));
                fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state: RunState::Running }));
            }
            EngineEventKind::TextDelta { run, item, delta } => {
                let Some(record) = self.runs.get_mut(&run) else {
                    return;
                };
                if record.state.is_terminal() {
                    return;
                }
                let entry = self.items.entry(item).or_insert_with(|| {
                    record.reply_items.push(item);
                    TranscriptItem {
                        id: item,
                        task: record.task,
                        run: Some(run),
                        kind: ItemKind::AgentMessage { provider: record.provider },
                        text: String::new(),
                        meta: None,
                        completed: false,
                        revision: 0,
                    }
                });
                if entry.completed {
                    return;
                }
                entry.text.push_str(&delta);
                entry.revision += 1;
                // The runtime coalesces these for display; tokens are not persisted one by one.
                fx.push(Effect::Publish(ViewUpdate::ItemUpserted(entry.clone())));
            }
            EngineEventKind::ItemCompleted { run, item } => {
                let Some(entry) = self.items.get_mut(&item).filter(|i| i.run == Some(run)) else {
                    return;
                };
                if entry.completed {
                    return;
                }
                entry.completed = true;
                entry.revision += 1;
                fx.push(Effect::Persist(PersistRequest::Item(entry.clone())));
                fx.push(Effect::Publish(ViewUpdate::ItemUpserted(entry.clone())));
            }
            EngineEventKind::RunEnded { run, outcome } => {
                let state = match outcome {
                    RunOutcome::Completed => RunState::Completed,
                    RunOutcome::Failed => RunState::Failed,
                    RunOutcome::Interrupted => RunState::Interrupted,
                };
                self.settle(run, state, fx);
            }
            EngineEventKind::ConnectionLost => {
                let generation = event.connection_generation;
                let affected: Vec<RunId> = self
                    .runs
                    .iter()
                    .filter(|(_, r)| !r.state.is_terminal() && r.generation == Some(generation))
                    .map(|(id, _)| *id)
                    .collect();
                for run in affected {
                    if let Some(message) = self.runs.get(&run).map(|r| r.message)
                        && let Some(delivery) = self.deliveries.get_mut(&message)
                        && delivery.state == DeliveryState::Sent
                    {
                        // Written but never acknowledged: unknown, and never resent automatically.
                        delivery.state = DeliveryState::Unknown;
                        let task = delivery.task;
                        fx.push(Effect::Persist(PersistRequest::DeliveryState {
                            message,
                            state: DeliveryState::Unknown,
                        }));
                        fx.push(Effect::Publish(ViewUpdate::DeliveryChanged {
                            task,
                            message,
                            state: DeliveryState::Unknown,
                        }));
                    }
                    self.settle(run, RunState::OutcomeUnknown, fx);
                }
            }
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
            if let Some(entry) = self.items.remove(&item)
                && !entry.completed
            {
                // Keep partial output; it is the user's content.
                fx.push(Effect::Persist(PersistRequest::Item(entry)));
            }
        }
        fx.push(Effect::Persist(PersistRequest::RunState { run, state }));
        fx.push(Effect::Publish(ViewUpdate::RunStateChanged { task, run, state }));
    }
}
