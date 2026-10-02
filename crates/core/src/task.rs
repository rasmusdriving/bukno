//! Chats, projects, workspaces and the settings a run is sent with
//! (specification sections 6, 11 and 12).

use crate::ids::{ProjectId, TaskId, WorkspaceId};
use crate::message::Provider;

/// A folder Bukno runs work in. The runtime resolves its identity (symlinks,
/// Git root, filesystem ID) before core sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub id: WorkspaceId,
    /// Absolute path as shown to the user.
    pub path: String,
    /// Path components used to detect overlap, already normalized for the
    /// platform's case rules. Two workspaces overlap when one is a prefix of
    /// the other.
    pub key: Vec<String>,
    pub kind: WorkspaceKind,
    /// The Git checkout root, when the folder is in one and Git works.
    pub git_root: Option<String>,
    /// "device:inode" of the folder, so two paths to one folder are one workspace.
    pub identity: Option<String>,
    /// False when the folder is missing, for example on an unplugged drive.
    /// Checked at launch; not stored.
    pub available: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceKind {
    /// A project's existing folder, wherever it is.
    Project,
    /// A projectless chat's own folder in the work folder.
    Chat,
}

impl WorkspaceInfo {
    /// The same folder, or one inside the other.
    pub fn overlaps(&self, other: &WorkspaceInfo) -> bool {
        let n = self.key.len().min(other.key.len());
        self.key[..n] == other.key[..n]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub name: String,
    pub workspace: WorkspaceId,
}

/// The provider's own conversation identity for a chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    /// The Codex thread ID or Claude session ID. Resume can issue a new one.
    pub thread: String,
    /// The last provider turn Bukno saw, to detect work continued outside Bukno.
    pub latest_turn: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskInfo {
    pub id: TaskId,
    /// Empty until the first message names it.
    pub title: String,
    pub provider: Provider,
    pub project: Option<ProjectId>,
    pub workspace: WorkspaceId,
    pub session: Option<SessionInfo>,
    /// Creation order, newest highest, for the sidebar.
    pub created: i64,
}

/// Whether a permission preset can change files (section 11). Runs that can
/// never write take no writer lease (section 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writes {
    Never,
    AfterApproval,
    Freely,
}

/// What a run is sent with, captured when the user presses Send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSettings {
    /// The provider preset's stable ID; the adapter turns it into engine settings.
    pub preset: String,
    pub writes: Writes,
    /// None lets the adapter use the engine's own default.
    pub model: Option<String>,
    pub effort: Option<String>,
}
