//! The open chat's timeline, kept current from `orchestration.subscribeThread`
//! and older pages from the history endpoint.
//!
//! Follows T3's own client (`client-runtime/src/state/orchestrationV2Projection.ts`):
//! rows are the projection's `visibleTurnItems`; a `turn-item.updated` event
//! replaces its row or inserts it by `(ordinal, id)` among local rows; while
//! only a recent window is loaded, events for rows older than the window are
//! skipped because the history page will bring them.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::{HistoryPage, ProjectedItem, RunAttempt, ThreadEvent, ThreadItem, TurnItem, TurnKind};

/// Revisions come from one counter for the whole process, so a chat that is
/// reloaded or reopened never repeats a revision a view has already seen.
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}
use crate::shell::StreamCounts;

/// One timeline row. `revision` changes whenever the row's item changes, so
/// a view can skip rows it already drew.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub source_thread_id: String,
    pub source_item_id: String,
    pub local: bool,
    pub item: TurnItem,
    pub revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ThreadState {
    pub thread_id: String,
    /// Identifies this load of the chat. Replies to requests made for an
    /// earlier load (such as a history page) are discarded.
    pub incarnation: u64,
    pub title: String,
    pub rows: Vec<Arc<Row>>,
    runs: HashMap<String, String>,
    attempts: HashMap<String, RunAttempt>,
    pub last_sequence: Option<u64>,
    /// True after the catch-up marker on the current subscription.
    pub synchronized: bool,
    pub history_cursor: Option<String>,
    pub has_more_history: bool,
    latest_local_turn_ordinal: Option<u64>,
    /// The thread was deleted in T3 while open.
    pub removed: bool,
    pub counts: StreamCounts,
    pub unknown_event_types: BTreeSet<String>,
    /// Changes on every change to what is shown; unique across reloads.
    pub revision: u64,
}

impl ThreadState {
    pub fn new(thread_id: &str) -> Self {
        Self {
            thread_id: thread_id.to_owned(),
            incarnation: next_revision(),
            revision: next_revision(),
            ..Self::default()
        }
    }

    /// Whether any data has arrived yet.
    pub fn loaded(&self) -> bool {
        self.last_sequence.is_some()
    }

    pub fn subscription_started(&mut self) {
        self.synchronized = false;
    }

    fn row(&mut self, item: ProjectedItem) -> Arc<Row> {
        Arc::new(Row {
            local: item.is_local(),
            source_thread_id: item.source_thread_id,
            source_item_id: item.source_item_id,
            item: item.item,
            revision: next_revision(),
        })
    }

    fn changed(&mut self) -> bool {
        self.revision = next_revision();
        true
    }

    /// Returns true when what the transcript shows changed.
    pub fn apply(&mut self, item: ThreadItem) -> bool {
        match item {
            ThreadItem::Synchronized => {
                self.synchronized = true;
                self.changed()
            }
            ThreadItem::Snapshot(snapshot) => {
                self.counts.snapshots += 1;
                self.title = snapshot.title;
                self.runs = snapshot.runs.into_iter().map(|r| (r.id, r.status)).collect();
                self.attempts = snapshot.attempts.into_iter().map(|a| (a.id.clone(), a)).collect();
                self.rows = snapshot.items.into_iter().map(|i| self.row(i)).collect();
                self.history_cursor = snapshot.history_cursor;
                self.has_more_history = snapshot.has_more_history;
                self.latest_local_turn_ordinal = snapshot.latest_local_turn_ordinal;
                self.last_sequence = Some(snapshot.snapshot_sequence);
                self.changed()
            }
            ThreadItem::UnknownEvent { sequence, event_type } => {
                // Skip it but move the cursor, as T3's own client does.
                if self.fresh(sequence) {
                    self.counts.unknown += 1;
                    self.unknown_event_types.insert(event_type);
                }
                false
            }
            ThreadItem::Unknown { .. } => {
                self.counts.unknown += 1;
                false
            }
            ThreadItem::Event { sequence, thread_id, event } => {
                if !self.fresh(sequence) || thread_id != self.thread_id {
                    return false;
                }
                match *event {
                    ThreadEvent::TurnItem(item) => self.upsert(item),
                    ThreadEvent::Run(run) => {
                        // Rows can hide, and the working state can change.
                        let working = self.working();
                        let affects_rows = matches!(run.status.as_str(), "rolled_back" | "cancelled");
                        self.runs.insert(run.id, run.status);
                        (affects_rows || working != self.working()) && self.changed()
                    }
                    ThreadEvent::Attempt(attempt) => {
                        let was = self.attempts.get(&attempt.id).map(|a| a.status == "superseded");
                        let now = attempt.status == "superseded";
                        self.attempts.insert(attempt.id.clone(), attempt);
                        (was != Some(now) && (now || was == Some(true))) && self.changed()
                    }
                    ThreadEvent::Thread { title, deleted } => {
                        if deleted {
                            self.removed = true;
                        }
                        if let Some(title) = title {
                            self.title = title;
                        }
                        self.changed()
                    }
                    ThreadEvent::Other { .. } => false,
                }
            }
        }
    }

    /// Record a sequence; false for duplicates.
    fn fresh(&mut self, sequence: u64) -> bool {
        if self.last_sequence.is_some_and(|last| sequence <= last) {
            self.counts.duplicates += 1;
            return false;
        }
        self.last_sequence = Some(sequence);
        self.counts.applied += 1;
        true
    }

    fn oldest_local_ordinal(&self) -> Option<u64> {
        self.rows.iter().filter(|r| r.local).map(|r| r.item.ordinal).min()
    }

    fn upsert(&mut self, item: TurnItem) -> bool {
        let index = self.rows.iter().position(|r| r.local && r.source_item_id == item.id);
        if index.is_none() && self.has_more_history {
            // A row older than the loaded window: its history page brings it.
            let older_than_snapshot = self.latest_local_turn_ordinal.is_some_and(|latest| item.ordinal <= latest);
            let older_than_window = self.oldest_local_ordinal().is_some_and(|oldest| item.ordinal < oldest);
            if older_than_snapshot || older_than_window {
                return false;
            }
        }
        if let Some(index) = index
            && self.rows[index].item == item
        {
            return false;
        }
        let projected = ProjectedItem {
            visibility: "local".into(),
            source_thread_id: item.thread_id.clone(),
            source_item_id: item.id.clone(),
            item,
        };
        let row = self.row(projected);
        if let Some(index) = index {
            if self.rows[index].item.ordinal == row.item.ordinal {
                self.rows[index] = row;
                return self.changed();
            }
            self.rows.remove(index);
        }
        let at = self
            .rows
            .iter()
            .position(|r| {
                r.local
                    && (r.item.ordinal > row.item.ordinal
                        || (r.item.ordinal == row.item.ordinal && r.item.id > row.item.id))
            })
            .unwrap_or(self.rows.len());
        self.rows.insert(at, row);
        self.changed()
    }

    /// Put an older history page in front of the loaded rows.
    pub fn merge_history(&mut self, page: HistoryPage) -> bool {
        let present: HashSet<(String, String)> =
            self.rows.iter().map(|r| (r.source_thread_id.clone(), r.source_item_id.clone())).collect();
        let older: Vec<Arc<Row>> =
            page.items.into_iter().filter(|i| !present.contains(&i.key())).map(|i| self.row(i)).collect();
        self.history_cursor = page.next_cursor;
        self.has_more_history = page.has_more_history && self.history_cursor.is_some();
        self.rows.splice(0..0, older);
        self.changed()
    }

    /// Rows to show, without those T3 hides (`shared/src/orchestrationV2Timeline.ts`):
    /// items of rolled-back runs, queued messages whose run was cancelled
    /// before it reached the provider, and the interrupt result of a
    /// superseded attempt when its run has no interrupt request.
    pub fn visible_rows(&self) -> Vec<Arc<Row>> {
        let superseded: HashSet<(&str, &str)> = self
            .attempts
            .values()
            .filter(|a| a.status == "superseded")
            .map(|a| (a.run_id.as_str(), a.root_node_id.as_str()))
            .collect();
        let interrupt_requests: HashSet<&str> = self
            .rows
            .iter()
            .filter(|r| r.item.type_name == "run_interrupt_request")
            .filter_map(|r| r.item.run_id.as_deref())
            .collect();
        self.rows
            .iter()
            .filter(|row| {
                let item = &row.item;
                let run = item.run_id.as_ref().and_then(|id| self.runs.get(id)).map(String::as_str);
                if run == Some("rolled_back") {
                    return false;
                }
                if matches!(item.kind, TurnKind::UserMessage { queued: true, .. }) && run == Some("cancelled") {
                    return false;
                }
                let superseded_interrupt = item.type_name == "run_interrupt_result"
                    && match (item.run_id.as_deref(), item.node_id.as_deref()) {
                        (Some(run), Some(node)) => {
                            superseded.contains(&(run, node)) && !interrupt_requests.contains(run)
                        }
                        _ => false,
                    };
                !superseded_interrupt
            })
            .cloned()
            .collect()
    }

    /// Whether a run is active, from the statuses seen so far.
    pub fn working(&self) -> bool {
        self.runs.values().any(|s| matches!(s.as_str(), "preparing" | "queued" | "starting" | "running" | "waiting"))
    }
}
