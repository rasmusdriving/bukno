//! Owned engine processes (specification section 16).
//!
//! On macOS every engine starts as the leader of its own process group, so
//! Bukno can end it together with the tools it started. Records of owned
//! processes let the next launch clean up after a crash. Stopping is always
//! by recorded process group and start time, never by a name match.

use std::path::Path;

/// Identity of an owned process. PIDs are reused, so the start time is part of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRecord {
    pub pid: u32,
    /// Seconds since the Unix epoch, as the OS reports it.
    pub started: u64,
    pub executable: String,
    pub generation: u64,
}

#[derive(Debug)]
pub enum ProcessError {
    Unavailable(&'static str),
    Io(std::io::Error),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(what) => write!(f, "{what} is not available on this platform yet"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProcessError {}

/// Make a command start in its own process group when spawned.
pub fn own_group(command: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = command;
}

/// When a process started, or None if it does not exist.
pub fn start_time(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::process_start(pid)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// Whether the recorded process is still the same running process.
pub fn is_alive(record: &ProcessRecord) -> bool {
    start_time(record.pid) == Some(record.started)
}

/// Processes still in the group that `leader` started.
pub fn group_members(leader: u32) -> Vec<u32> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::group_members(leader)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = leader;
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// Ask politely.
    Terminate,
    /// End now.
    Kill,
}

/// Signal every process in the group led by `leader`.
pub fn signal_group(leader: u32, signal: Signal) -> Result<(), ProcessError> {
    #[cfg(unix)]
    {
        let sig = match signal {
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: killpg only sends a signal; a stale group ID fails with ESRCH.
        let rc = unsafe { libc::killpg(leader as libc::pid_t, sig) };
        if rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(ProcessError::Io(std::io::Error::last_os_error()))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (leader, signal);
        Err(ProcessError::Unavailable("Stopping a process family"))
    }
}

/// Load the owned-process records left by an earlier run.
pub fn load_records(path: &Path) -> Vec<ProcessRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            Some(ProcessRecord {
                pid: parts.next()?.parse().ok()?,
                started: parts.next()?.parse().ok()?,
                generation: parts.next()?.parse().ok()?,
                executable: parts.next()?.to_owned(),
            })
        })
        .collect()
}

/// Replace the records file atomically.
pub fn save_records(path: &Path, records: &[ProcessRecord]) -> std::io::Result<()> {
    let text: String = records
        .iter()
        .map(|r| format!("{}\t{}\t{}\t{}\n", r.pid, r.started, r.generation, r.executable.replace(['\t', '\n'], " ")))
        .collect();
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

/// End process families recorded by an earlier Bukno that are still running.
/// Returns the ones that had to be stopped.
pub fn clean_up_leftovers(path: &Path) -> Vec<ProcessRecord> {
    let leftovers: Vec<ProcessRecord> = load_records(path).into_iter().filter(is_alive).collect();
    for record in &leftovers {
        let _ = signal_group(record.pid, Signal::Terminate);
    }
    if !leftovers.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(500));
        for record in leftovers.iter().filter(|r| is_alive(r)) {
            let _ = signal_group(record.pid, Signal::Kill);
        }
    }
    let _ = save_records(path, &[]);
    leftovers
}

/// One application writer per state folder (section 5).
pub struct InstanceLock {
    #[cfg(unix)]
    _file: std::fs::File,
}

#[derive(Debug)]
pub enum LockError {
    /// Another Bukno holds the lock. Its PID, when readable.
    Held(Option<u32>),
    Unavailable(&'static str),
    Io(std::io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held(Some(pid)) => write!(f, "Bukno is already running (process {pid})"),
            Self::Held(None) => write!(f, "Bukno is already running"),
            Self::Unavailable(what) => write!(f, "{what} is not available on this platform yet"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl InstanceLock {
    /// Take the lock in `state_dir`, or report who holds it. The OS releases
    /// it when the process ends, so a crash never leaves a stale lock.
    pub fn acquire(state_dir: &Path) -> Result<Self, LockError> {
        #[cfg(unix)]
        {
            use std::io::{Read, Seek, Write};
            use std::os::fd::AsRawFd;
            std::fs::create_dir_all(state_dir).map_err(LockError::Io)?;
            let path = state_dir.join("bukno.lock");
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(LockError::Io)?;
            // SAFETY: flock on an open descriptor we own.
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc != 0 {
                let mut text = String::new();
                let _ = file.read_to_string(&mut text);
                return Err(LockError::Held(text.trim().parse().ok()));
            }
            file.set_len(0).map_err(LockError::Io)?;
            file.rewind().map_err(LockError::Io)?;
            write!(file, "{}", std::process::id()).map_err(LockError::Io)?;
            file.flush().map_err(LockError::Io)?;
            Ok(Self { _file: file })
        }
        #[cfg(not(unix))]
        {
            let _ = state_dir;
            Err(LockError::Unavailable("The single-instance lock"))
        }
    }
}
