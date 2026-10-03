//! The engine's stdio: one owned child, stdout only for protocol, stderr
//! drained separately so neither pipe can stall the engine.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::protocol::{self, Incoming, MAX_FRAME};

/// How to start one engine process. Resolved again for every start, so an
/// engine updated while Bukno runs is picked up at the next start.
#[derive(Clone, Debug)]
pub struct Launch {
    pub executable: PathBuf,
    pub args: Vec<String>,
    /// Extra environment, such as a PATH that works from a Finder launch.
    pub env: Vec<(String, OsString)>,
    pub version: String,
}

/// Lines of engine stderr kept for diagnostics.
const STDERR_LINES: usize = 40;

#[derive(Debug)]
pub enum Line {
    Message(Incoming),
    /// Not JSON, or not a message Bukno recognizes.
    Malformed(usize),
    /// Longer than [`MAX_FRAME`]; discarded unparsed.
    TooLarge,
    /// stdout closed: the engine exited or closed its side.
    Closed,
}

pub struct Spawned {
    pub child: Child,
    pub pid: u32,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
}

pub fn spawn(launch: &Launch) -> std::io::Result<Spawned> {
    let mut command = Command::new(&launch.executable);
    command
        .args(&launch.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Bukno stops the process group itself; dropping the handle is not cleanup.
        .kill_on_drop(false);
    for (key, value) in &launch.env {
        command.env(key, value);
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let pid = child.id().ok_or_else(|| std::io::Error::other("the engine exited immediately"))?;
    let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| std::io::Error::other("no stderr"))?;
    Ok(Spawned { child, pid, stdin, stdout, stderr })
}

/// Read protocol lines, bounded per line, and forward them tagged with the
/// connection generation.
pub async fn read(stdout: ChildStdout, generation: u64, out: mpsc::Sender<(u64, Line)>) {
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let chunk = match reader.fill_buf().await {
            Ok([]) | Err(_) => break,
            Ok(chunk) => chunk,
        };
        let (take, ends) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !oversized {
            if line.len() + take > MAX_FRAME + 1 {
                oversized = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(&chunk[..take]);
            }
        }
        reader.consume(take);
        if ends {
            let message = if oversized {
                Line::TooLarge
            } else {
                let bytes = std::mem::take(&mut line);
                let trimmed = bytes.trim_ascii();
                if trimmed.is_empty() {
                    continue;
                }
                match protocol::parse(trimmed) {
                    Some(m) => Line::Message(m),
                    None => Line::Malformed(trimmed.len()),
                }
            };
            oversized = false;
            if out.send((generation, message)).await.is_err() {
                return;
            }
        }
    }
    let _ = out.send((generation, Line::Closed)).await;
}

/// Write queued lines. Ends when the sender is dropped, which closes stdin
/// so the engine sees end of input and exits.
pub async fn write(mut stdin: ChildStdin, mut lines: mpsc::UnboundedReceiver<String>) {
    while let Some(line) = lines.recv().await {
        if stdin.write_all(line.as_bytes()).await.is_err()
            || stdin.write_all(b"\n").await.is_err()
            || stdin.flush().await.is_err()
        {
            break;
        }
    }
    let _ = stdin.shutdown().await;
}

pub type StderrTail = Arc<Mutex<VecDeque<String>>>;

/// Keep the last lines of stderr. Never parsed as protocol and never shown
/// in full; it can contain paths but is not written to diagnostics wholesale.
pub async fn drain_stderr(stderr: ChildStderr, tail: StderrTail) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut tail = tail.lock().expect("stderr tail");
        if tail.len() == STDERR_LINES {
            tail.pop_front();
        }
        tail.push_back(line.chars().take(400).collect());
    }
}
