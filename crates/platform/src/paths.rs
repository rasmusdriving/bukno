//! Where Bukno keeps its state and the user's work (specification section 5).

use std::env;
use std::path::PathBuf;

/// Points app state at an isolated folder for development and E2E runs.
pub const STATE_DIR_ENV: &str = "BUKNO_STATE_DIR";
/// Points the work folder at an isolated folder for development and E2E runs.
pub const WORK_DIR_ENV: &str = "BUKNO_WORK_DIR";

#[derive(Clone, Debug)]
pub struct AppPaths {
    /// Preferences, database and diagnostics. Always on the internal drive.
    pub state_dir: PathBuf,
    /// Projectless chats, attachments and outputs. Chosen during onboarding,
    /// so it can be unknown before then.
    pub work_dir: Option<PathBuf>,
    /// True when either location comes from an override variable.
    pub overridden: bool,
}

#[derive(Debug)]
pub enum PathError {
    /// The platform's per-user folder could not be found.
    NoUserFolder(&'static str),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoUserFolder(var) => write!(f, "{var} is not set, so the app state folder is unknown"),
        }
    }
}

impl std::error::Error for PathError {}

/// Resolve the locations for a normal launch, honouring the override variables.
pub fn resolve() -> Result<AppPaths, PathError> {
    let state_override = env::var_os(STATE_DIR_ENV).map(PathBuf::from);
    let work_override = env::var_os(WORK_DIR_ENV).map(PathBuf::from);
    let overridden = state_override.is_some() || work_override.is_some();
    let state_dir = match state_override {
        Some(dir) => dir,
        None => default_state_dir()?,
    };
    Ok(AppPaths { state_dir, work_dir: work_override, overridden })
}

#[cfg(target_os = "macos")]
fn default_state_dir() -> Result<PathBuf, PathError> {
    let home = env::var_os("HOME").ok_or(PathError::NoUserFolder("HOME"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/Bukno"))
}

#[cfg(target_os = "windows")]
fn default_state_dir() -> Result<PathBuf, PathError> {
    let local = env::var_os("LOCALAPPDATA").ok_or(PathError::NoUserFolder("LOCALAPPDATA"))?;
    Ok(PathBuf::from(local).join("Bukno"))
}

#[cfg(target_os = "linux")]
fn default_state_dir() -> Result<PathBuf, PathError> {
    if let Some(data) = env::var_os("XDG_DATA_HOME").map(PathBuf::from)
        && data.is_absolute()
    {
        return Ok(data.join("bukno"));
    }
    let home = env::var_os("HOME").ok_or(PathError::NoUserFolder("HOME"))?;
    Ok(PathBuf::from(home).join(".local/share/bukno"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn default_state_dir() -> Result<PathBuf, PathError> {
    Err(PathError::NoUserFolder("a supported platform"))
}
