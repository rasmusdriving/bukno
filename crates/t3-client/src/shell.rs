//! The project and chat list of one environment, kept current from
//! `orchestration.subscribeShell`.
//!
//! Sequence numbers come from T3's one global event log, so they jump within
//! this stream; a jump is not a lost event. Items at or below the last
//! applied number are duplicates from a resume and are dropped.

use std::collections::BTreeMap;

use crate::model::{ProjectShell, ShellItem, ThreadShell};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamCounts {
    pub snapshots: u64,
    pub applied: u64,
    pub duplicates: u64,
    pub unknown: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ShellState {
    pub projects: BTreeMap<String, ProjectShell>,
    /// Active (not archived) threads by ID.
    pub threads: BTreeMap<String, ThreadShell>,
    /// The last sequence applied; the `afterSequence` for a resume.
    pub last_sequence: Option<u64>,
    /// True after the server's catch-up marker on the current connection.
    pub synchronized: bool,
    pub counts: StreamCounts,
}

impl ShellState {
    /// A new connection started: the list is no longer known to be current.
    pub fn connection_started(&mut self) {
        self.synchronized = false;
    }

    /// Returns true when what the sidebar shows changed.
    pub fn apply(&mut self, item: ShellItem) -> bool {
        let sequence = match &item {
            ShellItem::ProjectUpdated { sequence, .. }
            | ShellItem::ProjectRemoved { sequence, .. }
            | ShellItem::ThreadUpdated { sequence, .. }
            | ShellItem::ThreadRemoved { sequence, .. } => Some(*sequence),
            _ => None,
        };
        if let Some(sequence) = sequence {
            if self.last_sequence.is_some_and(|last| sequence <= last) {
                self.counts.duplicates += 1;
                return false;
            }
            self.last_sequence = Some(sequence);
            self.counts.applied += 1;
        }
        match item {
            ShellItem::Synchronized => {
                self.synchronized = true;
                true
            }
            ShellItem::Snapshot(snapshot) => {
                self.counts.snapshots += 1;
                self.projects = snapshot.projects.into_iter().map(|p| (p.id.clone(), p)).collect();
                self.threads = snapshot
                    .threads
                    .into_iter()
                    .filter(|t| t.deleted_at.is_none() && t.archived_at.is_none())
                    .map(|t| (t.id.clone(), t))
                    .collect();
                self.last_sequence = Some(snapshot.snapshot_sequence);
                true
            }
            ShellItem::ProjectRefresh(snapshot) => {
                // Metadata only. Never replaces threads or moves the cursor.
                for project in snapshot.projects {
                    self.projects.insert(project.id.clone(), project);
                }
                true
            }
            ShellItem::ProjectUpdated { project, .. } => {
                self.projects.insert(project.id.clone(), project);
                true
            }
            ShellItem::ProjectRemoved { project_id, .. } => {
                self.projects.remove(&project_id);
                true
            }
            ShellItem::ThreadUpdated { archived, thread, .. } => {
                if archived || thread.archived_at.is_some() || thread.deleted_at.is_some() {
                    self.threads.remove(&thread.id).is_some()
                } else {
                    self.threads.insert(thread.id.clone(), *thread);
                    true
                }
            }
            ShellItem::ThreadRemoved { archived, thread_id, .. } => {
                // An archived thread's removal from the archive list does not
                // affect the active list.
                !archived && self.threads.remove(&thread_id).is_some()
            }
            ShellItem::Unknown { .. } => {
                self.counts.unknown += 1;
                false
            }
        }
    }
}
