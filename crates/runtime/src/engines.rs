//! Engine discovery, versions and reverting (specification sections 9a and 15).
//!
//! Bukno runs whatever engine version is installed. It reads the version at
//! every process start, records the last version that completed a run, and
//! offers a one-click switch back to it when a newer one stops working. It
//! never changes an engine version without a click.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bukno_platform::discovery::{self, Found};
use bukno_providers::codex::Launch;
use bukno_providers::codex::protocol::Account;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Versions that passed the acceptance flow. Information only, never a gate.
const TESTED: &str = include_str!("../../../protocol/tested-engines.json");

/// Overrides the Codex executable for development and end-to-end runs.
pub const CODEX_PATH_ENV: &str = "BUKNO_CODEX_PATH";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Working {
    pub version: String,
    pub path: PathBuf,
    pub sha256: String,
}

/// What the setup screen and settings show for an engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineView {
    pub state: EngineState,
    pub path: Option<String>,
    pub version: Option<String>,
    pub tested: bool,
    pub account: Option<String>,
    /// A sentence to show under the engine, such as why it is not working.
    pub detail: Option<String>,
    pub last_working: Option<String>,
    /// Bukno stays on this executable until Try latest version.
    pub pinned: Option<String>,
    pub revert: Option<Revert>,
    /// The model Bukno will use when the chat does not choose one, and why.
    pub default_model: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineState {
    /// Found, not started yet.
    Found,
    NotFound,
    Starting,
    /// Started and signed in. Ready to try; a real task proves it.
    Ready,
    SignedOut,
    /// The version that is installed fails with Bukno.
    NotWorking,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Revert {
    /// The previous executable is still on disk.
    UseFile { path: String, version: String },
    /// The engine's own install command, run only on a click.
    Command { program: String, args: Vec<String>, shown: String, changes_terminal: bool },
    /// Nothing Bukno can run; the user can.
    Manual { shown: String },
}

#[derive(Default)]
struct Records {
    last_working: Option<Working>,
    pinned: Option<PathBuf>,
    chosen: Option<PathBuf>,
}

/// Shared between the coordinator (which shows status) and the adapter
/// (which asks what to launch).
#[derive(Clone)]
pub struct Engines {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    file: PathBuf,
    records: Records,
    current: Option<(Found, String)>,
    state: EngineState,
    detail: Option<String>,
    account: Option<String>,
    default_model: Option<String>,
    note: Option<String>,
}

impl Engines {
    pub fn load(state_dir: &Path) -> Self {
        let file = state_dir.join("engines.json");
        let records = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("codex").cloned())
            .map(|codex| Records {
                last_working: codex.get("lastWorking").and_then(|w| {
                    Some(Working {
                        version: w.get("version")?.as_str()?.to_owned(),
                        path: PathBuf::from(w.get("path")?.as_str()?),
                        sha256: w.get("sha256")?.as_str()?.to_owned(),
                    })
                }),
                pinned: codex.get("pinned").and_then(Value::as_str).map(PathBuf::from),
                chosen: codex.get("chosen").and_then(Value::as_str).map(PathBuf::from),
            })
            .unwrap_or_default();
        let engines = Self {
            inner: Arc::new(Mutex::new(Inner {
                file,
                records,
                current: None,
                state: EngineState::Found,
                detail: None,
                account: None,
                default_model: None,
                note: None,
            })),
        };
        engines.discover();
        engines
    }

    /// Find the engine and read its version, without starting it.
    pub fn discover(&self) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        match locate(&inner.records) {
            Ok((found, version)) => {
                inner.current = Some((found, version));
                if matches!(inner.state, EngineState::NotFound) {
                    inner.state = EngineState::Found;
                    inner.detail = None;
                }
            }
            Err(detail) => {
                inner.current = None;
                inner.state = EngineState::NotFound;
                inner.detail = Some(detail);
            }
        }
        inner.view()
    }

    /// What the adapter launches. Called at every engine start.
    pub fn launch(&self) -> Result<Launch, String> {
        let mut inner = self.inner.lock().expect("engines");
        let (found, version) = locate(&inner.records)?;
        inner.current = Some((found.clone(), version.clone()));
        inner.state = EngineState::Starting;
        let mut env = vec![("PATH".to_owned(), discovery::engine_path(&found))];
        if let Some(package) = &found.npm_package {
            // What the npm wrapper would have set before starting the binary.
            env.push(("CODEX_MANAGED_PACKAGE_ROOT".into(), package.clone().into_os_string()));
            env.push(("CODEX_MANAGED_BY_NPM".into(), "1".into()));
        }
        Ok(Launch { executable: found.native, args: vec!["app-server".into()], env, version })
    }

    pub fn view(&self) -> EngineView {
        self.inner.lock().expect("engines").view()
    }

    pub fn ready(&self, account: &Account, default_model: Option<String>, note: Option<String>) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        inner.state = EngineState::Ready;
        inner.detail = None;
        inner.account = Some(match account {
            Account::ChatGpt { plan } => format!("Signed in with ChatGPT ({plan})"),
            Account::ApiKey => "Using an API key".into(),
            Account::Other(kind) => format!("Signed in ({kind})"),
            Account::SignedOut => "Not signed in".into(),
        });
        inner.default_model = default_model;
        inner.note = note;
        inner.view()
    }

    pub fn failed(&self, reason: &str) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        inner.state = if reason.contains("not signed in") { EngineState::SignedOut } else { EngineState::NotWorking };
        inner.detail = Some(reason.to_owned());
        inner.view()
    }

    pub fn stopped(&self) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        if matches!(inner.state, EngineState::Ready | EngineState::Starting) {
            inner.state = EngineState::Found;
        }
        inner.view()
    }

    /// A run completed normally: this engine is the last working version.
    pub fn worked(&self) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        if let Some((found, version)) = inner.current.clone()
            && let Some(sha256) = hash(&found.native)
        {
            let working = Working { version, path: found.native, sha256 };
            if inner.records.last_working.as_ref() != Some(&working) {
                inner.records.last_working = Some(working);
                inner.save();
            }
        }
        inner.view()
    }

    /// Use the last working version, or the route to get it back.
    pub fn use_previous(&self) -> Result<EngineView, String> {
        let mut inner = self.inner.lock().expect("engines");
        let working = inner.records.last_working.clone().ok_or("No earlier version has worked with Bukno yet.")?;
        if hash(&working.path).as_deref() == Some(working.sha256.as_str()) {
            inner.records.pinned = Some(working.path);
            inner.state = EngineState::Found;
            inner.detail = None;
            inner.save();
            return Ok(inner.view());
        }
        Err(format!("Codex {} is no longer on disk. Use the install command to get it back.", working.version))
    }

    /// Clear the pin and go back to whatever is installed.
    pub fn try_latest(&self) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        inner.records.pinned = None;
        inner.state = EngineState::Found;
        inner.detail = None;
        inner.save();
        drop(inner);
        self.discover()
    }

    /// The user picked an executable by hand.
    pub fn choose(&self, path: PathBuf) -> EngineView {
        let mut inner = self.inner.lock().expect("engines");
        inner.records.chosen = Some(path);
        inner.records.pinned = None;
        inner.state = EngineState::Found;
        inner.save();
        drop(inner);
        self.discover()
    }
}

impl Inner {
    fn view(&self) -> EngineView {
        let tested_versions: Vec<String> = serde_json::from_str::<Value>(TESTED)
            .ok()
            .and_then(|v| v.get("codex").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.get("version").and_then(Value::as_str).map(str::to_owned))
            .collect();
        let version = self.current.as_ref().map(|(_, v)| v.clone());
        let last = self.records.last_working.clone();
        let revert = match (&self.state, &last, &version) {
            (EngineState::NotWorking, Some(working), Some(current)) if working.version != *current => {
                Some(revert_route(working, self.current.as_ref().map(|(f, _)| f)))
            }
            _ => None,
        };
        EngineView {
            state: self.state.clone(),
            path: self.current.as_ref().map(|(f, _)| f.path.display().to_string()),
            tested: version.as_ref().is_some_and(|v| tested_versions.contains(v)),
            version,
            account: self.account.clone(),
            detail: self.detail.clone(),
            last_working: last.map(|w| w.version),
            pinned: self.records.pinned.as_ref().map(|p| p.display().to_string()),
            revert,
            default_model: self.default_model.clone(),
            note: self.note.clone(),
        }
    }

    fn save(&self) {
        let records = &self.records;
        let value = json!({
            "codex": {
                "lastWorking": records.last_working.as_ref().map(|w| json!({
                    "version": w.version,
                    "path": w.path.display().to_string(),
                    "sha256": w.sha256,
                })),
                "pinned": records.pinned.as_ref().map(|p| p.display().to_string()),
                "chosen": records.chosen.as_ref().map(|p| p.display().to_string()),
            }
        });
        let tmp = self.file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&value).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, &self.file);
        }
    }
}

/// Order: the pinned last working version, the user's choice, the
/// development override, then the usual install locations.
fn locate(records: &Records) -> Result<(Found, String), String> {
    let env = std::env::var_os(CODEX_PATH_ENV).map(PathBuf::from);
    let explicit = records.pinned.clone().or_else(|| records.chosen.clone()).or(env);
    let found = match explicit {
        Some(path) => discovery::inspect(&path).ok_or_else(|| {
            format!("Codex was not found at {}. Choose another file, or install Codex.", path.display())
        })?,
        None => discovery::find_engine("codex").ok_or_else(|| {
            "Codex is not installed. Install it with `npm install -g @openai/codex`, then choose Check again."
                .to_owned()
        })?,
    };
    let version = read_version(&found.native)?;
    Ok((found, version))
}

fn read_version(native: &Path) -> Result<String, String> {
    let output = std::process::Command::new(native)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Codex at {} could not run: {e}", native.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    // "codex-cli 0.158.0"
    text.split_whitespace()
        .last()
        .map(str::to_owned)
        .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("Codex at {} did not report a version.", native.display()))
}

fn hash(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

fn revert_route(working: &Working, current: Option<&Found>) -> Revert {
    if hash(&working.path).as_deref() == Some(working.sha256.as_str()) {
        return Revert::UseFile { path: working.path.display().to_string(), version: working.version.clone() };
    }
    let package = format!("@openai/codex@{}", working.version);
    // npm installs keep no old binaries; reinstalling that exact version is the route.
    if let Some(found) = current.filter(|f| f.npm_package.is_some())
        && let Some(npm) = found.path.parent().map(|d| d.join("npm")).filter(|p| p.exists())
    {
        return Revert::Command {
            program: npm.display().to_string(),
            args: vec!["install".into(), "-g".into(), package.clone()],
            shown: format!("npm install -g {package}"),
            changes_terminal: true,
        };
    }
    Revert::Manual { shown: format!("npm install -g {package}") }
}
