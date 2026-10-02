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
//! C6 A late `Connected` from an older connection, or a repeated one after
//!    its connection was lost, replaces the live connection or revives a
//!    dead one (PR 1, second review).

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

/// C6: generations only increase. A late or repeated `Connected` from an
/// older generation changes nothing, before or after a loss.
#[test]
fn late_connected_notices_are_ignored() {
    let mut m = Machine::new();
    replay(
        &mut m,
        vec![
            engine(2, EngineEventKind::Connected),
            submit(1),
            recorded(1),
            engine(2, EngineEventKind::RunAccepted { run: RunId(1) }),
        ],
    );
    // Out of order: generation 1's notice arrives after generation 2 is live.
    let late = replay(&mut m, vec![engine(1, EngineEventKind::Connected)]);
    assert!(late.is_empty(), "a late Connected changes nothing: {late:?}");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));
    let delta = replay(
        &mut m,
        vec![engine(2, EngineEventKind::TextDelta { run: RunId(1), item: ItemId(40), delta: "still live".into() })],
    );
    assert!(!delta.is_empty(), "generation 2 is still the live connection");

    // After generation 2 is lost, neither it nor an older one can come back.
    replay(&mut m, vec![engine(2, EngineEventKind::ConnectionLost)]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::OutcomeUnknown));
    let next = replay(&mut m, vec![submit(2), recorded(2)]);
    assert_eq!(writes(&next), 0, "no live connection after the loss");
    let revived = replay(&mut m, vec![engine(2, EngineEventKind::Connected), engine(1, EngineEventKind::Connected)]);
    assert!(revived.is_empty(), "a dead connection is not revived: {revived:?}");
    let ignored = replay(&mut m, vec![engine(2, EngineEventKind::RunAccepted { run: RunId(2) })]);
    assert!(ignored.is_empty(), "events from the dead connection stay ignored");

    // Only a newer generation is accepted, and it writes the waiting turn once.
    let fresh = replay(&mut m, vec![engine(3, EngineEventKind::Connected)]);
    assert_eq!(writes(&fresh), 1);
    assert_eq!(m.run_state(RunId(2)), Some(RunState::Starting));
}
