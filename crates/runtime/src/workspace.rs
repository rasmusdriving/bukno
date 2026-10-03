//! Workspace identity and folders (specification sections 5 and 12).

use std::path::{Path, PathBuf};

use bukno_core::ids::{TaskId, WorkspaceId};
use bukno_core::task::{WorkspaceInfo, WorkspaceKind};

/// Resolve a folder into a workspace: symlinks resolved, normalized to its
/// Git checkout root when it is in one, with its filesystem identity.
pub fn resolve(folder: &Path, kind: WorkspaceKind, git: Option<&Path>) -> std::io::Result<WorkspaceInfo> {
    let canonical = std::fs::canonicalize(folder)?;
    if !canonical.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a folder"));
    }
    let git_root = git.and_then(|git| git_root(git, &canonical));
    // A Git checkout is one workspace however deep the chosen folder is.
    let root = git_root.clone().unwrap_or_else(|| canonical.clone());
    Ok(WorkspaceInfo {
        id: WorkspaceId(uuid::Uuid::new_v4().as_u128()),
        path: canonical.display().to_string(),
        key: key(&root),
        kind,
        git_root: git_root.map(|r| r.display().to_string()),
        identity: identity(&root),
        available: true,
    })
}

/// Components used to detect the same or nested folders. macOS and Windows
/// volumes are usually case-insensitive, so compare without case there.
pub fn key(path: &Path) -> Vec<String> {
    path.components()
        .map(|c| {
            let text = c.as_os_str().to_string_lossy().into_owned();
            if cfg!(any(target_os = "macos", target_os = "windows")) { text.to_lowercase() } else { text }
        })
        .collect()
}

fn identity(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| format!("{}:{}", m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn git_root(git: &Path, folder: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new(git)
        .arg("-C")
        .arg(folder)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    std::fs::canonicalize(text.trim()).ok()
}

/// A projectless chat's own folder: `<work folder>/chats/<task>/workspace`.
/// Never initialized as a Git repository.
pub fn chat_folder(work: &Path, task: TaskId) -> PathBuf {
    work.join("chats").join(task.hex()).join("workspace")
}

/// Mark workspaces whose folder is missing (for example on an unplugged drive).
pub fn check_available(workspaces: &mut [WorkspaceInfo]) {
    for workspace in workspaces {
        workspace.available = Path::new(&workspace.path).is_dir();
    }
}
