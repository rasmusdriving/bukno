//! The Codex adapter: one owned `codex app-server` process shared by every
//! Codex chat, a request map, and the mapping from protocol messages to
//! Bukno's normalized events (specification section 9).
//!
//! It reports what the engine says and answers what it is told. Which chat
//! an event belongs to, whether a run may start and what a failure means for
//! the chat are the coordinator's decisions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bukno_core::decision::{DecisionAnswer, DecisionKind, Question};
use bukno_core::event::{EngineEvent, EngineEventKind, EngineRequest, ImportedItem, Input, Reconciliation};
use bukno_core::ids::{DecisionId, ItemId, MessageId, RunId, TaskId};
use bukno_core::message::Provider;
use bukno_core::run::RunOutcome;
use bukno_core::task::RunSettings;
use bukno_platform::process::{self, ProcessRecord, Signal};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::presets;
use super::protocol::{self, Account, Incoming, ModelInfo, RpcError, str_at};
use super::transport::{self, Launch, Line, StderrTail};

/// Ordinary control requests time out after this (section 17). A timeout
/// does not mean a submitted task failed.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
/// Startup, including the handshake, is capped separately.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
/// How long an orderly shutdown waits for the engine to exit by itself.
const EXIT_GRACE: Duration = Duration::from_secs(3);
/// Turns read back when reconciling or checking outside continuation.
const HISTORY_TURNS: u32 = 20;

/// Resolves the engine to start, every time one starts.
pub type Resolve = Arc<dyn Fn() -> Result<Launch, String> + Send + Sync>;
/// Engine status for the setup screen and the engine records.
pub type StatusFn = Arc<dyn Fn(Status) + Send + Sync>;

#[derive(Clone, Debug)]
pub enum Status {
    Starting {
        version: String,
        path: String,
    },
    Spawned {
        record: ProcessRecord,
    },
    Ready {
        generation: u64,
        account: Account,
        models: Vec<ModelInfo>,
        default_model: Option<String>,
        note: Option<String>,
    },
    /// Start or handshake failed. `version` is the engine that failed, when known.
    Failed {
        reason: String,
        version: Option<String>,
    },
    Exited {
        generation: u64,
        pid: u32,
        reason: String,
    },
    /// A turn completed normally on this engine.
    TurnCompleted {
        generation: u64,
    },
}

#[derive(Clone, Debug, Default)]
pub struct ShutdownReport {
    /// The engine exited after its input closed, without being signalled.
    pub exited_by_itself: bool,
    pub forced: bool,
    /// Processes still in the engine's group afterwards.
    pub leftover: Vec<u32>,
    pub waited_ms: u64,
}

enum Control {
    Request(Box<EngineRequest>),
    Shutdown(oneshot::Sender<ShutdownReport>),
}

/// Handle to the adapter task.
#[derive(Clone)]
pub struct CodexAdapter {
    control: mpsc::UnboundedSender<Control>,
}

impl CodexAdapter {
    /// Start the adapter task on the current Tokio runtime. No engine starts
    /// until the coordinator asks for a connection.
    pub fn spawn(inbox: mpsc::Sender<Input>, resolve: Resolve, status: StatusFn) -> Self {
        let (control, rx) = mpsc::unbounded_channel();
        let (lines_tx, lines) = mpsc::channel(1_024);
        let supervisor = Supervisor { inbox, resolve, status, generation: 0, conn: None, lines_tx };
        tokio::spawn(supervisor.run(rx, lines));
        Self { control }
    }

    pub fn request(&self, request: EngineRequest) {
        let _ = self.control.send(Control::Request(Box::new(request)));
    }

    /// Close the engine's input, wait for it to exit, and end its process
    /// group if it does not.
    pub async fn shutdown(&self) -> ShutdownReport {
        let (tx, rx) = oneshot::channel();
        if self.control.send(Control::Shutdown(tx)).is_err() {
            return ShutdownReport::default();
        }
        rx.await.unwrap_or_default()
    }
}

#[derive(Debug)]
enum Pending {
    Initialize,
    Account,
    Models,
    Config,
    ThreadStart { run: RunId },
    ThreadResume { run: RunId },
    TurnStart { run: RunId },
    Interrupt,
    Reconcile { run: RunId, message: MessageId },
    Outside { task: TaskId, latest: Option<String> },
}

struct RunCtx {
    task: TaskId,
    message: MessageId,
    body: String,
    cwd: String,
    settings: RunSettings,
    thread: Option<String>,
    turn: Option<String>,
    interrupt_pending: bool,
    last_error: Option<String>,
    accepted: bool,
}

enum Waiting {
    Approval,
    Question,
}

struct DecisionCtx {
    request: Value,
    waiting: Waiting,
}

struct Conn {
    generation: u64,
    pid: u32,
    started: u64,
    version: String,
    child: tokio::process::Child,
    writer: Option<mpsc::UnboundedSender<String>>,
    stderr: StderrTail,
    ready: bool,
    next_id: i64,
    pending: HashMap<i64, (Pending, Instant)>,
    /// Requests that arrived during the handshake.
    queued: Vec<EngineRequest>,
    account: Account,
    models: Vec<ModelInfo>,
    config_model: Option<String>,
    default_model: Option<String>,
    /// Threads loaded on this connection, with the preset they were loaded with.
    threads: HashMap<String, String>,
    runs: HashMap<RunId, RunCtx>,
    turns: HashMap<String, RunId>,
    /// The run currently starting or active on each thread.
    thread_runs: HashMap<String, RunId>,
    items: HashMap<String, ItemId>,
    file_items: HashMap<String, Vec<String>>,
    decisions: HashMap<DecisionId, DecisionCtx>,
    by_request: HashMap<String, DecisionId>,
    malformed: u64,
}

impl Conn {
    fn send(&self, line: String) {
        if let Some(writer) = &self.writer {
            let _ = writer.send(line);
        }
    }

    fn call(&mut self, method: &str, params: Value, pending: Pending, timeout: Duration) {
        self.next_id += 1;
        let id = self.next_id;
        self.pending.insert(id, (pending, Instant::now() + timeout));
        self.send(protocol::request(id, method, params));
    }

    fn run_for(&self, params: &Value) -> Option<RunId> {
        if let Some(run) = str_at(params, "turnId").and_then(|t| self.turns.get(t)) {
            return Some(*run);
        }
        str_at(params, "threadId").and_then(|t| self.thread_runs.get(t)).copied()
    }

    fn stderr_tail(&self) -> String {
        let tail = self.stderr.lock().expect("stderr tail");
        tail.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default()
    }
}

struct Supervisor {
    inbox: mpsc::Sender<Input>,
    resolve: Resolve,
    status: StatusFn,
    generation: u64,
    conn: Option<Conn>,
    lines_tx: mpsc::Sender<(u64, Line)>,
}

impl Supervisor {
    async fn run(mut self, mut control: mpsc::UnboundedReceiver<Control>, mut lines: mpsc::Receiver<(u64, Line)>) {
        loop {
            let deadline = self.conn.as_ref().and_then(|c| c.pending.values().map(|(_, at)| *at).min());
            tokio::select! {
                message = control.recv() => match message {
                    Some(Control::Request(request)) => self.on_request(*request).await,
                    Some(Control::Shutdown(reply)) => {
                        let report = self.shutdown().await;
                        let _ = reply.send(report);
                    }
                    None => {
                        self.shutdown().await;
                        return;
                    }
                },
                Some((generation, line)) = lines.recv() => self.on_line(generation, line).await,
                _ = sleep_until(deadline) => self.on_timeouts().await,
            }
        }
    }

    async fn emit(&self, generation: u64, kind: EngineEventKind) {
        let event = EngineEvent { provider: Provider::Codex, connection_generation: generation, kind };
        let _ = self.inbox.send(Input::Engine(event)).await;
    }

    // ----- Requests from the coordinator ---------------------------------

    async fn on_request(&mut self, request: EngineRequest) {
        if let EngineRequest::Connect = request {
            return self.connect().await;
        }
        let Some(conn) = self.conn.as_mut() else {
            // The connection this was meant for is gone; the coordinator hears
            // ConnectionLost and treats anything unsent as unknown.
            return;
        };
        if !conn.ready {
            conn.queued.push(request);
            return;
        }
        let generation = conn.generation;
        match request {
            EngineRequest::Connect => {}
            EngineRequest::StartTurn { task, run, message, body, session, cwd, settings } => {
                let thread = session.as_ref().map(|s| s.thread.clone());
                conn.runs.insert(
                    run,
                    RunCtx {
                        task,
                        message,
                        body,
                        cwd: cwd.clone(),
                        settings: settings.clone(),
                        thread: thread.clone(),
                        turn: None,
                        interrupt_pending: false,
                        last_error: None,
                        accepted: false,
                    },
                );
                let preset = presets::find(&settings.preset);
                let model = settings.model.clone().or_else(|| conn.default_model.clone());
                match thread {
                    None => {
                        let params = json!({
                            "cwd": cwd,
                            "model": model,
                            "sandbox": preset.sandbox,
                            "approvalPolicy": preset.approval,
                        });
                        conn.call("thread/start", params, Pending::ThreadStart { run }, CONTROL_TIMEOUT);
                    }
                    Some(thread) if conn.threads.contains_key(&thread) => {
                        conn.thread_runs.insert(thread.clone(), run);
                        start_turn(conn, run);
                    }
                    Some(thread) => {
                        conn.thread_runs.insert(thread.clone(), run);
                        let params = json!({
                            "threadId": thread,
                            "cwd": cwd,
                            "model": model,
                            "sandbox": preset.sandbox,
                            "approvalPolicy": preset.approval,
                            "excludeTurns": true,
                        });
                        conn.call("thread/resume", params, Pending::ThreadResume { run }, CONTROL_TIMEOUT);
                    }
                }
            }
            EngineRequest::Interrupt { run } => {
                let Some(ctx) = conn.runs.get_mut(&run) else {
                    return;
                };
                match (ctx.thread.clone(), ctx.turn.clone()) {
                    (Some(thread), Some(turn)) => {
                        conn.call(
                            "turn/interrupt",
                            json!({"threadId": thread, "turnId": turn}),
                            Pending::Interrupt,
                            CONTROL_TIMEOUT,
                        );
                    }
                    // Sent once the turn ID is known.
                    _ => ctx.interrupt_pending = true,
                }
            }
            EngineRequest::Answer { decision, answer } => {
                let Some(ctx) = conn.decisions.get(&decision) else {
                    return;
                };
                let line = match (&ctx.waiting, answer) {
                    (Waiting::Approval, DecisionAnswer::Allow) => {
                        protocol::reply(&ctx.request, json!({"decision": "accept"}))
                    }
                    (Waiting::Approval, _) => protocol::reply(&ctx.request, json!({"decision": "decline"})),
                    (Waiting::Question, DecisionAnswer::Answers(answers)) => {
                        let map: serde_json::Map<String, Value> =
                            answers.into_iter().map(|(id, values)| (id, json!({"answers": values}))).collect();
                        protocol::reply(&ctx.request, json!({"answers": map}))
                    }
                    (Waiting::Question, _) => {
                        protocol::reply_error(&ctx.request, -32000, "The user declined to answer.")
                    }
                };
                conn.send(line);
            }
            EngineRequest::Reconcile { run, message, session, cwd: _ } => {
                conn.call(
                    "thread/turns/list",
                    json!({"threadId": session.thread, "limit": HISTORY_TURNS, "itemsView": "full"}),
                    Pending::Reconcile { run, message },
                    CONTROL_TIMEOUT,
                );
            }
            EngineRequest::CheckOutside { task, session, cwd: _ } => {
                conn.call(
                    "thread/turns/list",
                    json!({"threadId": session.thread, "limit": HISTORY_TURNS, "itemsView": "full"}),
                    Pending::Outside { task, latest: session.latest_turn },
                    CONTROL_TIMEOUT,
                );
            }
            EngineRequest::Kill => {
                let pid = conn.pid;
                let _ = process::signal_group(pid, Signal::Kill);
                let _ = generation;
            }
        }
    }

    async fn connect(&mut self) {
        if self.conn.is_some() {
            return;
        }
        let launch = match (self.resolve)() {
            Ok(launch) => launch,
            Err(reason) => {
                (self.status)(Status::Failed { reason: reason.clone(), version: None });
                return self.emit(self.generation + 1, EngineEventKind::ConnectFailed { reason }).await;
            }
        };
        (self.status)(Status::Starting {
            version: launch.version.clone(),
            path: launch.executable.display().to_string(),
        });
        let spawned = match transport::spawn(&launch) {
            Ok(spawned) => spawned,
            Err(e) => {
                let reason = format!("Codex {} could not start: {e}", launch.version);
                (self.status)(Status::Failed { reason: reason.clone(), version: Some(launch.version) });
                return self.emit(self.generation + 1, EngineEventKind::ConnectFailed { reason }).await;
            }
        };
        self.generation += 1;
        let generation = self.generation;
        let started = process::start_time(spawned.pid).unwrap_or_default();
        (self.status)(Status::Spawned {
            record: ProcessRecord {
                pid: spawned.pid,
                started,
                executable: launch.executable.display().to_string(),
                generation,
            },
        });
        let (writer, writes) = mpsc::unbounded_channel();
        let stderr: StderrTail = Arc::default();
        tokio::spawn(transport::write(spawned.stdin, writes));
        tokio::spawn(transport::read(spawned.stdout, generation, self.lines_tx.clone()));
        tokio::spawn(transport::drain_stderr(spawned.stderr, stderr.clone()));
        let mut conn = Conn {
            generation,
            pid: spawned.pid,
            started,
            version: launch.version.clone(),
            child: spawned.child,
            writer: Some(writer),
            stderr,
            ready: false,
            next_id: 0,
            pending: HashMap::new(),
            queued: Vec::new(),
            account: Account::SignedOut,
            models: Vec::new(),
            config_model: None,
            default_model: None,
            threads: HashMap::new(),
            runs: HashMap::new(),
            turns: HashMap::new(),
            thread_runs: HashMap::new(),
            items: HashMap::new(),
            file_items: HashMap::new(),
            decisions: HashMap::new(),
            by_request: HashMap::new(),
            malformed: 0,
        };
        // Bukno's own name and version; only request types the UI can answer are handled.
        conn.call(
            "initialize",
            json!({
                "clientInfo": {"name": "bukno", "title": "Bukno", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": false},
            }),
            Pending::Initialize,
            STARTUP_TIMEOUT,
        );
        self.conn = Some(conn);
    }

    async fn on_timeouts(&mut self) {
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let now = Instant::now();
        let expired: Vec<i64> = conn.pending.iter().filter(|(_, (_, at))| *at <= now).map(|(id, _)| *id).collect();
        let generation = conn.generation;
        let mut startup_failed = None;
        let mut events = Vec::new();
        for id in expired {
            let Some((pending, _)) = conn.pending.remove(&id) else {
                continue;
            };
            match pending {
                Pending::Initialize | Pending::Account | Pending::Models | Pending::Config => {
                    startup_failed = Some(format!("Codex {} did not finish starting within 20 seconds.", conn.version));
                }
                // No reply to Stop is not a failure of the run: the turn's own
                // completion settles it (an interrupt for a finished turn gets no reply).
                Pending::Interrupt => {}
                Pending::ThreadStart { run } | Pending::ThreadResume { run } | Pending::TurnStart { run } => {
                    // The outcome is unknown. Forget the run here so late events
                    // cannot revive it; the coordinator reconciles instead of guessing.
                    if let Some(ctx) = conn.runs.remove(&run)
                        && let Some(thread) = ctx.thread
                    {
                        conn.thread_runs.remove(&thread);
                    }
                    events.push(EngineEventKind::RunLost {
                        run,
                        reason: "Codex did not confirm the message within 30 seconds".into(),
                    });
                }
                Pending::Reconcile { run, .. } => events.push(EngineEventKind::Reconciled {
                    run,
                    result: Reconciliation::Unavailable { reason: "Codex did not answer in time.".into() },
                }),
                Pending::Outside { task, .. } => events
                    .push(EngineEventKind::OutsideCheckFailed { task, reason: "Codex did not answer in time".into() }),
            }
        }
        for kind in events {
            self.emit(generation, kind).await;
        }
        if let Some(reason) = startup_failed {
            let conn = self.conn.as_ref().expect("connection");
            if !conn.ready {
                (self.status)(Status::Failed { reason: reason.clone(), version: Some(conn.version.clone()) });
            }
            let _ = process::signal_group(conn.pid, Signal::Kill);
        }
    }

    // ----- Lines from the engine -----------------------------------------

    async fn on_line(&mut self, generation: u64, line: Line) {
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        if conn.generation != generation {
            return;
        }
        match line {
            Line::Message(Incoming::Response { id, result }) => {
                let Some((pending, _)) = conn.pending.remove(&id) else {
                    return;
                };
                self.on_response(pending, result).await;
            }
            Line::Message(Incoming::Request { id, method, params }) => self.on_server_request(id, method, params).await,
            Line::Message(Incoming::Notification { method, params }) => self.on_notification(method, params).await,
            Line::Malformed(_) => {
                conn.malformed += 1;
            }
            Line::TooLarge => {
                // Never parse part of a frame. End the connection; the
                // coordinator reconciles what was running.
                let _ = process::signal_group(conn.pid, Signal::Kill);
            }
            Line::Closed => self.on_closed().await,
        }
    }

    async fn on_closed(&mut self) {
        let Some(mut conn) = self.conn.take() else {
            return;
        };
        conn.writer = None;
        let status = tokio::time::timeout(Duration::from_secs(2), conn.child.wait()).await;
        let exit = match status {
            Ok(Ok(status)) => describe_exit(status),
            _ => "it stopped responding".into(),
        };
        // The engine's own log goes to diagnostics, never into the chat: it is
        // terminal output, and can hold paths and colour codes.
        let tail = conn.stderr_tail();
        if !tail.is_empty() {
            eprintln!("codex stderr before exit: {}", tail.chars().filter(|c| !c.is_control()).collect::<String>());
        }
        let detail = exit;
        // Tool children left behind by the engine are ended with it.
        let _ = process::signal_group(conn.pid, Signal::Kill);
        if conn.ready {
            (self.status)(Status::Exited { generation: conn.generation, pid: conn.pid, reason: detail.clone() });
            self.emit(
                conn.generation,
                EngineEventKind::ConnectionLost { reason: format!("Codex stopped unexpectedly ({detail})") },
            )
            .await;
        } else {
            let reason = format!("Codex {} exited during startup, {detail}", conn.version);
            (self.status)(Status::Exited { generation: conn.generation, pid: conn.pid, reason: detail });
            (self.status)(Status::Failed { reason: reason.clone(), version: Some(conn.version.clone()) });
            self.emit(conn.generation, EngineEventKind::ConnectFailed { reason }).await;
        }
    }

    async fn on_response(&mut self, pending: Pending, result: Result<Value, RpcError>) {
        let conn = self.conn.as_mut().expect("connection");
        let generation = conn.generation;
        match pending {
            Pending::Initialize => match result {
                Ok(_) => {
                    conn.send(protocol::notification("initialized"));
                    conn.call("account/read", json!({}), Pending::Account, STARTUP_TIMEOUT);
                }
                Err(e) => {
                    let reason = format!("Codex {} refused Bukno's handshake: {e}", conn.version);
                    self.fail_startup(reason).await;
                }
            },
            Pending::Account => match result.map(|r| protocol::account(&r)) {
                Ok(Account::SignedOut) => {
                    self.fail_startup(
                        "Codex is not signed in. Run `codex login` in Terminal, then choose Check again.".into(),
                    )
                    .await;
                }
                Ok(account) => {
                    conn.account = account;
                    conn.call("model/list", json!({}), Pending::Models, STARTUP_TIMEOUT);
                }
                Err(e) => self.fail_startup(format!("Codex could not read its account: {e}")).await,
            },
            Pending::Models => {
                conn.models = result.map(|r| protocol::models(&r)).unwrap_or_default();
                conn.call("config/read", json!({}), Pending::Config, STARTUP_TIMEOUT);
            }
            Pending::Config => {
                conn.config_model = result.ok().and_then(|r| protocol::config_model(&r));
                // The user's configured model when this account offers it, else the
                // engine's own default. Never a silent substitute after a failure.
                let listed = |id: &str| conn.models.iter().any(|m| m.id == id);
                let fallback = conn.models.iter().find(|m| m.is_default).or(conn.models.first()).map(|m| m.id.clone());
                let (default_model, note) = match conn.config_model.clone() {
                    Some(model) if listed(&model) || conn.models.is_empty() => (Some(model), None),
                    Some(model) => {
                        let note = fallback.as_ref().map(|f| {
                            format!(
                                "Your Codex default model {model} is not offered for this account, so Bukno uses {f}."
                            )
                        });
                        (fallback, note)
                    }
                    None => (fallback, None),
                };
                conn.default_model = default_model.clone();
                conn.ready = true;
                (self.status)(Status::Ready {
                    generation,
                    account: conn.account.clone(),
                    models: conn.models.clone(),
                    default_model,
                    note,
                });
                let queued = std::mem::take(&mut conn.queued);
                self.emit(generation, EngineEventKind::Connected).await;
                for request in queued {
                    Box::pin(self.on_request(request)).await;
                }
            }
            Pending::ThreadStart { run } | Pending::ThreadResume { run } => {
                let resumed = matches!(pending, Pending::ThreadResume { .. });
                let Some(ctx) = conn.runs.get(&run) else {
                    return;
                };
                let task = ctx.task;
                match result {
                    Ok(value) => {
                        let thread = value
                            .get("thread")
                            .and_then(|t| str_at(t, "id"))
                            .map(str::to_owned)
                            .or_else(|| ctx.thread.clone());
                        let Some(thread) = thread else {
                            let reason = "Codex did not return a thread.".to_owned();
                            return self.emit(generation, EngineEventKind::RunRejected { run, reason }).await;
                        };
                        let model = str_at(&value, "model").map(|m| {
                            let name = conn.models.iter().find(|x| x.id == m).map_or(m, |x| x.display_name.as_str());
                            // The turn's own effort, when it sets one, is what the run uses.
                            let turn_effort = ctx.settings.effort.as_deref();
                            match turn_effort.or_else(|| str_at(&value, "reasoningEffort")) {
                                Some(effort) => format!("{name} · {}", capitalize(effort)),
                                None => name.to_owned(),
                            }
                        });
                        let preset = conn.runs.get(&run).map(|c| c.settings.preset.clone()).unwrap_or_default();
                        conn.threads.insert(thread.clone(), preset);
                        conn.thread_runs.insert(thread.clone(), run);
                        if let Some(ctx) = conn.runs.get_mut(&run) {
                            ctx.thread = Some(thread.clone());
                        }
                        start_turn(conn, run);
                        self.emit(generation, EngineEventKind::SessionReady { task, thread, model }).await;
                    }
                    Err(e) => {
                        let reason = if resumed && e.message.contains("not found") {
                            "Codex no longer has this chat's history, so it cannot continue with its earlier context. Start a new chat to continue.".into()
                        } else {
                            readable_error(&e.message)
                        };
                        conn.runs.remove(&run);
                        self.emit(generation, EngineEventKind::RunRejected { run, reason }).await;
                    }
                }
            }
            Pending::TurnStart { run } => match result {
                Ok(value) => {
                    let Some(turn) = value.get("turn").and_then(|t| str_at(t, "id")).map(str::to_owned) else {
                        return;
                    };
                    let Some(ctx) = conn.runs.get_mut(&run) else {
                        return;
                    };
                    ctx.turn = Some(turn.clone());
                    ctx.accepted = true;
                    conn.turns.insert(turn.clone(), run);
                    let interrupt = std::mem::take(&mut ctx.interrupt_pending).then(|| ctx.thread.clone()).flatten();
                    if let Some(thread) = interrupt {
                        conn.call(
                            "turn/interrupt",
                            json!({"threadId": thread, "turnId": turn}),
                            Pending::Interrupt,
                            CONTROL_TIMEOUT,
                        );
                    }
                    self.emit(generation, EngineEventKind::RunAccepted { run, turn }).await;
                }
                Err(e) => {
                    if let Some(ctx) = conn.runs.remove(&run)
                        && let Some(thread) = ctx.thread
                    {
                        conn.thread_runs.remove(&thread);
                    }
                    let reason = readable_error(&e.message);
                    self.emit(generation, EngineEventKind::RunRejected { run, reason }).await;
                }
            },
            Pending::Interrupt => {}
            Pending::Reconcile { run, message } => {
                let result = match result {
                    Ok(value) => find_message(&value, message),
                    Err(e) => Reconciliation::Unavailable { reason: readable_error(&e.message) },
                };
                self.emit(generation, EngineEventKind::Reconciled { run, result }).await;
            }
            Pending::Outside { task, latest } => {
                let kind = match result {
                    Ok(value) => {
                        let (latest_turn, items) = missed_turns(&value, latest.as_deref());
                        EngineEventKind::OutsideChecked { task, latest_turn, items }
                    }
                    Err(e) => EngineEventKind::OutsideCheckFailed { task, reason: readable_error(&e.message) },
                };
                self.emit(generation, kind).await;
            }
        }
    }

    async fn fail_startup(&mut self, reason: String) {
        let Some(conn) = self.conn.as_ref() else {
            return;
        };
        (self.status)(Status::Failed { reason: reason.clone(), version: Some(conn.version.clone()) });
        let generation = conn.generation;
        let pid = conn.pid;
        // Report the reason now; the exit that follows is not a second failure.
        self.emit(generation, EngineEventKind::ConnectFailed { reason }).await;
        if let Some(conn) = self.conn.as_mut() {
            conn.ready = true;
            conn.runs.clear();
        }
        let _ = process::signal_group(pid, Signal::Terminate);
        if let Some(mut conn) = self.conn.take() {
            conn.writer = None;
            let _ = tokio::time::timeout(Duration::from_secs(2), conn.child.wait()).await;
            let _ = process::signal_group(pid, Signal::Kill);
            (self.status)(Status::Exited { generation, pid, reason: "stopped after a failed start".into() });
        }
    }

    async fn on_server_request(&mut self, id: Value, method: String, params: Value) {
        let conn = self.conn.as_mut().expect("connection");
        let generation = conn.generation;
        let run = conn.run_for(&params);
        let kind = match method.as_str() {
            "item/commandExecution/requestApproval" => Some((
                DecisionKind::Command {
                    command: str_at(&params, "command").map(protocol::display_command).unwrap_or_default(),
                    cwd: str_at(&params, "cwd").map(str::to_owned),
                    reason: str_at(&params, "reason").map(str::to_owned),
                },
                Waiting::Approval,
            )),
            "item/fileChange/requestApproval" => {
                let files = str_at(&params, "itemId").and_then(|i| conn.file_items.get(i)).cloned().unwrap_or_default();
                Some((
                    DecisionKind::FileChange { files, reason: str_at(&params, "reason").map(str::to_owned) },
                    Waiting::Approval,
                ))
            }
            "item/tool/requestUserInput" => {
                let questions: Vec<Question> = params
                    .get("questions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|q| {
                        Some(Question {
                            id: str_at(q, "id")?.to_owned(),
                            header: str_at(q, "header").unwrap_or_default().to_owned(),
                            text: str_at(q, "question").unwrap_or_default().to_owned(),
                            options: q
                                .get("options")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(|o| str_at(o, "label").map(str::to_owned))
                                .collect(),
                            other: q.get("isOther").and_then(Value::as_bool).unwrap_or(false),
                            multi: false,
                        })
                    })
                    .collect();
                // A secret question cannot be answered safely in a chat card.
                let secret = params
                    .get("questions")
                    .and_then(Value::as_array)
                    .is_some_and(|qs| qs.iter().any(|q| q.get("isSecret").and_then(Value::as_bool) == Some(true)));
                (!secret && !questions.is_empty()).then_some((DecisionKind::Question { questions }, Waiting::Question))
            }
            _ => None,
        };
        match (run, kind) {
            (Some(run), Some((kind, waiting))) => {
                let decision = DecisionId(uuid::Uuid::new_v4().as_u128());
                conn.by_request.insert(id.to_string(), decision);
                conn.decisions.insert(decision, DecisionCtx { request: id, waiting });
                self.emit(generation, EngineEventKind::DecisionRequested { run, decision, kind }).await;
            }
            (run, _) => {
                // Never invent an approval: refuse with a supported error and say so.
                conn.send(protocol::reply_error(&id, -32601, &format!("Bukno cannot answer {method} yet.")));
                if let Some(run) = run {
                    let text = format!(
                        "Codex asked for something Bukno cannot answer yet ({}), so it was declined.",
                        request_words(&method)
                    );
                    self.emit(generation, EngineEventKind::Notice { run, text }).await;
                }
            }
        }
    }

    async fn on_notification(&mut self, method: String, params: Value) {
        let conn = self.conn.as_mut().expect("connection");
        let generation = conn.generation;
        let run = conn.run_for(&params);
        let mut events = Vec::new();
        match method.as_str() {
            "turn/started" => {
                if let (Some(run), Some(turn)) = (run, params.get("turn").and_then(|t| str_at(t, "id"))) {
                    conn.turns.insert(turn.to_owned(), run);
                    events.push(EngineEventKind::Activity { run, text: "Starting".into() });
                }
            }
            "item/started" => {
                let item = params.get("item").cloned().unwrap_or(Value::Null);
                if let (Some(run), Some(text)) = (run, activity_words(&item)) {
                    events.push(EngineEventKind::Activity { run, text });
                }
                if str_at(&item, "type") == Some("fileChange")
                    && let Some(id) = str_at(&item, "id")
                {
                    let files = item
                        .get("changes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|c| str_at(c, "path").map(str::to_owned))
                        .collect();
                    conn.file_items.insert(id.to_owned(), files);
                }
            }
            "item/agentMessage/delta" => {
                if let (Some(run), Some(provider_item), Some(delta)) =
                    (run, str_at(&params, "itemId"), str_at(&params, "delta"))
                {
                    let item = *conn
                        .items
                        .entry(provider_item.to_owned())
                        .or_insert_with(|| ItemId(uuid::Uuid::new_v4().as_u128()));
                    events.push(EngineEventKind::TextDelta {
                        run,
                        item,
                        provider_item: provider_item.to_owned(),
                        delta: delta.to_owned(),
                    });
                }
            }
            "item/completed" => {
                let item = params.get("item").cloned().unwrap_or(Value::Null);
                if let (Some(run), Some("agentMessage"), Some(provider_item)) =
                    (run, str_at(&item, "type"), str_at(&item, "id"))
                {
                    let id = *conn
                        .items
                        .entry(provider_item.to_owned())
                        .or_insert_with(|| ItemId(uuid::Uuid::new_v4().as_u128()));
                    events.push(EngineEventKind::ItemCompleted {
                        run,
                        item: id,
                        provider_item: provider_item.to_owned(),
                        text: str_at(&item, "text").map(str::to_owned),
                    });
                }
                if let Some(id) = str_at(&item, "id") {
                    conn.file_items.remove(id);
                }
            }
            "serverRequest/resolved" => {
                if let Some(decision) = params.get("requestId").and_then(|id| conn.by_request.remove(&id.to_string())) {
                    conn.decisions.remove(&decision);
                    events.push(EngineEventKind::DecisionResolved { decision });
                }
            }
            "error" => {
                let message = params.get("error").and_then(|e| str_at(e, "message")).map(readable_error);
                let retry = params.get("willRetry").and_then(Value::as_bool).unwrap_or(false);
                if let (Some(run), Some(message)) = (run, message) {
                    if retry {
                        events.push(EngineEventKind::Activity {
                            run,
                            text: format!("Retrying after an error: {message}"),
                        });
                    } else if let Some(ctx) = conn.runs.get_mut(&run) {
                        ctx.last_error = Some(message);
                    }
                }
            }
            "turn/completed" => {
                let turn = params.get("turn").cloned().unwrap_or(Value::Null);
                let run = str_at(&turn, "id").and_then(|t| conn.turns.get(t)).copied().or(run);
                if let Some(run) = run
                    && let Some(ctx) = conn.runs.remove(&run)
                {
                    if let Some(thread) = &ctx.thread
                        && conn.thread_runs.get(thread) == Some(&run)
                    {
                        conn.thread_runs.remove(thread);
                    }
                    if let Some(turn) = &ctx.turn {
                        conn.turns.remove(turn);
                    }
                    let error = turn.get("error").and_then(|e| str_at(e, "message")).map(readable_error);
                    let outcome = match str_at(&turn, "status") {
                        Some("completed") => RunOutcome::Completed,
                        Some("interrupted") => RunOutcome::Interrupted,
                        _ => RunOutcome::Failed,
                    };
                    if outcome == RunOutcome::Completed {
                        (self.status)(Status::TurnCompleted { generation });
                    }
                    let _ = (ctx.task, &ctx.body, &ctx.cwd, ctx.message, ctx.accepted);
                    events.push(EngineEventKind::RunEnded { run, outcome, reason: error.or(ctx.last_error) });
                }
            }
            _ => {}
        }
        for kind in events {
            self.emit(generation, kind).await;
        }
    }

    async fn shutdown(&mut self) -> ShutdownReport {
        let Some(mut conn) = self.conn.take() else {
            return ShutdownReport { exited_by_itself: true, ..Default::default() };
        };
        let began = std::time::Instant::now();
        // Closing stdin is how the engine learns Bukno is done.
        conn.writer = None;
        let mut report = ShutdownReport::default();
        match tokio::time::timeout(EXIT_GRACE, conn.child.wait()).await {
            Ok(_) => report.exited_by_itself = true,
            Err(_) => {
                report.forced = true;
                let _ = process::signal_group(conn.pid, Signal::Terminate);
                if tokio::time::timeout(Duration::from_secs(1), conn.child.wait()).await.is_err() {
                    let _ = process::signal_group(conn.pid, Signal::Kill);
                    let _ = tokio::time::timeout(Duration::from_secs(1), conn.child.wait()).await;
                }
            }
        }
        // Tools the engine started belong to its group; none may survive it.
        let mut leftover = process::group_members(conn.pid);
        if !leftover.is_empty() {
            let _ = process::signal_group(conn.pid, Signal::Kill);
            tokio::time::sleep(Duration::from_millis(200)).await;
            leftover = process::group_members(conn.pid);
        }
        report.leftover = leftover;
        report.waited_ms = began.elapsed().as_millis() as u64;
        (self.status)(Status::Exited { generation: conn.generation, pid: conn.pid, reason: "Bukno closed it".into() });
        let _ = conn.started;
        report
    }
}

/// Send `turn/start` for a run whose thread is loaded.
fn start_turn(conn: &mut Conn, run: RunId) {
    let Some(ctx) = conn.runs.get(&run) else {
        return;
    };
    let Some(thread) = ctx.thread.clone() else {
        return;
    };
    let mut params = json!({
        "threadId": thread,
        "input": [{"type": "text", "text": ctx.body, "text_elements": []}],
        // Lets reconciliation find this exact message after a crash.
        "clientUserMessageId": ctx.message.hex(),
    });
    if let Some(effort) = &ctx.settings.effort {
        params["effort"] = json!(effort);
    }
    if let Some(model) = &ctx.settings.model {
        params["model"] = json!(model);
    }
    // Apply the run's preset when it differs from the one the thread was loaded with.
    if conn.threads.get(&thread).is_some_and(|p| *p != ctx.settings.preset) {
        let preset = presets::find(&ctx.settings.preset);
        params["approvalPolicy"] = json!(preset.approval);
        params["sandboxPolicy"] = preset.sandbox_policy();
        conn.threads.insert(thread.clone(), ctx.settings.preset.clone());
    }
    conn.call("turn/start", params, Pending::TurnStart { run }, CONTROL_TIMEOUT);
}

/// Find the turn that carries Bukno's message ID.
fn find_message(value: &Value, message: MessageId) -> Reconciliation {
    let wanted = message.hex();
    for turn in value.get("data").and_then(Value::as_array).into_iter().flatten() {
        let items: Vec<&Value> = turn.get("items").and_then(Value::as_array).into_iter().flatten().collect();
        let ours = items
            .iter()
            .any(|i| str_at(i, "type") == Some("userMessage") && str_at(i, "clientId") == Some(wanted.as_str()));
        if !ours {
            continue;
        }
        let outcome = match str_at(turn, "status") {
            Some("completed") => Some(RunOutcome::Completed),
            Some("interrupted") => Some(RunOutcome::Interrupted),
            Some("failed") => Some(RunOutcome::Failed),
            _ => None,
        };
        let items = items
            .iter()
            .filter(|i| str_at(i, "type") == Some("agentMessage"))
            .filter_map(|i| {
                Some(ImportedItem {
                    provider_item: str_at(i, "id")?.to_owned(),
                    user: false,
                    text: str_at(i, "text").unwrap_or_default().to_owned(),
                })
            })
            .collect();
        return Reconciliation::Found { turn: str_at(turn, "id").unwrap_or_default().to_owned(), outcome, items };
    }
    Reconciliation::NotFound
}

/// Turns newer than `latest`, oldest first, as transcript items.
fn missed_turns(value: &Value, latest: Option<&str>) -> (Option<String>, Vec<ImportedItem>) {
    let turns: Vec<&Value> = value.get("data").and_then(Value::as_array).into_iter().flatten().collect();
    let newest = turns.first().and_then(|t| str_at(t, "id")).map(str::to_owned);
    // Without a baseline Bukno cannot tell its own turns from outside ones.
    let Some(latest) = latest else {
        return (None, Vec::new());
    };
    if newest.as_deref() == Some(latest) || newest.is_none() {
        return (None, Vec::new());
    }
    let mut missed = Vec::new();
    for turn in &turns {
        if str_at(turn, "id") == Some(latest) {
            break;
        }
        missed.push(*turn);
    }
    let mut items = Vec::new();
    for turn in missed.into_iter().rev() {
        for item in turn.get("items").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = str_at(item, "id") else {
                continue;
            };
            match str_at(item, "type") {
                Some("userMessage") => {
                    let text: Vec<&str> = item
                        .get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|c| str_at(c, "text"))
                        .collect();
                    items.push(ImportedItem { provider_item: id.to_owned(), user: true, text: text.join("\n") });
                }
                Some("agentMessage") => items.push(ImportedItem {
                    provider_item: id.to_owned(),
                    user: false,
                    text: str_at(item, "text").unwrap_or_default().to_owned(),
                }),
                _ => {}
            }
        }
    }
    (newest, items)
}

/// What an item says the agent is doing, in words. Only real events.
fn activity_words(item: &Value) -> Option<String> {
    Some(match str_at(item, "type")? {
        "commandExecution" => {
            let command = str_at(item, "command").map(protocol::display_command).unwrap_or_default();
            let short: String = command.chars().take(80).collect();
            format!("Running `{short}`")
        }
        "fileChange" => {
            let files: Vec<String> = item
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|c| str_at(c, "path"))
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
                .collect();
            match files.as_slice() {
                [] => "Editing files".into(),
                [one] => format!("Editing {one}"),
                many => format!("Editing {} files", many.len()),
            }
        }
        "mcpToolCall" => {
            format!("Using {} {}", str_at(item, "server").unwrap_or("a tool"), str_at(item, "tool").unwrap_or_default())
                .trim_end()
                .to_owned()
        }
        "webSearch" => "Searching the web".into(),
        "reasoning" => "Thinking".into(),
        "agentMessage" => "Writing".into(),
        "imageView" => "Looking at an image".into(),
        "contextCompaction" => "Compacting the conversation".into(),
        "collabAgentToolCall" | "subAgentActivity" => "Working with a helper agent".into(),
        _ => return None,
    })
}

fn request_words(method: &str) -> &str {
    match method {
        "mcpServer/elicitation/request" => "input for an MCP tool",
        "item/permissions/requestApproval" => "extra permissions",
        "item/tool/call" => "a tool call that runs inside Bukno",
        "account/chatgptAuthTokens/refresh" => "a sign-in refresh",
        "attestation/generate" => "an attestation",
        other => other,
    }
}

/// Codex sometimes wraps an API error as JSON text; show the human message.
fn readable_error(message: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(message)
        && let Some(inner) = value.get("error").and_then(|e| str_at(e, "message")).or_else(|| str_at(&value, "message"))
    {
        return inner.to_owned();
    }
    message.to_owned()
}

fn capitalize(word: &str) -> String {
    match word {
        "xhigh" => "Extra high".into(),
        _ => {
            let mut chars = word.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

fn describe_exit(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("ended by signal {signal}");
        }
    }
    match status.code() {
        Some(0) => "it exited normally".into(),
        Some(code) => format!("exit code {code}"),
        None => "it exited".into(),
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
