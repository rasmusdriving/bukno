//! Replays of recorded input sequences through the real coordinator state
//! machine (specification section 2).
//!
//! Connection identity, enumerated for PR 1:
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
//!    dead one.
//!
//! Pass 1 rows, numbered as in e2e/scenarios/pass1-codex-failure-paths.md:
//!
//! C16 A repeated Send creates a second message or turn.
//! C17 A message goes to whichever chat is selected when it is written.
//! C22 An approval answered after its run ended, or for an older
//!     connection, reaches the engine.
//! C23 A decision from a restarted engine can be answered by an old card.
//! C25 Stop and completion race to two different outcomes.
//! C30 Two chats write to the same folder, or one inside the other, at once.
//! C41 More runs start than the limit allows, or the extra one is lost.
//! R1  After a restart, something that may have been sent is sent again, or
//!     a queued message replays by itself.
//! R2  Reconciliation cannot settle an unknown outcome, or settles it as a
//!     success without the provider saying so.
//! R3  A chat that continued outside Bukno is sent to before its missed
//!     turns are loaded.

use bukno_core::decision::{DecisionAnswer, DecisionKind, DecisionState};
use bukno_core::event::{
    Command, Effect, EngineEvent, EngineEventKind, EngineRequest, ImportedItem, Input, OpenRun, PersistRequest,
    Reconciliation, RejectReason, Snapshot, StorageResult, ViewUpdate, WaitReason,
};
use bukno_core::ids::{DecisionId, ItemId, MessageId, RunId, TaskId, WorkspaceId};
use bukno_core::machine::Machine;
use bukno_core::message::{DeliveryState, Provider};
use bukno_core::run::{RunOutcome, RunState};
use bukno_core::task::{RunSettings, SessionInfo, TaskInfo, WorkspaceInfo, WorkspaceKind, Writes};

const TASK: TaskId = TaskId(1);

fn workspace(id: u128, path: &[&str]) -> WorkspaceInfo {
    WorkspaceInfo {
        id: WorkspaceId(id),
        path: format!("/{}", path.join("/")),
        key: path.iter().map(|s| s.to_string()).collect(),
        kind: WorkspaceKind::Project,
        git_root: None,
        identity: None,
        available: true,
    }
}

fn task_in(task: TaskId, ws: WorkspaceId) -> TaskInfo {
    TaskInfo {
        id: task,
        title: format!("Chat {}", task.0),
        provider: Provider::Codex,
        project: None,
        workspace: ws,
        session: None,
        created: task.0 as i64,
    }
}

/// A machine with chats 1 to `n`, each in its own folder.
fn machine_with(n: u128) -> Machine {
    let mut m = Machine::new();
    for t in 1..=n {
        let ws = workspace(100 + t, &["work", &format!("chat{t}")]);
        replay(
            &mut m,
            vec![
                Input::Command(Command::CreateChat { task: task_in(TaskId(t), ws.id), workspace: ws }),
                Input::Stored(StorageResult::ChatSaved { task: TaskId(t) }),
            ],
        );
    }
    m
}

fn settings(writes: Writes) -> RunSettings {
    RunSettings { preset: "test".into(), writes, model: None, effort: None }
}

fn submit_to(task: TaskId, n: u128, writes: Writes) -> Input {
    Input::Command(Command::SubmitMessage {
        task,
        provider: Provider::Codex,
        message: MessageId(n),
        run: RunId(n),
        item: ItemId(n),
        draft_revision: 1,
        body: format!("message {n}"),
        settings: settings(writes),
    })
}

fn submit(n: u128) -> Input {
    submit_to(TASK, n, Writes::Never)
}

fn recorded(n: u128) -> Input {
    Input::Stored(StorageResult::DeliveryRecorded { message: MessageId(n) })
}

fn engine(generation: u64, kind: EngineEventKind) -> Input {
    Input::Engine(EngineEvent { provider: Provider::Codex, connection_generation: generation, kind })
}

fn accepted(n: u128) -> EngineEventKind {
    EngineEventKind::RunAccepted { run: RunId(n), turn: format!("turn-{n}") }
}

fn lost() -> EngineEventKind {
    EngineEventKind::ConnectionLost { reason: "test".into() }
}

fn delta(n: u128, text: &str) -> EngineEventKind {
    EngineEventKind::TextDelta { run: RunId(n), item: ItemId(40), provider_item: "msg".into(), delta: text.into() }
}

fn ended(n: u128, outcome: RunOutcome) -> EngineEventKind {
    EngineEventKind::RunEnded { run: RunId(n), outcome, reason: None }
}

fn writes(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Engine(_, EngineRequest::StartTurn { .. }))).count()
}

fn started_runs(effects: &[Effect]) -> Vec<RunId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Engine(_, EngineRequest::StartTurn { run, .. }) => Some(*run),
            _ => None,
        })
        .collect()
}

fn rejected(effects: &[Effect], reason: &RejectReason) -> bool {
    effects.iter().any(|e| matches!(e, Effect::Rejected { reason: r, .. } if r == reason))
}

/// Replay `inputs` and return every effect, in order.
fn replay(machine: &mut Machine, inputs: Vec<Input>) -> Vec<Effect> {
    inputs.into_iter().flat_map(|input| machine.step(input)).collect()
}

/// C1, C5: nothing is written until a connection exists, then exactly once.
#[test]
fn turn_waits_for_a_connection_and_is_written_once() {
    let mut m = machine_with(1);
    let before = replay(&mut m, vec![submit(1), recorded(1)]);
    assert_eq!(writes(&before), 0, "no connection yet, nothing written");
    assert!(before.contains(&Effect::Engine(Provider::Codex, EngineRequest::Connect)), "the engine is started");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Preparing));

    let after = replay(&mut m, vec![engine(1, EngineEventKind::Connected), engine(1, EngineEventKind::Connected)]);
    assert_eq!(writes(&after), 1, "written once when the connection arrives, not again on a repeat");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Starting));
}

/// C2: losing the first connection settles its run, and the chat accepts new work.
#[test]
fn first_connection_loss_settles_the_run() {
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), recorded(1), engine(1, accepted(1))]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));

    let lost = replay(&mut m, vec![engine(1, lost())]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::OutcomeUnknown));
    assert!(lost.contains(&Effect::Publish(ViewUpdate::RunStateChanged {
        task: TASK,
        run: RunId(1),
        state: RunState::OutcomeUnknown
    })));

    // Not rejected as RunActive; it waits for a new connection instead.
    let next = replay(&mut m, vec![submit(2), recorded(2)]);
    assert!(!rejected(&next, &RejectReason::RunActive));
    assert_eq!(writes(&next), 0, "no live connection after the loss");
    let reconnected = replay(&mut m, vec![engine(2, EngineEventKind::Connected)]);
    assert_eq!(writes(&reconnected), 1);
}

/// C3: events from any other connection cannot change the run.
#[test]
fn events_from_another_connection_are_ignored() {
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), recorded(1), engine(1, accepted(1))]);
    let stray = replay(
        &mut m,
        vec![
            engine(2, delta(1, "wrong connection")),
            engine(0, delta(1, "older connection")),
            engine(2, ended(1, RunOutcome::Completed)),
        ],
    );
    assert!(stray.is_empty(), "no effects from other connections: {stray:?}");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));
}

/// C4: a loss for a connection that is not live changes nothing; a
/// replacement connection settles the runs of the one it replaces.
#[test]
fn only_the_live_connection_can_be_lost() {
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), recorded(1), engine(1, accepted(1))]);
    let stale = replay(&mut m, vec![engine(7, lost())]);
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
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(2, EngineEventKind::Connected), submit(1), recorded(1), engine(2, accepted(1))]);
    let late = replay(&mut m, vec![engine(1, EngineEventKind::Connected)]);
    assert!(late.is_empty(), "a late Connected changes nothing: {late:?}");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));
    let still = replay(&mut m, vec![engine(2, delta(1, "still live"))]);
    assert!(!still.is_empty(), "generation 2 is still the live connection");

    replay(&mut m, vec![engine(2, lost())]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::OutcomeUnknown));
    let next = replay(&mut m, vec![submit(2), recorded(2)]);
    assert_eq!(writes(&next), 0, "no live connection after the loss");
    let revived = replay(&mut m, vec![engine(2, EngineEventKind::Connected), engine(1, EngineEventKind::Connected)]);
    assert!(revived.is_empty(), "a dead connection is not revived: {revived:?}");
    let ignored = replay(&mut m, vec![engine(2, accepted(2))]);
    assert!(ignored.is_empty(), "events from the dead connection stay ignored");

    let fresh = replay(&mut m, vec![engine(3, EngineEventKind::Connected)]);
    assert_eq!(writes(&fresh), 1);
    assert_eq!(m.run_state(RunId(2)), Some(RunState::Starting));
}

/// C16: the same message ID twice is one message and one turn.
#[test]
fn repeated_send_is_one_message() {
    let mut m = machine_with(1);
    let fx = replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), submit(1), recorded(1)]);
    assert!(rejected(&fx, &RejectReason::DuplicateMessage));
    assert_eq!(writes(&fx), 1);
    let records = fx.iter().filter(|e| matches!(e, Effect::Persist(PersistRequest::RecordDelivery { .. }))).count();
    assert_eq!(records, 1, "one outbox record");
}

/// C17: the recipient is the chat the command named, whatever is selected later.
#[test]
fn message_stays_with_the_captured_chat() {
    let mut m = machine_with(2);
    let fx = replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            Input::Command(Command::SelectChat { task: TaskId(1) }),
            submit_to(TaskId(1), 1, Writes::Never),
            Input::Command(Command::SelectChat { task: TaskId(2) }),
            recorded(1),
        ],
    );
    let targets: Vec<TaskId> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Engine(_, EngineRequest::StartTurn { task, .. }) => Some(*task),
            _ => None,
        })
        .collect();
    assert_eq!(targets, vec![TaskId(1)]);
}

fn decision_requested(generation: u64, run: u128, id: u128) -> Input {
    engine(
        generation,
        EngineEventKind::DecisionRequested {
            run: RunId(run),
            decision: DecisionId(id),
            kind: DecisionKind::Command { command: "touch a".into(), cwd: None, reason: None },
        },
    )
}

fn answer(id: u128, run: u128, generation: u64) -> Input {
    Input::Command(Command::AnswerDecision {
        decision: DecisionId(id),
        run: RunId(run),
        generation,
        answer: DecisionAnswer::Allow,
    })
}

fn answers_sent(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Engine(_, EngineRequest::Answer { .. }))).count()
}

/// C22: a decision can be answered once, only while its run and connection live.
#[test]
fn stale_decisions_are_rejected() {
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), recorded(1), engine(1, accepted(1))]);
    replay(&mut m, vec![decision_requested(1, 1, 9)]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::WaitingForApproval));

    // Wrong generation: rejected, nothing sent.
    let wrong = replay(&mut m, vec![answer(9, 1, 2)]);
    assert!(rejected(&wrong, &RejectReason::StaleDecision));
    assert_eq!(answers_sent(&wrong), 0);

    // Right answer: sent once; a second click is stale.
    let first = replay(&mut m, vec![answer(9, 1, 1)]);
    assert_eq!(answers_sent(&first), 1);
    assert_eq!(m.decision_state(DecisionId(9)), Some(DecisionState::Sending));
    let again = replay(&mut m, vec![answer(9, 1, 1)]);
    assert!(rejected(&again, &RejectReason::StaleDecision));
    replay(&mut m, vec![engine(1, EngineEventKind::DecisionResolved { decision: DecisionId(9) })]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Running));

    // A new request on a run that then ends: the answer is too late.
    replay(&mut m, vec![decision_requested(1, 1, 10), engine(1, ended(1, RunOutcome::Completed))]);
    let late = replay(&mut m, vec![answer(10, 1, 1)]);
    assert_eq!(answers_sent(&late), 0, "an answer after the run ended never reaches the engine");
}

/// C23: decisions are keyed by connection; an old card cannot answer a new engine.
#[test]
fn old_cards_cannot_answer_a_restarted_engine() {
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), recorded(1), engine(1, accepted(1))]);
    replay(&mut m, vec![decision_requested(1, 1, 9), engine(1, lost())]);
    assert_eq!(m.decision_state(DecisionId(9)), None, "expired with its run");
    let late = replay(&mut m, vec![engine(2, EngineEventKind::Connected), answer(9, 1, 1)]);
    assert_eq!(answers_sent(&late), 0);
}

/// C25: Stop then completion, or completion then Stop: one settled outcome.
#[test]
fn stop_and_completion_settle_once() {
    let mut m = machine_with(1);
    replay(&mut m, vec![engine(1, EngineEventKind::Connected), submit(1), recorded(1), engine(1, accepted(1))]);
    let stop = replay(&mut m, vec![Input::Command(Command::InterruptRun { task: TASK, run: RunId(1) })]);
    assert!(stop.contains(&Effect::Engine(Provider::Codex, EngineRequest::Interrupt { run: RunId(1) })));
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Cancelling));
    replay(&mut m, vec![engine(1, ended(1, RunOutcome::Completed))]);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Completed), "the engine's own outcome wins");
    let late = replay(&mut m, vec![engine(1, ended(1, RunOutcome::Interrupted))]);
    assert!(late.is_empty(), "a second ending changes nothing");
    let stop_after = replay(&mut m, vec![Input::Command(Command::InterruptRun { task: TASK, run: RunId(1) })]);
    assert!(!stop_after.iter().any(|e| matches!(e, Effect::Engine(..))), "Stop after the end sends nothing");
}

/// Stop before the outbox commit lands: nothing is ever written.
#[test]
fn stop_before_commit_never_writes() {
    let mut m = machine_with(1);
    let fx = replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit(1),
            Input::Command(Command::InterruptRun { task: TASK, run: RunId(1) }),
            recorded(1),
        ],
    );
    assert_eq!(writes(&fx), 0);
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Interrupted));
    assert_eq!(m.delivery_state(MessageId(1)), Some(DeliveryState::Rejected));
}

/// C30: same folder, nested folder: the second writer queues; Run anyway is
/// explicit; a read-only run goes alongside.
#[test]
fn second_writer_waits_for_the_workspace() {
    let mut m = Machine::new();
    let outer = workspace(201, &["code", "app"]);
    let nested = workspace(202, &["code", "app", "src"]);
    let other = workspace(203, &["code", "other"]);
    for (t, ws) in [(1, &outer), (2, &nested), (3, &other), (4, &outer)] {
        replay(
            &mut m,
            vec![
                Input::Command(Command::CreateChat { task: task_in(TaskId(t), ws.id), workspace: ws.clone() }),
                Input::Stored(StorageResult::ChatSaved { task: TaskId(t) }),
            ],
        );
    }
    let mut m = { m };
    let fx = replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit_to(TaskId(1), 1, Writes::Freely),
            recorded(1),
            submit_to(TaskId(2), 2, Writes::Freely),
            recorded(2),
        ],
    );
    assert_eq!(started_runs(&fx), vec![RunId(1)], "the nested folder's writer waits");
    assert!(m.holds_lease(RunId(1)));
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Publish(ViewUpdate::RunWaiting { run: RunId(2), reason: Some(WaitReason::Workspace { .. }), .. })
    )));

    // A read-only run in the same folder is not blocked, and takes no lease.
    let reader = replay(&mut m, vec![submit_to(TaskId(4), 4, Writes::Never), recorded(4)]);
    assert_eq!(started_runs(&reader), vec![RunId(4)]);
    assert!(!m.holds_lease(RunId(4)));

    // Run anyway is explicit: it goes once the slot frees, labeled shared.
    replay(&mut m, vec![engine(1, ended(4, RunOutcome::Completed))]);
    let anyway = replay(&mut m, vec![Input::Command(Command::RunAnyway { run: RunId(2) }), recorded(2)]);
    assert_eq!(started_runs(&anyway), vec![RunId(2)]);
    let shared = anyway.iter().rev().find_map(|e| match e {
        Effect::Publish(ViewUpdate::Chats { chats, .. }) => Some(chats.clone()),
        _ => None,
    });
    let shared = shared.expect("sidebar published");
    assert!(shared.iter().filter(|c| c.shared_workspace).count() >= 2, "both chats are labeled: {shared:?}");
}

/// C30: releasing the lease lets the queued writer go, in order.
#[test]
fn queued_writer_goes_when_the_lease_frees() {
    let mut m = Machine::new();
    let ws = workspace(301, &["code", "app"]);
    for t in [1, 2] {
        replay(
            &mut m,
            vec![
                Input::Command(Command::CreateChat { task: task_in(TaskId(t), ws.id), workspace: ws.clone() }),
                Input::Stored(StorageResult::ChatSaved { task: TaskId(t) }),
            ],
        );
    }
    replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit_to(TaskId(1), 1, Writes::Freely),
            recorded(1),
            submit_to(TaskId(2), 2, Writes::AfterApproval),
            recorded(2),
            engine(1, accepted(1)),
        ],
    );
    assert_eq!(m.delivery_state(MessageId(2)), Some(DeliveryState::Queued));
    let fx = replay(&mut m, vec![engine(1, ended(1, RunOutcome::Completed))]);
    assert!(fx.contains(&Effect::Persist(PersistRequest::MarkAboutToSend { message: MessageId(2) })));
    assert!(!m.holds_lease(RunId(1)));
    let go = replay(&mut m, vec![recorded(2)]);
    assert_eq!(started_runs(&go), vec![RunId(2)], "written only after its new state commits");
}

/// C41: the third run waits visibly and is sent when a slot frees.
#[test]
fn run_limit_queues_the_third() {
    let mut m = machine_with(3);
    let fx = replay(
        &mut m,
        vec![
            engine(1, EngineEventKind::Connected),
            submit_to(TaskId(1), 1, Writes::Never),
            recorded(1),
            submit_to(TaskId(2), 2, Writes::Never),
            recorded(2),
            submit_to(TaskId(3), 3, Writes::Never),
            recorded(3),
        ],
    );
    assert_eq!(started_runs(&fx), vec![RunId(1), RunId(2)]);
    assert_eq!(m.delivery_state(MessageId(3)), Some(DeliveryState::Queued));
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Publish(ViewUpdate::RunWaiting { run: RunId(3), reason: Some(WaitReason::Slots { limit: 2 }), .. })
    )));
    replay(&mut m, vec![engine(1, ended(1, RunOutcome::Completed))]);
    let go = replay(&mut m, vec![recorded(3)]);
    assert_eq!(started_runs(&go), vec![RunId(3)]);
}

fn snapshot_with(open: Vec<OpenRun>, session: bool) -> Snapshot {
    let ws = workspace(401, &["code", "app"]);
    let mut task = task_in(TASK, ws.id);
    if session {
        task.session = Some(SessionInfo { thread: "thread-1".into(), latest_turn: Some("turn-0".into()) });
    }
    Snapshot {
        workspaces: vec![ws],
        projects: vec![],
        tasks: vec![task],
        open_runs: open,
        decisions: vec![DecisionId(5)],
    }
}

fn open_run(n: u128, delivery: DeliveryState, state: RunState) -> OpenRun {
    OpenRun {
        run: RunId(n),
        task: TASK,
        state,
        settings: settings(Writes::Freely),
        turn: None,
        message: MessageId(n),
        delivery,
        body: format!("message {n}"),
    }
}

/// R1: after a restart nothing is resent; queued messages pause; pending
/// decisions expire; uncertain runs keep their lease.
#[test]
fn restart_never_resends() {
    let mut m = Machine::new();
    let fx = replay(
        &mut m,
        vec![
            Input::Stored(StorageResult::Restored(snapshot_with(
                vec![open_run(1, DeliveryState::Sent, RunState::Running)],
                true,
            ))),
            engine(1, EngineEventKind::Connected),
        ],
    );
    assert_eq!(writes(&fx), 0, "never resent automatically");
    assert_eq!(m.run_state(RunId(1)), Some(RunState::OutcomeUnknown));
    assert_eq!(m.delivery_state(MessageId(1)), Some(DeliveryState::Unknown));
    assert!(m.holds_lease(RunId(1)), "an uncertain run keeps its workspace");
    assert!(fx.contains(&Effect::Persist(PersistRequest::DecisionState {
        decision: DecisionId(5),
        state: DecisionState::Expired
    })));
    assert!(fx.iter().any(|e| matches!(e, Effect::Engine(_, EngineRequest::Reconcile { run: RunId(1), .. }))));

    let mut q = Machine::new();
    let queued = replay(
        &mut q,
        vec![
            Input::Stored(StorageResult::Restored(snapshot_with(
                vec![open_run(2, DeliveryState::Queued, RunState::Preparing)],
                false,
            ))),
            engine(1, EngineEventKind::Connected),
        ],
    );
    assert_eq!(writes(&queued), 0, "a queued message does not replay by itself");
    let go = replay(&mut q, vec![Input::Command(Command::SendQueued { run: RunId(2) }), recorded(2)]);
    assert_eq!(started_runs(&go), vec![RunId(2)], "it goes when the user says so");
}

/// R2: reconciliation settles an unknown run only from the provider's answer.
#[test]
fn reconciliation_uses_the_provider_answer() {
    let mut m = Machine::new();
    replay(
        &mut m,
        vec![
            Input::Stored(StorageResult::Restored(snapshot_with(
                vec![open_run(1, DeliveryState::Sent, RunState::Running)],
                true,
            ))),
            engine(1, EngineEventKind::Connected),
        ],
    );
    let found = replay(
        &mut m,
        vec![engine(
            1,
            EngineEventKind::Reconciled {
                run: RunId(1),
                result: Reconciliation::Found {
                    turn: "turn-1".into(),
                    outcome: Some(RunOutcome::Completed),
                    items: vec![ImportedItem { provider_item: "msg_1".into(), user: false, text: "done".into() }],
                },
            },
        )],
    );
    assert_eq!(m.run_state(RunId(1)), Some(RunState::Completed));
    assert_eq!(m.delivery_state(MessageId(1)), Some(DeliveryState::Acknowledged));
    assert!(!m.holds_lease(RunId(1)));
    assert!(found.iter().any(|e| matches!(e, Effect::Persist(PersistRequest::ImportItems { .. }))));

    let mut n = Machine::new();
    replay(
        &mut n,
        vec![
            Input::Stored(StorageResult::Restored(snapshot_with(
                vec![open_run(1, DeliveryState::AboutToSend, RunState::Preparing)],
                true,
            ))),
            engine(1, EngineEventKind::Connected),
            engine(1, EngineEventKind::Reconciled { run: RunId(1), result: Reconciliation::NotFound }),
        ],
    );
    assert_eq!(n.run_state(RunId(1)), Some(RunState::OutcomeUnknown), "no trace is still unknown, not success");
    let resend = replay(
        &mut n,
        vec![
            Input::Command(Command::Resend {
                unknown: RunId(1),
                message: MessageId(2),
                run: RunId(2),
                item: ItemId(2),
                settings: settings(Writes::Freely),
            }),
            recorded(2),
            engine(1, EngineEventKind::OutsideChecked { task: TASK, latest_turn: None, items: vec![] }),
        ],
    );
    assert_eq!(started_runs(&resend), vec![RunId(2)], "an explicit resend goes, once");
}

/// R3: before the first send on a connection, missed outside turns load first.
#[test]
fn outside_turns_load_before_a_send() {
    let mut m = Machine::new();
    let fx = replay(
        &mut m,
        vec![
            Input::Stored(StorageResult::Restored(snapshot_with(vec![], true))),
            engine(1, EngineEventKind::Connected),
            submit_to(TASK, 1, Writes::Freely),
            recorded(1),
        ],
    );
    assert_eq!(writes(&fx), 0, "the send waits for the check");
    assert!(fx.iter().any(|e| matches!(e, Effect::Engine(_, EngineRequest::CheckOutside { .. }))));
    let checked = replay(
        &mut m,
        vec![engine(
            1,
            EngineEventKind::OutsideChecked {
                task: TASK,
                latest_turn: Some("turn-cli".into()),
                items: vec![ImportedItem { provider_item: "u1".into(), user: true, text: "from the CLI".into() }],
            },
        )],
    );
    let import_before = checked.iter().find_map(|e| match e {
        Effect::Persist(PersistRequest::ImportItems { before, .. }) => Some(*before),
        _ => None,
    });
    assert_eq!(import_before, Some(Some(ItemId(1))), "missed turns go before the new message");
    let order: Vec<usize> = checked
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(
                e,
                Effect::Persist(PersistRequest::ImportItems { .. })
                    | Effect::Engine(_, EngineRequest::StartTurn { .. })
            )
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(order.len(), 2);
    assert_eq!(writes(&checked), 1);
    let session = checked.iter().find_map(|e| match e {
        Effect::Engine(_, EngineRequest::StartTurn { session, .. }) => session.clone(),
        _ => None,
    });
    assert_eq!(session.and_then(|s| s.latest_turn).as_deref(), Some("turn-cli"));
}
