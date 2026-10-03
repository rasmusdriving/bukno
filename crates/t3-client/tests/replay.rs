//! Replay: recorded frames from the live Ubuntu server (sanitized, see
//! `fixtures/sanitize.py`) through the real decoders and state code. Covers
//! failure rows F9, F10 and F12 in `e2e/scenarios/t3-client-failure-paths.md`.

use bukno_t3_client::model::{ServerConfig, ShellItem, ThreadItem, TurnKind};
use bukno_t3_client::shell::ShellState;
use bukno_t3_client::thread::ThreadState;
use serde_json::{Value, json};

fn records(name: &str) -> Vec<(String, Value)> {
    let path = format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            (record["stream"].as_str().unwrap().to_owned(), record["value"].clone())
        })
        .collect()
}

fn thread_items(name: &str) -> Vec<ThreadItem> {
    records(name)
        .into_iter()
        .filter(|(stream, _)| stream == "subscribeThread")
        .map(|(_, v)| ThreadItem::decode(&v).expect("recorded thread item decodes"))
        .collect()
}

#[test]
fn shell_and_config_from_the_live_server() {
    let mut shell = ShellState::default();
    let mut config = None;
    for (stream, value) in records("shell.jsonl") {
        match stream.as_str() {
            "server.getConfig" => config = Some(ServerConfig::decode(&value).expect("config decodes")),
            "subscribeShell" => {
                shell.apply(ShellItem::decode(&value).expect("shell item decodes"));
            }
            other => panic!("unexpected stream {other}"),
        }
    }
    let config = config.expect("config recorded");
    assert_eq!(config.environment.orchestration_protocol_version, 2);
    let codex = config.providers.iter().find(|p| p.instance_id == "codex").expect("codex provider");
    assert!(codex.enabled && !codex.models.is_empty());
    assert!(config.providers.iter().any(|p| p.instance_id == "claudeAgent" && p.enabled));

    assert!(shell.synchronized, "the catch-up marker arrived");
    assert_eq!(shell.projects.len(), 4);
    assert_eq!(shell.threads.len(), 12);
    // The three metadata-only frames after the marker must not move the cursor
    // or clear the chats.
    assert_eq!(shell.last_sequence, Some(6005));
    assert_eq!(shell.counts.snapshots, 1);
    assert_eq!(shell.counts.unknown, 0);
}

#[test]
fn long_thread_snapshot_keeps_every_row() {
    let mut thread = ThreadState::new("4be3bb38-9a61-40ad-8af9-80a1cc4074f0");
    for item in thread_items("long-thread.jsonl") {
        thread.apply(item);
    }
    assert!(thread.synchronized);
    assert_eq!(thread.rows.len(), 362);
    assert_eq!(thread.visible_rows().len(), 362);
    let unknown: Vec<&str> =
        thread.rows.iter().filter(|r| r.item.kind == TurnKind::Unknown).map(|r| r.item.type_name.as_str()).collect();
    assert!(unknown.is_empty(), "every recorded type is known: {unknown:?}");
    let users = thread.rows.iter().filter(|r| matches!(r.item.kind, TurnKind::UserMessage { .. })).count();
    let replies = thread.rows.iter().filter(|r| matches!(r.item.kind, TurnKind::AssistantMessage { .. })).count();
    assert_eq!((users, replies), (6, 32));
    // Rows stay in T3's order: ordinals never go down.
    assert!(thread.rows.windows(2).all(|w| w[0].item.ordinal <= w[1].item.ordinal));
}

/// Final rows, as (id, status) pairs, for comparing two runs.
fn shown(thread: &ThreadState) -> Vec<(String, String)> {
    thread.visible_rows().iter().map(|r| (r.item.id.clone(), r.item.status.clone())).collect()
}

#[test]
fn live_events_update_rows_in_place_and_resume_without_duplicates() {
    let items = thread_items("live-thread.jsonl");
    let id = "02f1b76f-37f1-4230-9895-a7363f56ce12";

    let mut straight = ThreadState::new(id);
    for item in items.clone() {
        straight.apply(item);
    }
    let snapshot_ids: std::collections::BTreeSet<String> = items
        .iter()
        .find_map(|i| match i {
            ThreadItem::Snapshot(s) => Some(s.items.iter().map(|r| r.item.id.clone()).collect()),
            _ => None,
        })
        .expect("the recording starts with a snapshot");
    let event_ids: std::collections::BTreeSet<String> = items
        .iter()
        .filter_map(|i| match i {
            ThreadItem::Event { event, .. } => match event.as_ref() {
                bukno_t3_client::model::ThreadEvent::TurnItem(t) => Some(t.id.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(!event_ids.is_empty(), "the recording has live turn items");
    let new_rows = event_ids.difference(&snapshot_ids).count();
    assert!(new_rows > 0, "some live items are new rows");
    // A row that goes from running to completed stays one row.
    assert_eq!(straight.rows.len(), snapshot_ids.len() + new_rows);
    let completed =
        straight.rows.iter().filter(|r| event_ids.contains(&r.item.id) && r.item.status == "completed").count();
    assert!(completed > 0, "updates replaced rows in place");
    assert!(straight.rows.windows(2).all(|w| w[0].item.ordinal <= w[1].item.ordinal));

    // A reconnect replays from an earlier cursor: overlapping events arrive
    // twice and must be dropped by sequence.
    let mut resumed = ThreadState::new(id);
    let split = items.len() / 2;
    for item in items[..split].iter().cloned() {
        resumed.apply(item);
    }
    resumed.subscription_started();
    let overlap = split.saturating_sub(5).max(2);
    resumed.apply(ThreadItem::Synchronized);
    for item in items[overlap..].iter().cloned() {
        resumed.apply(item);
    }
    assert_eq!(shown(&resumed), shown(&straight));
    assert!(resumed.counts.duplicates > 0, "the overlap was seen and dropped");
}

#[test]
fn unknown_types_stay_visible_and_move_the_cursor() {
    let mut thread = ThreadState::new("t1");
    thread.apply(
        ThreadItem::decode(&json!({
            "kind": "snapshot", "snapshotSequence": 10,
            "projection": {"thread": {"title": "T"}, "runs": [], "visibleTurnItems": []}
        }))
        .unwrap(),
    );
    // An unknown event type: skipped, but its sequence is consumed.
    let unknown_event = ThreadItem::decode(&json!({
        "kind": "event", "sequence": 11,
        "event": {"type": "galaxy.updated", "threadId": "t1", "payload": {}}
    }))
    .unwrap();
    assert!(matches!(unknown_event, ThreadItem::UnknownEvent { .. }));
    thread.apply(unknown_event);
    assert_eq!(thread.last_sequence, Some(11));
    assert_eq!(thread.unknown_event_types.iter().collect::<Vec<_>>(), ["galaxy.updated"]);

    // An unknown turn item type becomes a visible row.
    thread.apply(
        ThreadItem::decode(&json!({
            "kind": "event", "sequence": 12,
            "event": {"type": "turn-item.updated", "threadId": "t1", "payload": {
                "type": "hologram", "id": "i1", "threadId": "t1", "runId": null, "ordinal": 1, "status": "completed"
            }}
        }))
        .unwrap(),
    );
    let rows = thread.visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].item.kind.clone(), rows[0].item.type_name.as_str()), (TurnKind::Unknown, "hologram"));

    // Unknown stream kinds decode to Unknown instead of failing.
    assert!(matches!(ThreadItem::decode(&json!({"kind": "teleport"})).unwrap(), ThreadItem::Unknown { .. }));
    assert!(matches!(ShellItem::decode(&json!({"kind": "teleport"})).unwrap(), ShellItem::Unknown { .. }));

    // A known event with a broken payload is an error the caller handles (F10).
    assert!(
        ThreadItem::decode(&json!({
            "kind": "event", "sequence": 13,
            "event": {"type": "turn-item.updated", "threadId": "t1", "payload": {"type": "user_message"}}
        }))
        .is_err()
    );
}

fn event(sequence: u64, event_type: &str, payload: Value) -> ThreadItem {
    ThreadItem::decode(&json!({
        "kind": "event", "sequence": sequence,
        "event": {"type": event_type, "threadId": "t1", "payload": payload}
    }))
    .unwrap()
}

fn empty_snapshot() -> ThreadItem {
    ThreadItem::decode(&json!({
        "kind": "snapshot", "snapshotSequence": 10,
        "projection": {"thread": {"title": "T"}, "runs": [], "attempts": [], "visibleTurnItems": []}
    }))
    .unwrap()
}

fn item(id: &str, item_type: &str, ordinal: u64, node: &str) -> Value {
    json!({"type": item_type, "id": id, "threadId": "t1", "runId": "r1", "nodeId": node,
           "ordinal": ordinal, "status": "completed", "message": "m"})
}

#[test]
fn a_finished_run_is_published_as_no_longer_working() {
    let mut thread = ThreadState::new("t1");
    thread.apply(empty_snapshot());
    assert!(thread.apply(event(11, "run.created", json!({"id": "r1", "status": "running"}))));
    assert!(thread.working());
    // running -> completed changes what is shown (the working state), so it publishes.
    assert!(thread.apply(event(12, "run.updated", json!({"id": "r1", "status": "completed"}))));
    assert!(!thread.working());
}

#[test]
fn superseded_interrupt_results_are_hidden_as_in_t3() {
    let mut thread = ThreadState::new("t1");
    thread.apply(empty_snapshot());
    thread.apply(event(11, "turn-item.updated", item("i1", "run_interrupt_result", 1, "n1")));
    assert_eq!(thread.visible_rows().len(), 1);
    thread.apply(event(
        12,
        "run-attempt.updated",
        json!({"id": "a1", "runId": "r1", "rootNodeId": "n1", "status": "superseded"}),
    ));
    assert_eq!(thread.visible_rows().len(), 0, "hidden: superseded and no interrupt request");
    // A paired stop-then-steer keeps it visible.
    thread.apply(event(13, "turn-item.updated", item("i0", "run_interrupt_request", 0, "n0")));
    assert_eq!(thread.visible_rows().len(), 2);
}

#[test]
fn a_reloaded_chat_never_repeats_a_revision() {
    let mut first = ThreadState::new("t1");
    first.apply(empty_snapshot());
    first.apply(event(11, "turn-item.updated", item("i1", "system_notice", 1, "n1")));
    let seen_rows: Vec<u64> = first.rows.iter().map(|r| r.revision).collect();
    let mut again = ThreadState::new("t1");
    assert_ne!(again.incarnation, first.incarnation);
    again.apply(empty_snapshot());
    again.apply(event(11, "turn-item.updated", item("i1", "system_notice", 1, "n1")));
    assert!(again.revision > first.revision);
    assert!(again.rows.iter().all(|r| !seen_rows.contains(&r.revision)));
}

#[test]
fn an_interrupt_request_outside_the_loaded_window_keeps_its_result_visible() {
    // A bounded snapshot: the request is only in the full turnItems, not in
    // the visible window. T3 keeps the superseded attempt's result visible.
    let mut thread = ThreadState::new("t1");
    thread.apply(
        ThreadItem::decode(&json!({
            "kind": "snapshot", "snapshotSequence": 10, "hasMoreHistory": true, "historyCursor": "c1",
            "projection": {
                "thread": {"title": "T"}, "runs": [],
                "attempts": [{"id": "a1", "runId": "r1", "rootNodeId": "n1", "status": "superseded"}],
                "turnItems": [item("i0", "run_interrupt_request", 0, "n0"), item("i1", "run_interrupt_result", 1, "n1")],
                "visibleTurnItems": [{"position": 0, "visibility": "local", "sourceThreadId": "t1",
                    "sourceItemId": "i1", "item": item("i1", "run_interrupt_result", 1, "n1")}]
            }
        }))
        .unwrap(),
    );
    assert_eq!(thread.rows.len(), 1);
    assert_eq!(thread.visible_rows().len(), 1, "the request outside the window still counts");
}

#[test]
fn a_live_interrupt_request_outside_the_window_republishes_the_chat() {
    let mut thread = ThreadState::new("t1");
    thread.apply(
        ThreadItem::decode(&json!({
            "kind": "snapshot", "snapshotSequence": 10, "hasMoreHistory": true, "historyCursor": "c1",
            "latestLocalTurnOrdinal": 5,
            "projection": {
                "thread": {"title": "T"}, "runs": [],
                "attempts": [{"id": "a1", "runId": "r1", "rootNodeId": "n1", "status": "superseded"}],
                "turnItems": [], "visibleTurnItems": [{"position": 0, "visibility": "local", "sourceThreadId": "t1",
                    "sourceItemId": "i1", "item": item("i1", "run_interrupt_result", 5, "n1")}]
            }
        }))
        .unwrap(),
    );
    assert_eq!(thread.visible_rows().len(), 0);
    let before = thread.revision;
    // The request is older than the loaded window, so its row is not added,
    // but the chat must still publish: the result is now visible.
    assert!(thread.apply(event(11, "turn-item.updated", item("i0", "run_interrupt_request", 0, "n0"))));
    assert!(thread.revision != before);
    assert_eq!(thread.visible_rows().len(), 1);
}
