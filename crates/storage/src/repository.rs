//! Reads and writes for each record. Every function runs on the storage
//! worker's connection; multi-row changes run in one transaction.

use bukno_core::decision::DecisionView;
use bukno_core::event::{ImportedItem, OpenRun, PersistRequest, Snapshot};
use bukno_core::ids::{DecisionId, ItemId, MessageId, ProjectId, RunId, TaskId, WorkspaceId};
use bukno_core::message::{ItemKind, TranscriptItem};
use bukno_core::task::{ProjectInfo, SessionInfo, TaskInfo, WorkspaceInfo, WorkspaceKind};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::codec;

/// Most transcript items loaded for one chat at a time.
pub const HISTORY_LIMIT: i64 = 2_000;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

fn id<T>(text: String, make: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    make(&text).ok_or_else(|| rusqlite::Error::InvalidColumnType(0, text, rusqlite::types::Type::Text))
}

/// Everything the coordinator needs at launch. Workspace availability is
/// filled in by the runtime, which checks the folders.
pub fn load_snapshot(conn: &Connection) -> rusqlite::Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    let mut stmt = conn.prepare("SELECT id, path, key, kind, git_root, fs_identity FROM workspace")?;
    let rows = stmt.query_map([], |r| {
        Ok(WorkspaceInfo {
            id: id(r.get(0)?, WorkspaceId::from_hex)?,
            path: r.get(1)?,
            key: codec::parse_key(&r.get::<_, String>(2)?),
            kind: if r.get::<_, String>(3)? == "project" { WorkspaceKind::Project } else { WorkspaceKind::Chat },
            git_root: r.get(4)?,
            identity: r.get(5)?,
            available: true,
        })
    })?;
    snapshot.workspaces = rows.collect::<Result<_, _>>()?;

    let mut stmt = conn.prepare("SELECT id, name, workspace_id FROM project WHERE archived = 0 ORDER BY created_at")?;
    let rows = stmt.query_map([], |r| {
        Ok(ProjectInfo {
            id: id(r.get(0)?, ProjectId::from_hex)?,
            name: r.get(1)?,
            workspace: id(r.get(2)?, WorkspaceId::from_hex)?,
        })
    })?;
    snapshot.projects = rows.collect::<Result<_, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.provider, t.project_id, t.workspace_id, t.created_at, s.thread_id, s.latest_turn_id
         FROM task t LEFT JOIN provider_session s ON s.task_id = t.id
         WHERE t.archived = 0",
    )?;
    let rows = stmt.query_map([], |r| {
        let thread: Option<String> = r.get(6)?;
        Ok(TaskInfo {
            id: id(r.get(0)?, TaskId::from_hex)?,
            title: r.get(1)?,
            provider: codec::parse_provider(&r.get::<_, String>(2)?),
            project: r.get::<_, Option<String>>(3)?.and_then(|p| ProjectId::from_hex(&p)),
            workspace: id(r.get(4)?, WorkspaceId::from_hex)?,
            created: r.get(5)?,
            session: thread.map(|thread| SessionInfo { thread, latest_turn: r.get(7).ok().flatten() }),
        })
    })?;
    snapshot.tasks = rows.collect::<Result<_, _>>()?;

    // Runs that had not settled, and unknown outcomes the user has not dismissed.
    let mut stmt = conn.prepare(
        "SELECT r.id, r.task_id, r.state, r.settings, r.turn_id, d.id, d.state, d.body
         FROM run r JOIN message_delivery d ON d.run_id = r.id
         WHERE (r.state NOT IN ('completed', 'failed', 'interrupted', 'outcome_unknown')
                OR (r.state = 'outcome_unknown' AND r.dismissed = 0))
           AND d.state <> 'rejected'
         ORDER BY r.created_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(OpenRun {
            run: id(r.get(0)?, RunId::from_hex)?,
            task: id(r.get(1)?, TaskId::from_hex)?,
            state: codec::parse_run_state(&r.get::<_, String>(2)?),
            settings: codec::parse_settings(&r.get::<_, String>(3)?),
            turn: r.get(4)?,
            message: id(r.get(5)?, MessageId::from_hex)?,
            delivery: codec::parse_delivery(&r.get::<_, String>(6)?),
            body: r.get(7)?,
        })
    })?;
    snapshot.open_runs = rows.collect::<Result<_, _>>()?;

    let mut stmt = conn.prepare("SELECT id FROM pending_decision WHERE state IN ('pending', 'sending')")?;
    let rows = stmt.query_map([], |r| id(r.get(0)?, DecisionId::from_hex))?;
    snapshot.decisions = rows.collect::<Result<_, _>>()?;
    Ok(snapshot)
}

/// A chat's transcript and its saved draft (text and revision).
pub type History = (Vec<TranscriptItem>, Option<(String, u64)>);

/// A chat's transcript, oldest first, and its draft.
pub fn load_history(conn: &Connection, task: TaskId) -> rusqlite::Result<History> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, kind, provider, text, meta, completed, revision FROM (
             SELECT * FROM transcript_item WHERE task_id = ?1 ORDER BY seq DESC LIMIT ?2
         ) ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![task.hex(), HISTORY_LIMIT], |r| {
        let kind: String = r.get(2)?;
        let provider: Option<String> = r.get(3)?;
        Ok(TranscriptItem {
            id: id(r.get(0)?, ItemId::from_hex)?,
            task,
            run: r.get::<_, Option<String>>(1)?.and_then(|v| RunId::from_hex(&v)),
            kind: if kind == "user" {
                ItemKind::UserMessage
            } else {
                ItemKind::AgentMessage { provider: codec::parse_provider(provider.as_deref().unwrap_or("codex")) }
            },
            text: r.get(4)?,
            meta: r.get(5)?,
            completed: r.get::<_, i64>(6)? != 0,
            revision: r.get::<_, i64>(7)? as u64,
        })
    })?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    let draft = conn
        .query_row("SELECT text, revision FROM draft WHERE task_id = ?1", params![task.hex()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })
        .optional()?;
    // An empty draft still carries its revision, so later saves are numbered after it.
    Ok((items, draft))
}

/// Apply one persist request. Multi-row requests use one transaction.
pub fn apply(conn: &mut Connection, request: &PersistRequest) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    apply_in(&tx, request)?;
    tx.commit()
}

fn apply_in(tx: &Transaction<'_>, request: &PersistRequest) -> rusqlite::Result<()> {
    let at = now();
    match request {
        PersistRequest::Workspace(w) => {
            tx.execute(
                "INSERT INTO workspace (id, path, key, fs_identity, kind, git_root, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (id) DO UPDATE SET path = ?2, key = ?3, fs_identity = ?4, git_root = ?6",
                params![
                    w.id.hex(),
                    w.path,
                    codec::key(&w.key),
                    w.identity,
                    match w.kind {
                        WorkspaceKind::Project => "project",
                        WorkspaceKind::Chat => "chat",
                    },
                    w.git_root,
                    at
                ],
            )?;
        }
        PersistRequest::Project(p) => {
            tx.execute(
                "INSERT INTO project (id, name, workspace_id, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET name = ?2",
                params![p.id.hex(), p.name, p.workspace.hex(), at],
            )?;
        }
        PersistRequest::Chat(t) => {
            tx.execute(
                "INSERT INTO task (id, project_id, workspace_id, provider, title, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    t.id.hex(),
                    t.project.map(ProjectId::hex),
                    t.workspace.hex(),
                    codec::provider(t.provider),
                    t.title,
                    t.created
                ],
            )?;
        }
        PersistRequest::Title { task, title } => {
            tx.execute("UPDATE task SET title = ?2, updated_at = ?3 WHERE id = ?1", params![task.hex(), title, at])?;
        }
        PersistRequest::Session { task, session } => {
            tx.execute(
                "INSERT INTO provider_session (id, task_id, thread_id, latest_turn_id, updated_at)
                 VALUES (?1, ?1, ?2, ?3, ?4)
                 ON CONFLICT (task_id) DO UPDATE SET thread_id = ?2, latest_turn_id = ?3, updated_at = ?4",
                params![task.hex(), session.thread, session.latest_turn, at],
            )?;
        }
        PersistRequest::RecordDelivery { task, run, message, state, draft_revision, body, settings, item } => {
            let attempt: i64 = tx.query_row(
                "SELECT COALESCE(MAX(attempt), 0) + 1 FROM run WHERE task_id = ?1",
                params![task.hex()],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO run (id, task_id, attempt, state, settings, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'preparing', ?4, ?5, ?5)",
                params![run.hex(), task.hex(), attempt, codec::settings(settings), at],
            )?;
            tx.execute(
                "INSERT INTO message_delivery (id, task_id, run_id, draft_revision, body, state, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    message.hex(),
                    task.hex(),
                    run.hex(),
                    *draft_revision as i64,
                    body,
                    codec::delivery(*state),
                    at
                ],
            )?;
            upsert_item(tx, item, None)?;
            // The draft that was sent is cleared; anything typed since is newer.
            tx.execute(
                "UPDATE draft SET text = '', saved_at = ?3 WHERE task_id = ?1 AND revision <= ?2",
                params![task.hex(), *draft_revision as i64, at],
            )?;
            tx.execute("UPDATE task SET updated_at = ?2 WHERE id = ?1", params![task.hex(), at])?;
        }
        PersistRequest::MarkAboutToSend { message } => {
            let changed = tx.execute(
                "UPDATE message_delivery SET state = 'about_to_send', updated_at = ?2 WHERE id = ?1 AND state = 'queued'",
                params![message.hex(), at],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
        }
        PersistRequest::DeliveryState { message, state } => {
            tx.execute(
                "UPDATE message_delivery SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![message.hex(), codec::delivery(*state), at],
            )?;
        }
        PersistRequest::RunState { run, state } => {
            tx.execute(
                "UPDATE run SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![run.hex(), codec::run_state(*state), at],
            )?;
        }
        PersistRequest::RunTurn { run, turn } => {
            tx.execute("UPDATE run SET turn_id = ?2, updated_at = ?3 WHERE id = ?1", params![run.hex(), turn, at])?;
        }
        PersistRequest::RunDismissed { run } => {
            tx.execute("UPDATE run SET dismissed = 1, updated_at = ?2 WHERE id = ?1", params![run.hex(), at])?;
        }
        PersistRequest::Item { item, provider_item } => upsert_item(tx, item, provider_item.as_deref())?,
        PersistRequest::ImportItems { task, run, items, before } => {
            let mut slots = import_slots(tx, *task, *before, items.len())?;
            for imported in items {
                import_item(tx, *task, *run, imported, &mut slots)?;
            }
        }
        PersistRequest::Draft { task, revision, text } => {
            tx.execute(
                "INSERT INTO draft (task_id, text, revision, saved_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (task_id) DO UPDATE SET text = ?2, revision = ?3, saved_at = ?4
                 WHERE excluded.revision >= draft.revision",
                params![task.hex(), text, *revision as i64, at],
            )?;
        }
        PersistRequest::Decision { decision } => insert_decision(tx, decision, at)?,
        PersistRequest::DecisionState { decision, state } => {
            tx.execute(
                "UPDATE pending_decision SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![decision.hex(), codec::decision_state(*state), at],
            )?;
        }
    }
    Ok(())
}

fn insert_decision(tx: &Transaction<'_>, d: &DecisionView, at: i64) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO pending_decision (id, run_id, connection_generation, kind, allowed, state, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            d.id.hex(),
            d.run.hex(),
            d.generation as i64,
            codec::decision_kind(&d.kind),
            codec::allowed(&d.kind),
            codec::decision_state(d.state),
            at
        ],
    )?;
    Ok(())
}

fn next_seq(tx: &Transaction<'_>, task: TaskId) -> rusqlite::Result<f64> {
    tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM transcript_item WHERE task_id = ?1", params![task.hex()], |r| {
        r.get(0)
    })
}

fn upsert_item(tx: &Transaction<'_>, item: &TranscriptItem, provider_item: Option<&str>) -> rusqlite::Result<()> {
    let (kind, provider) = match item.kind {
        ItemKind::UserMessage => ("user", None),
        ItemKind::AgentMessage { provider } => ("agent", Some(codec::provider(provider))),
    };
    let exists: bool = tx
        .query_row("SELECT 1 FROM transcript_item WHERE id = ?1", params![item.id.hex()], |_| Ok(()))
        .optional()?
        .is_some();
    if exists {
        // Never move an item backwards: a late batch must not undo a completion.
        tx.execute(
            "UPDATE transcript_item SET text = ?2, meta = ?3, completed = ?4, revision = ?5
             WHERE id = ?1 AND revision <= ?5",
            params![item.id.hex(), item.text, item.meta, item.completed as i64, item.revision as i64],
        )?;
    } else {
        let seq = next_seq(tx, item.task)?;
        tx.execute(
            "INSERT INTO transcript_item (id, task_id, run_id, seq, provider_item_id, kind, provider, text, meta, completed, revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                item.id.hex(),
                item.task.hex(),
                item.run.map(RunId::hex),
                seq,
                provider_item,
                kind,
                provider,
                item.text,
                item.meta,
                item.completed as i64,
                item.revision as i64
            ],
        )?;
    }
    Ok(())
}

/// Display positions for `count` new items: evenly spaced just before
/// `before`, or after the last item.
fn import_slots(
    tx: &Transaction<'_>,
    task: TaskId,
    before: Option<ItemId>,
    count: usize,
) -> rusqlite::Result<std::vec::IntoIter<f64>> {
    let anchor: Option<f64> = match before {
        Some(item) => tx
            .query_row("SELECT seq FROM transcript_item WHERE id = ?1", params![item.hex()], |r| r.get(0))
            .optional()?,
        None => None,
    };
    let slots: Vec<f64> = match anchor {
        Some(hi) => {
            let lo: f64 = tx.query_row(
                "SELECT COALESCE(MAX(seq), ?2 - 1) FROM transcript_item WHERE task_id = ?1 AND seq < ?2",
                params![task.hex(), hi],
                |r| r.get(0),
            )?;
            (1..=count).map(|i| lo + (hi - lo) * i as f64 / (count + 1) as f64).collect()
        }
        None => {
            let start = next_seq(tx, task)?;
            (0..count).map(|i| start + i as f64).collect()
        }
    };
    Ok(slots.into_iter())
}

fn import_item(
    tx: &Transaction<'_>,
    task: TaskId,
    run: Option<RunId>,
    item: &ImportedItem,
    slots: &mut std::vec::IntoIter<f64>,
) -> rusqlite::Result<()> {
    let provider: String =
        tx.query_row("SELECT provider FROM task WHERE id = ?1", params![task.hex()], |r| r.get(0))?;
    let updated = tx.execute(
        "UPDATE transcript_item SET text = ?3, completed = 1, revision = revision + 1
         WHERE task_id = ?1 AND provider_item_id = ?2",
        params![task.hex(), item.provider_item, item.text],
    )?;
    if updated > 0 {
        return Ok(());
    }
    let seq = match slots.next() {
        Some(seq) => seq,
        None => next_seq(tx, task)?,
    };
    let new_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    tx.execute(
        "INSERT INTO transcript_item (id, task_id, run_id, seq, provider_item_id, kind, provider, text, completed, revision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1)",
        params![
            new_id,
            task.hex(),
            run.map(RunId::hex),
            seq,
            item.provider_item,
            if item.user { "user" } else { "agent" },
            if item.user { None } else { Some(provider) },
            item.text
        ],
    )?;
    Ok(())
}
