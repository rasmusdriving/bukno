//! T3's socket protocol: Effect RPC messages as JSON over a WebSocket.
//!
//! Wire format from `effect/unstable/rpc/RpcMessage.ts` and
//! `RpcSerialization.layerJson` (Effect 4.0.0-rc.115, with T3's patch):
//!
//! - One text frame holds one JSON message or an array of messages.
//! - Client to server: `Request {id, tag, payload, headers}`, `Ack {requestId}`,
//!   `Interrupt {requestId, interruptors}`, `Ping`, `Eof`.
//! - Server to client: `Chunk {requestId, values}`, `Exit {requestId, exit}`,
//!   `Defect {defect}`, `Pong`, `ClientProtocolError {error}`.
//! - A stream sends one `Chunk` and waits for the client's `Ack` before the
//!   next. The client pings every 5 s; 3 missed pongs mean the socket is dead.
//!
//! The only requests this module can send are the methods in [`Method`]: the
//! three reads, plus the two command methods that need the
//! `orchestration:operate` scope. The frame writer refuses any other tag.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::{self, Message};
use url::Url;

use crate::Log;
use crate::error::T3Error;

const PING_EVERY: Duration = Duration::from_secs(5);
const MISSED_PONGS_LIMIT: u32 = 3;
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);

/// Every RPC method this client may call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    GetConfig,
    SubscribeShell,
    SubscribeThread,
    /// One command from [`crate::command`]. Needs `orchestration:operate`.
    DispatchCommand,
    /// A new chat with its first message. Needs `orchestration:operate`.
    LaunchThread,
}

impl Method {
    pub const ALL: [Self; 5] =
        [Self::GetConfig, Self::SubscribeShell, Self::SubscribeThread, Self::DispatchCommand, Self::LaunchThread];

    pub fn tag(self) -> &'static str {
        match self {
            Self::GetConfig => "server.getConfig",
            Self::SubscribeShell => "orchestration.subscribeShell",
            Self::SubscribeThread => "orchestration.subscribeThread",
            Self::DispatchCommand => "orchestration.dispatchCommand",
            Self::LaunchThread => "orchestration.launchThread",
        }
    }

    /// Whether the method changes anything on the server.
    pub fn operates(self) -> bool {
        matches!(self, Self::DispatchCommand | Self::LaunchThread)
    }
}

/// Counters for one connection, kept for evidence and the status screen.
#[derive(Debug, Default)]
pub struct SessionStats {
    pub frames_in: AtomicU64,
    pub chunks_in: AtomicU64,
    pub acks_out: AtomicU64,
    pub requests_out: AtomicU64,
    pub unknown_frames: AtomicU64,
}

pub enum StreamEvent {
    /// One chunk. Call [`Subscription::ack`] after applying it.
    Values(Vec<Value>),
    /// The server ended the stream normally.
    End,
    Failed(T3Error),
}

enum Command {
    Call { method: Method, payload: Value, reply: oneshot::Sender<Result<Value, T3Error>> },
    Subscribe { method: Method, payload: Value, events: mpsc::UnboundedSender<StreamEvent>, id: oneshot::Sender<u64> },
    Ack { id: u64 },
    Interrupt { id: u64 },
}

enum Pending {
    Call(oneshot::Sender<Result<Value, T3Error>>),
    Stream(mpsc::UnboundedSender<StreamEvent>),
}

/// One open socket. Dropping it closes the socket.
pub struct Session {
    commands: mpsc::UnboundedSender<Command>,
    /// Becomes `Some(reason)` when the socket closes.
    pub closed: watch::Receiver<Option<String>>,
    pub stats: Arc<SessionStats>,
}

/// A cloneable handle for one-shot calls on a session, so a command can wait
/// for its reply in its own task while the streams keep flowing.
#[derive(Clone)]
pub struct Caller {
    commands: mpsc::UnboundedSender<Command>,
}

impl Caller {
    pub async fn call(&self, method: Method, payload: Value) -> Result<Value, T3Error> {
        call(&self.commands, method, payload).await
    }
}

async fn call(commands: &mpsc::UnboundedSender<Command>, method: Method, payload: Value) -> Result<Value, T3Error> {
    let (reply, rx) = oneshot::channel();
    commands
        .send(Command::Call { method, payload, reply })
        .map_err(|_| T3Error::Disconnected { detail: "the socket is closed".into() })?;
    rx.await.map_err(|_| T3Error::Disconnected { detail: "the socket closed before the reply".into() })?
}

/// A live stream. Dropping it sends `Interrupt` if the stream is still open.
pub struct Subscription {
    pub method: Method,
    id: u64,
    events: mpsc::UnboundedReceiver<StreamEvent>,
    commands: mpsc::UnboundedSender<Command>,
    ended: bool,
}

impl Subscription {
    pub async fn next(&mut self) -> Option<StreamEvent> {
        let event = self.events.recv().await;
        if matches!(event, None | Some(StreamEvent::End | StreamEvent::Failed(_))) {
            self.ended = true;
        }
        event
    }

    /// Tell the server the last chunk was applied, so it sends the next one.
    pub fn ack(&self) {
        let _ = self.commands.send(Command::Ack { id: self.id });
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if !self.ended {
            let _ = self.commands.send(Command::Interrupt { id: self.id });
        }
    }
}

/// Map a refused WebSocket upgrade to what it means.
fn upgrade_error(error: tungstenite::Error) -> T3Error {
    match error {
        tungstenite::Error::Http(response) => {
            let status = response.status().as_u16();
            let body: Option<Value> = response.body().as_deref().and_then(|b| serde_json::from_slice(b).ok());
            match status {
                426 => T3Error::ProtocolMismatch {
                    server: body
                        .as_ref()
                        .and_then(|b| b.get("orchestrationProtocolVersion"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                },
                401 | 403 => T3Error::SignInRejected,
                _ => T3Error::Server { status, detail: "the socket upgrade was refused".into() },
            }
        }
        tungstenite::Error::Io(e) => T3Error::Unreachable { detail: e.to_string() },
        other => T3Error::Disconnected { detail: other.to_string() },
    }
}

/// Describe an `Exit` failure cause in a few words.
fn failure_words(exit: &Value) -> String {
    let Some(causes) = exit.get("cause").and_then(Value::as_array) else {
        return "unknown failure".into();
    };
    causes
        .iter()
        .map(|c| match c.get("_tag").and_then(Value::as_str) {
            Some("Fail") => {
                let error = c.get("error").cloned().unwrap_or_default();
                let tag = error.get("_tag").and_then(Value::as_str).unwrap_or("error");
                match error.get("message").and_then(Value::as_str) {
                    Some(message) => format!("{tag}: {message}"),
                    None => tag.to_owned(),
                }
            }
            Some("Die") => "server defect".to_owned(),
            Some("Interrupt") => "interrupted".to_owned(),
            _ => "unknown failure".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn is_auth_failure(exit: &Value) -> bool {
    exit.get("cause").and_then(Value::as_array).is_some_and(|causes| {
        causes.iter().any(|c| {
            c.get("error").and_then(|e| e.get("_tag")).and_then(Value::as_str) == Some("EnvironmentAuthorizationError")
        })
    })
}

fn request_id(value: &Value) -> Option<u64> {
    match value.get("requestId")? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

impl Session {
    /// Open the socket. `url` carries a one-time ticket; it is never logged.
    pub async fn connect(url: &Url, log: Log) -> Result<Self, T3Error> {
        let (socket, _) = tokio::time::timeout(OPEN_TIMEOUT, tokio_tungstenite::connect_async(url.as_str()))
            .await
            .map_err(|_| T3Error::Unreachable { detail: "the socket did not open within 15 seconds".into() })?
            .map_err(upgrade_error)?;
        let (commands, inbox) = mpsc::unbounded_channel();
        let (closed_tx, closed) = watch::channel(None);
        let stats = Arc::new(SessionStats::default());
        tokio::spawn(run(socket, inbox, closed_tx, stats.clone(), log));
        Ok(Self { commands, closed, stats })
    }

    pub async fn call(&self, method: Method, payload: Value) -> Result<Value, T3Error> {
        call(&self.commands, method, payload).await
    }

    pub fn caller(&self) -> Caller {
        Caller { commands: self.commands.clone() }
    }

    pub async fn subscribe(&self, method: Method, payload: Value) -> Result<Subscription, T3Error> {
        let (events_tx, events) = mpsc::unbounded_channel();
        let (id_tx, id_rx) = oneshot::channel();
        self.commands
            .send(Command::Subscribe { method, payload, events: events_tx, id: id_tx })
            .map_err(|_| T3Error::Disconnected { detail: "the socket is closed".into() })?;
        let id = id_rx.await.map_err(|_| T3Error::Disconnected { detail: "the socket is closed".into() })?;
        Ok(Subscription { method, id, events, commands: self.commands.clone(), ended: false })
    }

    pub fn closed_reason(&self) -> Option<String> {
        self.closed.borrow().clone()
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn run(
    socket: Socket,
    mut inbox: mpsc::UnboundedReceiver<Command>,
    closed: watch::Sender<Option<String>>,
    stats: Arc<SessionStats>,
    log: Log,
) {
    let (mut sink, mut stream) = socket.split();
    let mut pending: HashMap<u64, (Method, Pending)> = HashMap::new();
    let mut next_id: u64 = 0;
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await;
    let mut awaiting_pong = false;
    let mut missed_pongs = 0;

    let reason: String = loop {
        tokio::select! {
            command = inbox.recv() => {
                let Some(command) = command else { break "closed by Bukno".into() };
                let frame = match command {
                    Command::Call { method, payload, reply } => {
                        let id = next_id;
                        next_id += 1;
                        pending.insert(id, (method, Pending::Call(reply)));
                        request(id, method, payload, &log)
                    }
                    Command::Subscribe { method, payload, events, id: id_reply } => {
                        let id = next_id;
                        next_id += 1;
                        pending.insert(id, (method, Pending::Stream(events)));
                        let _ = id_reply.send(id);
                        request(id, method, payload, &log)
                    }
                    Command::Ack { id } => {
                        stats.acks_out.fetch_add(1, Ordering::Relaxed);
                        Some(json!({"_tag": "Ack", "requestId": id}))
                    }
                    Command::Interrupt { id } => {
                        if pending.remove(&id).is_none() {
                            continue;
                        }
                        log(&format!("rpc: interrupt request {id}"));
                        Some(json!({"_tag": "Interrupt", "requestId": id, "interruptors": []}))
                    }
                };
                let Some(frame) = frame else { continue };
                if matches!(frame.get("_tag").and_then(Value::as_str), Some("Request")) {
                    stats.requests_out.fetch_add(1, Ordering::Relaxed);
                }
                if let Err(e) = sink.send(Message::text(frame.to_string())).await {
                    break format!("write failed: {e}");
                }
            }
            incoming = stream.next() => {
                let text = match incoming {
                    None => break "the server closed the socket".into(),
                    Some(Err(e)) => break format!("socket error: {e}"),
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(frame))) => {
                        break frame.map_or("the server closed the socket".into(), |f| format!("the server closed the socket ({} {})", u16::from(f.code), f.reason));
                    }
                    Some(Ok(Message::Binary(_))) => {
                        stats.unknown_frames.fetch_add(1, Ordering::Relaxed);
                        log("rpc: ignored a binary frame");
                        continue;
                    }
                    // Socket-level pings are answered by tungstenite.
                    Some(Ok(_)) => continue,
                };
                stats.frames_in.fetch_add(1, Ordering::Relaxed);
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => break format!("T3 sent a frame that is not JSON ({e})"),
                };
                let messages = match parsed {
                    Value::Array(list) => list,
                    single => vec![single],
                };
                for message in messages {
                    match message.get("_tag").and_then(Value::as_str) {
                        Some("Pong") => {
                            awaiting_pong = false;
                            missed_pongs = 0;
                        }
                        Some("Chunk") => {
                            stats.chunks_in.fetch_add(1, Ordering::Relaxed);
                            let Some(id) = request_id(&message) else { continue };
                            let values = match message.get("values") {
                                Some(Value::Array(values)) => values.clone(),
                                _ => continue,
                            };
                            if let Some((_, Pending::Stream(tx))) = pending.get(&id)
                                && tx.send(StreamEvent::Values(values)).is_err()
                            {
                                // The consumer is gone: stop the stream.
                                pending.remove(&id);
                                let frame = json!({"_tag": "Interrupt", "requestId": id, "interruptors": []});
                                let _ = sink.send(Message::text(frame.to_string())).await;
                            }
                        }
                        Some("Exit") => {
                            let Some(id) = request_id(&message) else { continue };
                            let Some((method, entry)) = pending.remove(&id) else { continue };
                            let exit = message.get("exit").cloned().unwrap_or_default();
                            let success = exit.get("_tag").and_then(Value::as_str) == Some("Success");
                            let failure = || if is_auth_failure(&exit) {
                                T3Error::SignInRejected
                            } else {
                                T3Error::Rpc { detail: format!("{}: {}", method.tag(), failure_words(&exit)) }
                            };
                            match entry {
                                Pending::Call(reply) => {
                                    let _ = reply.send(if success { Ok(exit.get("value").cloned().unwrap_or_default()) } else { Err(failure()) });
                                }
                                Pending::Stream(tx) => {
                                    let _ = tx.send(if success { StreamEvent::End } else { StreamEvent::Failed(failure()) });
                                }
                            }
                        }
                        Some("Defect") | Some("ClientProtocolError") => {
                            log(&format!("rpc: server reported {}; failing open requests", message["_tag"]));
                            for (_, (method, entry)) in pending.drain() {
                                let error = T3Error::Rpc { detail: format!("{}: server defect", method.tag()) };
                                match entry {
                                    Pending::Call(reply) => { let _ = reply.send(Err(error)); }
                                    Pending::Stream(tx) => { let _ = tx.send(StreamEvent::Failed(error)); }
                                }
                            }
                        }
                        other => {
                            stats.unknown_frames.fetch_add(1, Ordering::Relaxed);
                            log(&format!("rpc: ignored a frame with unknown tag {other:?}"));
                        }
                    }
                }
            }
            _ = ping.tick() => {
                if awaiting_pong {
                    missed_pongs += 1;
                    if missed_pongs >= MISSED_PONGS_LIMIT {
                        break format!("no reply from T3 for {} seconds", PING_EVERY.as_secs() * u64::from(MISSED_PONGS_LIMIT));
                    }
                }
                awaiting_pong = true;
                if let Err(e) = sink.send(Message::text(json!({"_tag": "Ping"}).to_string())).await {
                    break format!("write failed: {e}");
                }
            }
        }
    };

    log(&format!("rpc: socket closed: {reason}"));
    for (_, (_, entry)) in pending.drain() {
        let error = T3Error::Disconnected { detail: reason.clone() };
        match entry {
            Pending::Call(reply) => {
                let _ = reply.send(Err(error));
            }
            Pending::Stream(tx) => {
                let _ = tx.send(StreamEvent::Failed(error));
            }
        }
    }
    let _ = sink.close().await;
    let _ = closed.send(Some(reason));
}

/// Build a request frame. Refuses any tag outside [`Method`], which the types
/// already rule out; this is the last check before the socket. Command
/// payloads carry chat text, so only their type and command ID are logged.
fn request(id: u64, method: Method, payload: Value, log: &Log) -> Option<Value> {
    let tag = method.tag();
    if !Method::ALL.iter().any(|m| m.tag() == tag) {
        log(&format!("rpc: refused to send {tag}, which is not a known method"));
        return None;
    }
    if method.operates() {
        let kind = payload.get("type").and_then(Value::as_str).unwrap_or("launch");
        let command = payload.get("commandId").and_then(Value::as_str).unwrap_or("?");
        log(&format!("rpc: send request {id} {tag} {kind} command {command}"));
    } else {
        log(&format!("rpc: send request {id} {tag} {payload}"));
    }
    Some(json!({"_tag": "Request", "id": id, "tag": tag, "payload": payload, "headers": []}))
}
