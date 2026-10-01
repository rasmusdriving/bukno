//! Replays of recorded input sequences through the real coordinator state
//! machine (specification section 2). Added for review finding 2 on PR 1.
//!
//! Ways connection identity could fail, enumerated first:
//!
//! C1 A turn is written before any engine connection is known, so the run
//!    has no generation and a later loss cannot find it.
//! C2 Loss of the first connection leaves its run Running, and the chat then
//!    rejects every new message as RunActive.
//! C3 An event from another connection (newer or older) changes a run that
//!    was written to a different one.
//! C4 A loss reported for a connection that is not live settles runs on the
//!    live connection.
//! C5 A message recorded while disconnected is written twice, or never.

use bukno_core::event::{
    Command, Effect, EngineEvent, EngineEventKind, EngineRequest, Input, RejectReason, StorageResult, ViewUpdate,
};
use bukno_core::ids::{ItemId, MessageId, RunId, TaskId};
use bukno_core::machine::Machine;
use bukno_core::message::Provider;
use bukno_core::run::RunState;

const TASK: TaskId = TaskId(1);

fn submit(n: u128) -> Input {
    Input::Command(Command::SubmitMessage {
        task: TASK,
        provider: Provider::Codex,
        message: MessageId(n),
        run: RunId(n),
        item: ItemId(n),
        draft_revision: 1,
        body: format!("message {n}"),
    })
}

fn recorded(n: u128) -> Input {
    Input::Stored(StorageResult::DeliveryRecorded { message: MessageId(n) })
}

fn engine(generation: u64, kind: EngineEventKind) -> Input {
    Input::Engine(EngineEvent { connection_generation: generation, kind })
}

fn writes(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Engine(EngineRequest::StartTurn { .. }))).count()
}

/// Replay `inputs` and return every effect, in order.
fn replay(machine: &mut Machine, inputs: Vec<Input>) -> Vec<Effect> {
    inputs.into_iter().flat_map(|input| machine.step(input)).collect()
}

/// C1, C5: nothing is written until a connection exists, then exactly once.
#[test]
fn turn_waits_for_a_connection_and_is_written_once() {
    let mut m = Machine::new();
    let before = replay(&mut m, vec![submit(1), recorded(1)]);
    assert_eq!(writes(&before), 0, "no connection yet, nothing written");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Preparing));

    let after = replay(&mut m, vec![engine(1, EngineEventKind::Connected), engine(1, EngineEventKind::Connected)]);
    assert_eq!(writes(&after), 1, "written once when the connection arrives, not again on a repeat");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Starting));
}

/// C2: losing the first connection settles its run, and the chat accepts new work.
#[test]
fn first_connection_loss_settles_the_run() {
    let mut m = Machine::new();
    replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit(1),
            recorded(1),
            engine(1, EngineEventKind::RunAccepted { run: RunId(1) }),
        ],
    );
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));

    let lost = replay(&mut m, vec![engine(1, EngineEventKind::ConnectionLost)]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::OutcomeUnknown));
    assert!(lost.contains(&Effect::Publish(ViewUpdate::RunStateChanged {
        task: TASK,
        run: RunId(1),
        state: RunState::OutcomeUnknown
    })));

    // Not rejected as RunActive; it waits for a new connection instead.
    let next = replay(&mut m, vec![submit(2), recorded(2)]);
    assert!(!next.iter().any(|e| matches!(e, Effect::Rejected { reason: RejectReason::RunActive, .. })));
    assert_eq!(writes(&next), 0, "no live connection after the loss");
    let reconnected = replay(&mut m, vec![engine(2, EngineEventKind::Connected)]);
    assert_eq!(writes(&reconnected), 1);
}

/// C3: events from any other connection cannot change the run.
#[test]
fn events_from_another_connection_are_ignored() {
    let mut m = Machine::new();
    replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit(1),
            recorded(1),
            engine(1, EngineEventKind::RunAccepted { run: RunId(1) }),
        ],
    );
    let stray = replay(
        &mut m,
        vec![
            engine(2, EngineEventKind::TextDelta { run: RunId(1), item: ItemId(40), delta: "wrong connection".into() }),
            engine(0, EngineEventKind::TextDelta { run: RunId(1), item: ItemId(40), delta: "older connection".into() }),
            engine(2, EngineEventKind::RunEnded { run: RunId(1), outcome: bukno_core::run::RunOutcome::Completed }),
        ],
    );
    assert!(stray.is_empty(), "no effects from other connections: {stray:?}");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));
}

/// C4: a loss for a connection that is not live changes nothing; a
/// replacement connection settles the runs of the one it replaces.
#[test]
fn only_the_live_connection_can_be_lost() {
    let mut m = Machine::new();
    replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit(1),
            recorded(1),
            engine(1, EngineEventKind::RunAccepted { run: RunId(1) }),
        ],
    );
    let stale = replay(&mut m, vec![engine(7, EngineEventKind::ConnectionLost)]);
    assert!(stale.is_empty());
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));

    replay(&mut m, vec![engine(2, EngineEventKind::Connected)]);
    assert_eq!(
        m.run_state(RunId(1)),
        Some(RunState::OutcomeUnknown),
        "replacing a connection means the old one is gone"
    );
}
