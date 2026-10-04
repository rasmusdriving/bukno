//! Local T3 discovery and setup. The CLI mints credentials; Bukno never reads
//! T3's auth database. Downloads are pinned, verified and installed privately.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use fs2::FileExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::process::Command;
use url::Url;

use crate::error::T3Error;
use crate::http::Http;
use crate::model::Descriptor;
use crate::pairing::{normalize_address, parse_pairing};
use crate::secret::Secret;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SetupStatus {
    #[default]
    Idle,
    Working(String),
    Missing,
    Ready {
        environment_id: String,
        label: String,
    },
    Failed(String),
}

impl SetupStatus {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Working(_))
    }
}

#[derive(Clone)]
pub struct LocalConfig {
    pub t3_home: PathBuf,
    pub managed_home: PathBuf,
    pub install_dir: PathBuf,
    pub launchers: Vec<Launcher>,
}

/// Installed CLI, or the CLI entry bundled with an installed desktop app.
#[derive(Clone, Debug)]
pub struct Launcher {
    pub program: PathBuf,
    pub entry: Option<PathBuf>,
}

impl Launcher {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(entry) = &self.entry {
            command.env("ELECTRON_RUN_AS_NODE", "1").arg(entry);
        }
        // Do not inherit a development server or worktree's routing.
        command.env_remove("VITE_DEV_SERVER_URL").env_remove("T3CODE_PORT").env_remove("T3CODE_HOST");
        command.stdin(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for CLI operations.
        command
    }
}

impl LocalConfig {
    pub fn for_app(state_dir: &Path) -> Self {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.to_owned());
        let t3_home = std::env::var_os("BUKNO_T3_HOME")
            .or_else(|| std::env::var_os("T3CODE_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".t3"));
        let install_dir = state_dir.join("t3-runtime").join(crate::pinned::SERVER_VERSION);
        let mut launchers = Vec::new();
        if let Some(program) = std::env::var_os("BUKNO_T3_CLI") {
            launchers.push(Launcher {
                program: program.into(),
                entry: std::env::var_os("BUKNO_T3_CLI_ENTRY").map(PathBuf::from),
            });
        } else {
            let exe = if cfg!(windows) { "t3.exe" } else { "t3" };
            for folder in [
                home.join(".local/bin"),
                home.join(".bun/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/opt/homebrew/bin"),
            ] {
                launchers.push(Launcher { program: folder.join(exe), entry: None });
            }
            for folder in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
                launchers.push(Launcher { program: folder.join(exe), entry: None });
            }
            #[cfg(target_os = "linux")]
            for folder in ["/opt/T3 Code", "/opt/T3 Code (Nightly)"] {
                launchers.push(Launcher {
                    program: PathBuf::from(folder).join("t3code"),
                    entry: Some(PathBuf::from(folder).join("resources/app.asar/apps/server/dist/bin.mjs")),
                });
            }
            #[cfg(target_os = "macos")]
            for root in [PathBuf::from("/Applications"), home.join("Applications")] {
                for name in ["T3 Code.app", "T3 Code (Nightly).app"] {
                    let contents = root.join(name).join("Contents");
                    for executable in ["T3 Code", "T3 Code (Nightly)", "t3code"] {
                        launchers.push(Launcher {
                            program: contents.join("MacOS").join(executable),
                            entry: Some(contents.join("Resources/app.asar/apps/server/dist/bin.mjs")),
                        });
                    }
                }
            }
            #[cfg(windows)]
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                for name in ["T3 Code", "T3 Code (Nightly)", "t3code", "t3code-nightly"] {
                    let root = PathBuf::from(&local).join("Programs").join(name);
                    for executable in ["T3 Code.exe", "T3 Code (Nightly).exe", "t3code.exe"] {
                        launchers.push(Launcher {
                            program: root.join(executable),
                            entry: Some(root.join("resources/app.asar/apps/server/dist/bin.mjs")),
                        });
                    }
                }
            }
        }
        launchers
            .push(Launcher { program: install_dir.join(if cfg!(windows) { "t3.exe" } else { "t3" }), entry: None });
        Self { t3_home, managed_home: state_dir.join("t3-server"), install_dir, launchers }
    }
}

pub struct LocalTarget {
    pub launcher: Launcher,
    pub home: PathBuf,
    pub base: Url,
    pub descriptor: Descriptor,
}

#[derive(Deserialize)]
struct RuntimeRecord {
    version: u32,
    pid: u32,
    origin: String,
}

pub(crate) fn install_tls_provider() {
    #[cfg(unix)]
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Use ordinary Windows drive/UNC paths when passing a folder to Node or an engine.
pub fn workspace_path(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

#[cfg(not(target_os = "linux"))]
fn process_started_after_record(pid: u32, modified: std::time::SystemTime) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().without_tasks(),
    );
    let Some(process) = system.process(pid) else { return false };
    // Unknown start times stay conservative. Allow for OS/file precision.
    let written = modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
    written.is_some_and(|written| process.start_time() > written.saturating_add(300))
}

#[cfg(target_os = "linux")]
fn runtime_process_reused(pid: u32, path: &Path, proof_dir: &Path, verified: bool, record_hash: String) -> bool {
    use std::os::unix::ffi::OsStrExt;

    #[derive(Deserialize, serde::Serialize)]
    struct Owner {
        pid: u32,
        boot: String,
        ticks: u64,
        record_hash: String,
    }

    // Bukno itself cannot be the T3 backend. This is also proof for a record
    // whose PID was reused before Bukno ever observed that server.
    if !verified && stale_frontend_pid(pid, path) {
        return true;
    }
    let Some(owner) = (|| {
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The process name can contain spaces and parentheses. Field 22 is
        // the start tick count since boot, independent of wall-clock changes.
        let ticks = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()?;
        Some(Owner { pid, boot: boot.trim().to_owned(), ticks, record_hash })
    })() else {
        return false;
    };
    let proof = proof_dir.join(format!("runtime-owner-{:x}.json", Sha256::digest(path.as_os_str().as_bytes())));
    if verified {
        // Keep observations in Bukno's folder, without changing T3's files.
        // A missing/unreadable observation stays conservative on the next run.
        if let Ok(bytes) = serde_json::to_vec(&owner) {
            // An interrupted write becomes unreadable and therefore blocks
            // replacement, without leaving another temporary file behind.
            let _ = std::fs::write(proof, bytes);
        }
        false
    } else {
        std::fs::read(proof).ok().and_then(|bytes| serde_json::from_slice::<Owner>(&bytes).ok()).is_some_and(
            |previous| {
                previous.pid == owner.pid
                    && previous.record_hash == owner.record_hash
                    && (previous.boot != owner.boot || previous.ticks != owner.ticks)
            },
        )
    }
}

#[cfg(not(target_os = "linux"))]
fn runtime_process_reused(pid: u32, path: &Path, _proof_dir: &Path, verified: bool, _record_hash: String) -> bool {
    !verified
        && (stale_frontend_pid(pid, path)
            || std::fs::metadata(path)
                .and_then(|m| m.modified())
                .is_ok_and(|modified| process_started_after_record(pid, modified)))
}

fn stale_frontend_pid(pid: u32, path: &Path) -> bool {
    pid == std::process::id()
        && std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(300))
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // Signal 0 only checks existence/access, without signalling the process.
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0;
            let alive = GetExitCodeProcess(handle, &mut code) != 0 && code == 259;
            CloseHandle(handle);
            alive
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// None means there is no live runtime. A live record that fails identity
/// verification is a block, never permission to start another writer.
async fn discover(home: &Path, proof_dir: &Path) -> Result<Option<(Url, Descriptor)>, String> {
    for variant in ["userdata", "dev"] {
        let path = home.join(variant).join("server-runtime.json");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                return Err(
                    "Bukno could not read T3's connection details. Check the folder permissions and try again.".into(),
                );
            }
        };
        let record: RuntimeRecord = serde_json::from_slice(&bytes)
            .map_err(|_| "T3's connection details could not be read. Open T3 Code and try again.".to_owned())?;
        if record.version != 1 {
            return Err(
                "This T3 installation uses different connection details. Open T3 Code or use advanced connection."
                    .into(),
            );
        }
        if !process_alive(record.pid) {
            continue;
        }
        let base = normalize_address(&record.origin).map_err(|e| e.user_message())?;
        // Permit loopback and addresses actually assigned to this computer.
        // Binding an ephemeral socket proves locality without network scanning.
        let local = match base.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback() || std::net::TcpListener::bind((ip, 0)).is_ok(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback() || std::net::TcpListener::bind((ip, 0)).is_ok(),
            _ => false,
        };
        if !local || !base.username().is_empty() || base.password().is_some() {
            return Err(
                "T3's address could not be verified on this computer. Use advanced connection to choose that server."
                    .into(),
            );
        }
        // A verified endpoint wins over timestamps. On Linux, epoch-based
        // process start times can move after NTP or resume, even when the
        // process has not changed. Only a clock-independent owner mismatch
        // can permit replacement of a live but unresponsive managed server.
        let descriptor = Http::new().descriptor(&base).await;
        let verified = descriptor.is_ok();
        let proof_dir = proof_dir.to_owned();
        let pid = record.pid;
        let record_hash = format!("{:x}", Sha256::digest(&bytes));
        let reused =
            tokio::task::spawn_blocking(move || runtime_process_reused(pid, &path, &proof_dir, verified, record_hash))
                .await
                .unwrap_or(false);
        match descriptor {
            Ok(descriptor) => return Ok(Some((base, descriptor))),
            Err(T3Error::Unreachable { .. }) if reused => continue,
            Err(error) => return Err(error.user_message()),
        }
    }
    Ok(None)
}

async fn output(mut command: Command, seconds: u64) -> Result<Vec<u8>, String> {
    command.kill_on_drop(true).stdout(Stdio::piped());
    let result = tokio::time::timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| "T3 took too long to respond. Try again.".to_owned())?
        .map_err(|_| "T3 could not start. Open T3 Code or try downloading it again.".to_owned())?;
    if !result.status.success() {
        return Err("T3 could not finish setup. Open T3 Code and try again, or use advanced connection.".into());
    }
    Ok(result.stdout)
}

/// Discover first. A managed server gets a separate database and a private,
/// dynamic loopback port. Starting it never opens another user's T3 database.
pub async fn prepare(
    config: &LocalConfig,
    install: bool,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Option<LocalTarget>, String> {
    // One cross-process setup lock also protects staging and server launch.
    std::fs::create_dir_all(&config.managed_home)
        .map_err(|_| "Bukno could not create its T3 folder. Check free space and folder permissions.".to_owned())?;
    let lock = Arc::new(
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(config.managed_home.join("bukno-setup.lock"))
            .map_err(|_| "Bukno could not open its T3 setup lock.".to_owned())?,
    );
    lock.try_lock_exclusive()
        .map_err(|_| "Another Bukno window is setting up T3. Wait for it to finish and try again.".to_owned())?;
    progress("Looking for T3 Code…");
    let managed =
        Launcher { program: config.install_dir.join(if cfg!(windows) { "t3.exe" } else { "t3" }), entry: None };
    let resume_managed = config.managed_home.join("userdata/statev2.sqlite").is_file();
    let mut launcher = if resume_managed && managed.program.is_file() {
        Some(managed)
    } else {
        config.launchers.iter().find(|l| l.program.is_file()).cloned()
    };
    // Probe even when the CLI is missing: do not launch a replacement for a
    // live server whose install has moved. The download supplies just the CLI.
    let existing = match discover(&config.t3_home, &config.managed_home).await? {
        Some(found) => Some((config.t3_home.clone(), found)),
        None => discover(&config.managed_home, &config.managed_home)
            .await?
            .map(|found| (config.managed_home.clone(), found)),
    };
    if launcher.is_none() && install {
        launcher = Some(download(config, progress, lock.clone()).await?);
    }
    let Some(launcher) = launcher else { return Ok(None) };
    if let Some((home, (base, descriptor))) = existing {
        return Ok(Some(LocalTarget { launcher, home, base, descriptor }));
    }
    progress("Starting T3 Code…");
    if launcher.entry.is_some() && !resume_managed {
        // The desktop app owns its server and its single-instance behavior.
        // Open it normally, without ELECTRON_RUN_AS_NODE, then rediscover it.
        let mut command = Command::new(&launcher.program);
        command
            .env_remove("ELECTRON_RUN_AS_NODE")
            .env("T3CODE_HOME", &config.t3_home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child =
            command.spawn().map_err(|_| "T3 Code could not open. Try again or use advanced connection.".to_owned())?;
        let ready = wait_for_server(&config.t3_home, &config.managed_home, &mut child, true).await;
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        let (base, descriptor) = ready?;
        return Ok(Some(LocalTarget { launcher, home: config.t3_home.clone(), base, descriptor }));
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Bukno could not reserve a local connection for T3.".to_owned())?;
    let port = listener.local_addr().map_err(|_| "Bukno could not read its local connection.".to_owned())?.port();
    drop(listener);
    let mut command = launcher.command();
    command
        .arg("serve")
        .arg("--base-dir")
        .arg(&config.managed_home)
        .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--no-browser"])
        .env("T3CODE_HOME", &config.managed_home)
        .current_dir(&config.managed_home)
        .stdout(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|_| "T3 could not start. Try downloading it again.".to_owned())?;
    match wait_for_server(&config.managed_home, &config.managed_home, &mut child, false).await {
        Ok((base, descriptor)) => {
            // T3 persists independently of the frontend. Keep a reaper while
            // Bukno lives; closing the frontend never stops agent work.
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(Some(LocalTarget { launcher, home: config.managed_home.clone(), base, descriptor }))
        }
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(error)
        }
    }
}

async fn wait_for_server(
    home: &Path,
    proof_dir: &Path,
    child: &mut tokio::process::Child,
    allow_launcher_exit: bool,
) -> Result<(Url, Descriptor), String> {
    let mut last_error = None;
    let result = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if !allow_launcher_exit
                && child.try_wait().map_err(|_| "Bukno could not check T3's startup.".to_owned())?.is_some()
            {
                return Err("T3 stopped during startup. Try again or use advanced connection.".into());
            }
            // T3 can publish its record just before opening the HTTP listener.
            match discover(home, proof_dir).await {
                Ok(Some(found)) => return Ok(found),
                Err(error) => last_error = Some(error),
                Ok(None) => {}
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        Err(last_error.unwrap_or_else(|| "T3 did not become ready. Try again or use advanced connection.".into()))
    })
}

impl LocalTarget {
    pub async fn credential(&self) -> Result<Secret, String> {
        let mut command = self.launcher.command();
        command
            .arg("pair")
            .arg("--base-dir")
            .arg(&self.home)
            .args(["--ttl", "5m", "--label", "Bukno"])
            .current_dir(&self.home)
            .env("T3CODE_HOME", &self.home);
        let bytes = output(command, 30).await?;
        let text = String::from_utf8_lossy(&bytes);
        let link = text.lines().find_map(|line| line.trim().strip_prefix("Pairing URL: ")).ok_or_else(|| {
            "This T3 installation could not provide a connection link. Use advanced connection.".to_owned()
        })?;
        let request = parse_pairing(Some(self.base.as_str()), link).map_err(|e| e.user_message())?;
        // The CLI can advertise a LAN address while discovery uses loopback.
        // Always exchange at the verified runtime endpoint, not that link host.
        // Check identity again before exchanging a credential.
        let current = Http::new().descriptor(&self.base).await.map_err(|e| e.user_message())?;
        if current.environment_id != self.descriptor.environment_id {
            return Err("A different T3 server answered during setup. Try again.".into());
        }
        Ok(request.credential)
    }

    pub async fn add_project(&self, path: &Path) -> Result<String, String> {
        let mut command = self.launcher.command();
        command
            .args(["project", "add", "--title", "Bukno chats"])
            .arg(path)
            .env("T3CODE_HOME", &self.home)
            .current_dir(&self.home);
        let bytes = output(command, 30).await?;
        let text = String::from_utf8_lossy(&bytes);
        text.lines()
            .find_map(|line| {
                let id = line.trim().strip_prefix("Added project ")?.split_whitespace().next()?;
                uuid::Uuid::parse_str(id).ok()?;
                Some(id.to_owned())
            })
            .ok_or_else(|| {
                "T3 added the folder but did not return its project details. Check the connection and try again.".into()
            })
    }
}

fn release_asset() -> Result<(&'static str, &'static str), String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("linux-x64.tar.gz", "5f9e29cf2712c87736556c99ea580606b399897cb846c2401a434a0d05c4eeca")),
        ("linux", "aarch64") => Ok(("linux-arm64.tar.gz", "73f03e41f2173a9695bb60e2867f14133cf396e0340af6cc8e0ceecd4efcd7d4")),
        ("macos", "aarch64") => Ok(("darwin-arm64.tar.gz", "e3773c55c056efe8200c58e1717a3c9c265c6450af348afcddf5738b56bc935b")),
        ("windows", "x86_64") => Ok(("win32-x64.zip", "426ebec863bb0d3be833d70ae09ea4a06fc581836df1a37826aa1e5f6bea7fed")),
        ("windows", "aarch64") => Ok(("win32-arm64.zip", "9763a079c53bdc52e55fe534ff3caa43f6d806017d92de5366d757a24e6004ad")),
        _ => Err("Automatic T3 download is not available for this computer yet. Install T3 Code from t3.codes or use advanced connection.".into()),
    }
}

/// Cleans an interrupted download as well as ordinary errors.
struct StagingCleanup {
    path: PathBuf,
    // Keep the setup reservation until a cancelled worker has finished cleanup.
    lock: Arc<std::fs::File>,
}
impl StagingCleanup {
    async fn remove(mut self) {
        let path = std::mem::take(&mut self.path);
        let lock = self.lock.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _lock = lock;
            std::fs::remove_dir_all(path)
        })
        .await;
    }
}
impl Drop for StagingCleanup {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.path);
        if path.as_os_str().is_empty() {
            return;
        }
        let lock = self.lock.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(move || {
                let _lock = lock;
                let _ = std::fs::remove_dir_all(path);
            });
        } else {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn remove_owned_install_dirs(parent: &Path, prefix: &str) {
    for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let owned =
            name.to_str().and_then(|n| n.strip_prefix(prefix)).is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
        if owned && entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

async fn download(
    config: &LocalConfig,
    progress: &(dyn Fn(&str) + Send + Sync),
    lock: Arc<std::fs::File>,
) -> Result<Launcher, String> {
    let (asset, checksum) = release_asset()?;
    let parent = config.install_dir.parent().ok_or_else(|| "T3's install folder is unavailable.".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "Bukno could not create the download folder. Check free space and permissions.".to_owned())?;
    let sweep = parent.to_owned();
    let sweep_lock = lock.clone();
    // The setup lock is held. Only remove this installer's abandoned UUID staging.
    tokio::task::spawn_blocking(move || {
        let _lock = sweep_lock;
        remove_owned_install_dirs(&sweep, ".download-");
    })
    .await
    .map_err(|_| "Bukno could not prepare the download folder.".to_owned())?;
    let staging = parent.join(format!(".download-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging).map_err(|_| "Bukno could not prepare the T3 download.".to_owned())?;
    let cleanup = StagingCleanup { path: staging.clone(), lock };
    if let Err(error) = download_archive(&staging, asset, checksum, progress).await {
        cleanup.remove().await;
        return Err(error);
    }
    progress("Installing T3 Code…");
    // Give the worker ownership of staging while extracting. If setup is
    // cancelled, it finishes and cleans up without a competing deletion.
    let (cleanup, extracted) = tokio::task::spawn_blocking(move || {
        let result = extract_archive(&cleanup.path, asset);
        (cleanup, result)
    })
    .await
    .map_err(|_| "T3's download could not be installed.".to_owned())?;
    let result = async {
        extracted?;
        let launcher = Launcher { program: staging.join(if cfg!(windows) { "t3.exe" } else { "t3" }), entry: None };
        let version_bytes = output(
            {
                let mut c = launcher.command();
                c.arg("--version");
                c
            },
            30,
        )
        .await?;
        if !String::from_utf8_lossy(&version_bytes).contains(crate::pinned::SERVER_VERSION) {
            return Err("The downloaded T3 version did not match. Try downloading it again.".into());
        }
        let old_binary = config.install_dir.join(if cfg!(windows) { "t3.exe" } else { "t3" });
        let backup = if config.install_dir.exists() {
            if old_binary.is_file() {
                return Err("A T3 installation already exists here. Check its permissions and try again.".into());
            }
            let backup = parent.join(format!(".incomplete-{}-{}", crate::pinned::SERVER_VERSION, uuid::Uuid::new_v4()));
            tokio::fs::rename(&config.install_dir, &backup).await.map_err(|_| {
                "Bukno could not repair the incomplete T3 installation. Check folder permissions.".to_owned()
            })?;
            Some(backup)
        } else {
            None
        };
        if tokio::fs::rename(&staging, &config.install_dir).await.is_err() {
            if let Some(backup) = backup {
                let _ = tokio::fs::rename(backup, &config.install_dir).await;
            }
            return Err("Bukno could not finish installing T3. Check free space and try again.".into());
        }
        // The verified replacement is now in place. Rollback backups for this
        // version are no longer needed, including leftovers from an older repair.
        // Never prune them before the replacement succeeds.
        let parent = parent.to_owned();
        let lock = cleanup.lock.clone();
        tokio::task::spawn_blocking(move || {
            let _lock = lock;
            let prefix = format!(".incomplete-{}-", crate::pinned::SERVER_VERSION);
            remove_owned_install_dirs(&parent, &prefix);
        })
        .await
        .map_err(|_| "T3 was installed, but its old files could not be cleaned up. Try again.".to_owned())?;
        Ok::<_, String>(())
    }
    .await;
    cleanup.remove().await;
    result?;
    Ok(Launcher { program: config.install_dir.join(if cfg!(windows) { "t3.exe" } else { "t3" }), entry: None })
}

async fn download_archive(
    staging: &Path,
    asset: &str,
    checksum: &str,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    install_tls_provider();
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent("Bukno T3 setup")
        .build()
        .map_err(|_| "Bukno could not prepare a secure download.".to_owned())?;
    let version = crate::pinned::SERVER_VERSION;
    let url = format!("https://github.com/pingdotgg/t3code/releases/download/v{version}/t3-{version}-{asset}");
    progress("Downloading T3 Code…");
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "T3 could not be downloaded. Check your internet connection and try again.".to_owned())?;
    let archive_path = staging.join("archive");
    let mut file = tokio::fs::File::create(&archive_path)
        .await
        .map_err(|_| "Bukno could not save the download. Check free space and permissions.".to_owned())?;
    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    let mut last_mb = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The T3 download was interrupted. Check your connection and try again.".to_owned())?
    {
        received += chunk.len() as u64;
        if received > 250 * 1024 * 1024 {
            return Err("The T3 download was unexpectedly large. Try again later.".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|_| "Bukno could not save the download. Check free space and try again.".to_owned())?;
        hasher.update(&chunk);
        let mb = received / (1024 * 1024);
        if mb != last_mb {
            last_mb = mb;
            progress(&format!("Downloading T3 Code… {mb} MB"));
        }
    }
    file.sync_all().await.map_err(|_| "Bukno could not save the completed download.".to_owned())?;
    drop(file);
    if format!("{:x}", hasher.finalize()) != checksum {
        return Err("The T3 download did not pass verification. Try downloading it again.".into());
    }
    Ok(())
}

fn extract_archive(staging: &Path, asset: &str) -> Result<(), String> {
    // Upstream's archives have one top-level folder. Copy only regular files
    // and directories; reject traversal, links and unreasonable expansion.
    let unpack = |relative: &Path| -> Result<PathBuf, String> {
        use std::path::Component;
        let mut parts = relative.components();
        if !matches!(parts.next(), Some(Component::Normal(_))) {
            return Err("T3's archive has an invalid path.".into());
        }
        let tail = parts.as_path();
        if tail.components().any(|p| !matches!(p, Component::Normal(_))) {
            return Err("T3's archive has an invalid path.".into());
        }
        Ok(staging.join(tail))
    };
    let archive_path = staging.join("archive");
    let archive = std::fs::File::open(&archive_path).map_err(|_| "Bukno could not read the download.".to_owned())?;
    let mut expanded = 0_u64;
    if asset.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(archive).map_err(|_| "T3's download could not be opened.".to_owned())?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|_| "T3's archive could not be read.".to_owned())?;
            if entry.is_symlink() {
                return Err("T3's archive contains an unsupported link.".into());
            }
            let target = unpack(Path::new(entry.name()))?;
            expanded += entry.size();
            if expanded > 1024 * 1024 * 1024 {
                return Err("T3's archive was unexpectedly large.".into());
            }
            if entry.is_dir() {
                std::fs::create_dir_all(target)
            } else {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| "Bukno could not create T3's folders.".to_owned())?;
                }
                let mut file = std::fs::File::create(target)
                    .map_err(|_| "Bukno could not install T3. Check free space.".to_owned())?;
                std::io::copy(&mut entry, &mut file).map(|_| ())
            }
            .map_err(|_| "Bukno could not install T3. Check free space.".to_owned())?;
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in tar.entries().map_err(|_| "T3's archive could not be read.".to_owned())? {
            let mut entry = entry.map_err(|_| "T3's archive could not be read.".to_owned())?;
            let path = entry.path().map_err(|_| "T3's archive has an invalid path.".to_owned())?;
            let target = unpack(&path)?;
            let kind = entry.header().entry_type();
            if !(kind.is_file() || kind.is_dir()) {
                return Err("T3's archive contains an unsupported link.".into());
            }
            expanded += entry.size();
            if expanded > 1024 * 1024 * 1024 {
                return Err("T3's archive was unexpectedly large.".into());
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|_| "Bukno could not create T3's folders.".to_owned())?;
            }
            entry.unpack(target).map_err(|_| "Bukno could not install T3. Check free space.".to_owned())?;
        }
    }
    std::fs::remove_file(archive_path).map_err(|_| "Bukno could not finish the installation.".to_owned())?;
    Ok(())
}
