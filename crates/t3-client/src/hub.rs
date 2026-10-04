//! Paired environments, each with one supervisor task that connects, keeps
//! the chat list and the open chat current, and reconnects after failures.
//!
//! The UI never touches the network. It sends commands to the hub and reads
//! the latest [`HubView`], which the tasks replace whenever something shown
//! changes, then call `notify` so the window repaints.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use url::Url;

use crate::command::{CommandRequest, Outgoing};
use crate::error::T3Error;
use crate::http::Http;
use crate::model::{ProjectShell, Provider, ServerConfig, ShellItem, ThreadItem, ThreadShell};
use crate::pairing::{parse_pairing, socket_url};
use crate::rpc::{Method, Session, StreamEvent, Subscription};
use crate::secret::TokenVault;
use crate::shell::{ShellState, StreamCounts};
use crate::store::{EnvironmentStore, SavedEnvironment};
use crate::thread::{PendingRequest, QueuedMessage, Row, ThreadState};
use crate::{Log, time};

const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(10);
/// Thread stream failures in a row on one socket before reconnecting it.
const THREAD_RETRY_LIMIT: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connecting,
    /// Connected and subscribed. `current` is true after the server's
    /// catch-up marker for the chat list.
    Connected {
        current: bool,
    },
    Reconnecting {
        attempt: u32,
        reason: String,
        retry_in_ms: u64,
    },
    /// The sign-in is missing, expired or revoked.
    NeedsPairing {
        reason: String,
    },
    /// Retrying cannot help (wrong server, protocol mismatch). Waits for the user.
    Blocked {
        reason: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PairingStatus {
    #[default]
    Idle,
    Working,
    Failed(String),
    Paired {
        environment_id: String,
        label: String,
    },
}

#[derive(Clone, Debug)]
pub struct ThreadView {
    pub thread_id: String,
    pub title: String,
    /// Visible rows, oldest first.
    pub rows: Vec<Arc<Row>>,
    pub revision: u64,
    pub loaded: bool,
    pub current: bool,
    pub has_more_history: bool,
    pub loading_history: bool,
    pub history_error: Option<String>,
    pub removed: bool,
    pub working: bool,
    pub counts: StreamCounts,
    pub unknown_event_types: Vec<String>,
    pub error: Option<String>,
    /// The last event sequence applied: where a resume continues from.
    pub last_sequence: Option<u64>,
    /// The running turn, which Stop and Steer target.
    pub active_run: Option<String>,
    /// Approvals and questions waiting, oldest first.
    pub pending: Vec<PendingRequest>,
    /// Messages waiting for the running turn, in order.
    pub queued: Vec<QueuedMessage>,
    /// Status of each run by ID, for labels.
    pub run_status: BTreeMap<String, String>,
}

/// Where a command stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutboxState {
    /// Written to the socket; no reply yet.
    Sending,
    /// T3 accepted it, or its effect was seen in the chat after a lost reply.
    /// A launch carries the new chat's ID.
    Accepted { observed: bool, thread_id: Option<String> },
    /// Refused, by T3 or before sending. The text says why; nothing ran.
    Rejected(String),
    /// The connection dropped before T3 answered, and the chat does not show
    /// it yet. Sending it again is safe: T3 runs one command ID at most once.
    Unconfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxEntry {
    pub request: CommandRequest,
    pub state: OutboxState,
}

/// Commands kept for the view; older settled ones are dropped.
const OUTBOX_KEEP: usize = 64;

#[derive(Clone, Debug)]
pub struct EnvironmentView {
    pub saved: SavedEnvironment,
    pub status: ConnectionStatus,
    pub server_version: Option<String>,
    pub projects: Vec<ProjectShell>,
    /// Active chats, most recent first.
    pub threads: Vec<ThreadShell>,
    pub shell_counts: StreamCounts,
    pub providers: Vec<Provider>,
    pub thread: Option<ThreadView>,
    /// Commands sent from this computer, oldest first.
    pub outbox: Vec<OutboxEntry>,
    /// Successful socket connections since Bukno started.
    pub connections: u64,
    pub requests_sent: u64,
    pub unknown_frames: u64,
}

#[derive(Clone, Debug, Default)]
pub struct HubView {
    /// Increases whenever anything below changes.
    pub revision: u64,
    pub environments: Vec<Arc<EnvironmentView>>,
    pub pairing: PairingStatus,
    pub store_error: Option<String>,
}

enum EnvCommand {
    Dispatch(CommandRequest),
    Retry(String),
    DismissCommand(String),
    OpenThread(String),
    CloseThread,
    LoadOlder,
    Reconnect,
    Forget,
}

struct Shared {
    notify: Arc<dyn Fn() + Send + Sync>,
    log: Log,
    vault: Arc<dyn TokenVault>,
    store: EnvironmentStore,
    http: Http,
    saved: Mutex<Vec<SavedEnvironment>>,
    views: Mutex<BTreeMap<String, Arc<EnvironmentView>>>,
    pairing: Mutex<PairingStatus>,
    store_error: Mutex<Option<String>>,
    revision: AtomicU64,
    /// The running task of each environment, with its generation.
    tasks: Mutex<BTreeMap<String, (u64, mpsc::UnboundedSender<EnvCommand>)>>,
    next_generation: AtomicU64,
}

impl Shared {
    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        (self.notify)();
    }

    /// Replace an environment's view, unless `generation` is no longer its
    /// running task (it was forgotten or paired again).
    ///
    /// The task lock is held while the view is inserted, so a concurrent
    /// forget cannot slip in between the check and the insert. Lock order is
    /// always `tasks`, then `views`.
    fn publish(&self, generation: u64, view: EnvironmentView) {
        {
            let tasks = self.tasks.lock().unwrap();
            if !tasks.get(&view.saved.environment_id).is_some_and(|(g, _)| *g == generation) {
                return;
            }
            self.views.lock().unwrap().insert(view.saved.environment_id.clone(), Arc::new(view));
        }
        self.changed();
    }

    fn set_pairing(&self, status: PairingStatus) {
        *self.pairing.lock().unwrap() = status;
        self.changed();
    }
}

pub struct Hub {
    runtime: tokio::runtime::Runtime,
    shared: Arc<Shared>,
}

impl Hub {
    /// Load saved environments from `state_dir` and start connecting to each.
    pub fn start(state_dir: &Path, vault: Arc<dyn TokenVault>, notify: Arc<dyn Fn() + Send + Sync>, log: Log) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("bukno-t3")
            .enable_all()
            .build()
            .expect("T3 client runtime");
        let store = EnvironmentStore::new(state_dir.join("t3-environments.json"));
        let (saved, store_error) = match store.load() {
            Ok(saved) => (saved, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let shared = Arc::new(Shared {
            notify,
            log,
            vault,
            store,
            http: Http::new(),
            saved: Mutex::new(saved.clone()),
            views: Mutex::new(BTreeMap::new()),
            pairing: Mutex::new(PairingStatus::Idle),
            store_error: Mutex::new(store_error),
            revision: AtomicU64::new(1),
            tasks: Mutex::new(BTreeMap::new()),
            next_generation: AtomicU64::new(1),
        });
        let hub = Self { runtime, shared };
        for environment in saved {
            spawn_environment(&hub.shared, hub.runtime.handle(), environment, None);
        }
        hub
    }

    pub fn view(&self) -> HubView {
        HubView {
            revision: self.shared.revision.load(Ordering::SeqCst),
            environments: self.shared.views.lock().unwrap().values().cloned().collect(),
            pairing: self.shared.pairing.lock().unwrap().clone(),
            store_error: self.shared.store_error.lock().unwrap().clone(),
        }
    }

    pub fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::SeqCst)
    }

    fn send(&self, environment_id: &str, command: EnvCommand) {
        if let Some((_, tx)) = self.shared.tasks.lock().unwrap().get(environment_id) {
            let _ = tx.send(command);
        }
    }

    pub fn open_thread(&self, environment_id: &str, thread_id: &str) {
        self.send(environment_id, EnvCommand::OpenThread(thread_id.to_owned()));
    }

    pub fn close_thread(&self, environment_id: &str) {
        self.send(environment_id, EnvCommand::CloseThread);
    }

    pub fn load_older(&self, environment_id: &str) {
        self.send(environment_id, EnvCommand::LoadOlder);
    }

    /// Send a command. Its progress shows in [`EnvironmentView::outbox`].
    pub fn dispatch(&self, environment_id: &str, request: CommandRequest) {
        self.send(environment_id, EnvCommand::Dispatch(request));
    }

    /// Send an unconfirmed command again, with the same command ID.
    pub fn retry(&self, environment_id: &str, command_id: &str) {
        self.send(environment_id, EnvCommand::Retry(command_id.to_owned()));
    }

    /// Stop showing a command's notice.
    pub fn dismiss_command(&self, environment_id: &str, command_id: &str) {
        self.send(environment_id, EnvCommand::DismissCommand(command_id.to_owned()));
    }

    /// Try again now, after a block or during a backoff wait.
    pub fn reconnect(&self, environment_id: &str) {
        self.send(environment_id, EnvCommand::Reconnect);
    }

    /// Remove an environment and its keychain entry.
    pub fn forget(&self, environment_id: &str) {
        self.send(environment_id, EnvCommand::Forget);
        {
            let mut tasks = self.shared.tasks.lock().unwrap();
            tasks.remove(environment_id);
            self.shared.views.lock().unwrap().remove(environment_id);
        }
        let remaining: Vec<SavedEnvironment> = {
            let mut saved = self.shared.saved.lock().unwrap();
            saved.retain(|e| e.environment_id != environment_id);
            saved.clone()
        };
        if let Err(e) = self.shared.store.save(&remaining) {
            *self.shared.store_error.lock().unwrap() = Some(e);
        }
        if let Err(e) = self.shared.vault.delete(environment_id) {
            (self.shared.log)(&format!("hub: could not remove the keychain entry: {e}"));
        }
        self.shared.changed();
    }

    /// Pair with a server. The result arrives as [`HubView::pairing`].
    pub fn pair(&self, address: &str, link: &str, label: &str) {
        let request = match parse_pairing(Some(address), link) {
            Ok(request) => request,
            Err(e) => return self.shared.set_pairing(PairingStatus::Failed(e.user_message())),
        };
        self.shared.set_pairing(PairingStatus::Working);
        let shared = self.shared.clone();
        let label = label.to_owned();
        self.runtime.spawn(async move {
            let log = shared.log.clone();
            log(&format!("pair: checking {}", request.base));
            let result: Result<SavedEnvironment, T3Error> = async {
                let descriptor = shared.http.descriptor(&request.base).await?;
                log(&format!(
                    "pair: {} is {} ({}), server {}",
                    request.base, descriptor.label, descriptor.environment_id, descriptor.server_version
                ));
                let access = shared.http.exchange(&request.base, &request.credential, &label).await?;
                shared.vault.save(&descriptor.environment_id, &access.token)?;
                Ok(SavedEnvironment {
                    environment_id: descriptor.environment_id,
                    label: descriptor.label,
                    address: request.base.to_string(),
                    server_version: descriptor.server_version,
                    token_expires_at: access.expires_at_epoch,
                    scope: access.scope,
                    paired_at: time::now_epoch_secs(),
                })
            }
            .await;
            let saved = match result {
                Ok(saved) => saved,
                Err(e) => {
                    log(&format!("pair: failed: {e:?}"));
                    return shared.set_pairing(PairingStatus::Failed(e.user_message()));
                }
            };
            log(&format!("pair: paired with {} ({}), scope {}", saved.label, saved.environment_id, saved.scope));
            let all: Vec<SavedEnvironment> = {
                let mut list = shared.saved.lock().unwrap();
                list.retain(|e| e.environment_id != saved.environment_id);
                list.push(saved.clone());
                list.clone()
            };
            if let Err(e) = shared.store.save(&all) {
                let message = format!("Paired, but the environment list could not be saved: {e}");
                return shared.set_pairing(PairingStatus::Failed(message));
            }
            // Re-pairing replaces the running task, which then uses the new
            // sign-in. The chat open on screen stays open on the new task.
            let open = shared
                .views
                .lock()
                .unwrap()
                .get(&saved.environment_id)
                .and_then(|v| v.thread.as_ref().map(|t| t.thread_id.clone()));
            if let Some((_, previous)) = shared.tasks.lock().unwrap().remove(&saved.environment_id) {
                let _ = previous.send(EnvCommand::Forget);
            }
            spawn_environment(&shared, &tokio::runtime::Handle::current(), saved.clone(), open);
            shared.set_pairing(PairingStatus::Paired { environment_id: saved.environment_id, label: saved.label });
        });
    }

    pub fn clear_pairing_status(&self) {
        self.shared.set_pairing(PairingStatus::Idle);
    }
}

fn spawn_environment(
    shared: &Arc<Shared>,
    handle: &tokio::runtime::Handle,
    saved: SavedEnvironment,
    open_thread: Option<String>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let generation = shared.next_generation.fetch_add(1, Ordering::SeqCst);
    shared.tasks.lock().unwrap().insert(saved.environment_id.clone(), (generation, tx));
    shared.publish(generation, EnvironmentView::new(saved.clone()));
    handle.spawn(run_environment(shared.clone(), generation, saved, open_thread, rx));
}

impl EnvironmentView {
    fn new(saved: SavedEnvironment) -> Self {
        Self {
            saved,
            status: ConnectionStatus::Connecting,
            server_version: None,
            projects: Vec::new(),
            threads: Vec::new(),
            shell_counts: StreamCounts::default(),
            providers: Vec::new(),
            thread: None,
            outbox: Vec::new(),
            connections: 0,
            requests_sent: 0,
            unknown_frames: 0,
        }
    }

    /// Whether this sign-in may send, answer and stop.
    pub fn can_operate(&self) -> bool {
        self.saved.can_operate()
    }

    /// Connected, caught up, and allowed to send.
    pub fn ready_to_send(&self) -> bool {
        self.can_operate() && self.status == ConnectionStatus::Connected { current: true }
    }

    pub fn command(&self, command_id: &str) -> Option<&OutboxEntry> {
        self.outbox.iter().find(|e| e.request.command_id == command_id)
    }
}

/// Sort key matching T3's default "latest user message" order.
fn activity_key(thread: &ThreadShell) -> &str {
    thread.latest_user_message_at.as_deref().unwrap_or(&thread.updated_at)
}

/// State of one environment's task, turned into views.
struct EnvState {
    generation: u64,
    saved: SavedEnvironment,
    status: ConnectionStatus,
    server_version: Option<String>,
    shell: ShellState,
    providers: Vec<Provider>,
    thread: Option<ThreadState>,
    thread_error: Option<String>,
    /// The history page being fetched: (chat load, cursor). It counts as
    /// loading only while it still matches the open chat, so a reload or a
    /// new snapshot never leaves paging stuck.
    history_pending: Option<(u64, String)>,
    history_error: Option<String>,
    outbox: Vec<OutboxEntry>,
    connections: u64,
    requests_sent: u64,
    unknown_frames: u64,
}

impl EnvState {
    fn entry(&mut self, command_id: &str) -> Option<&mut OutboxEntry> {
        self.outbox.iter_mut().find(|e| e.request.command_id == command_id)
    }

    fn record(&mut self, request: CommandRequest, state: OutboxState) {
        self.outbox.retain(|e| e.request.command_id != request.command_id);
        self.outbox.push(OutboxEntry { request, state });
        // Drop the oldest settled entries; never one still in doubt.
        while self.outbox.len() > OUTBOX_KEEP {
            let Some(at) =
                self.outbox.iter().position(|e| !matches!(e.state, OutboxState::Sending | OutboxState::Unconfirmed))
            else {
                break;
            };
            self.outbox.remove(at);
        }
    }

    /// The connection is gone: any command without a reply is in doubt.
    fn connection_lost(&mut self) {
        for entry in &mut self.outbox {
            if entry.state == OutboxState::Sending {
                entry.state = OutboxState::Unconfirmed;
            }
        }
    }

    /// Settle commands whose effect the caught-up chat or chat list already
    /// shows, whether or not T3's reply has arrived, so a slow or lost reply
    /// never holds up what the user can already see. Only
    /// exact evidence counts: the message or chat ID Bukno chose, or the
    /// request, run or queue it acted on having moved on. A reply arriving
    /// later is ignored. Returns true when anything changed.
    fn reconcile(&mut self) -> bool {
        let thread = self.thread.as_ref().filter(|t| t.synchronized);
        let shell_current = self.shell.synchronized;
        let mut changed = false;
        for entry in &mut self.outbox {
            if !matches!(entry.state, OutboxState::Sending | OutboxState::Unconfirmed) {
                continue;
            }
            let request = &entry.request;
            let here = thread.filter(|t| t.thread_id == request.thread_id());
            let done = match &request.outgoing {
                Outgoing::Launch { thread_id, .. } => shell_current && self.shell.threads.contains_key(thread_id),
                Outgoing::Send { message_id, .. } => here.is_some_and(|t| t.has_message(message_id)),
                Outgoing::Approve { request_id, .. }
                | Outgoing::Answer { request_id, .. }
                | Outgoing::Dismiss { request_id, .. } => {
                    here.is_some_and(|t| t.request_status(request_id).is_some_and(|s| s != "pending"))
                }
                Outgoing::Interrupt { run_id, .. } => here.is_some_and(|t| {
                    t.run_status(run_id).is_some_and(|s| !matches!(s, "preparing" | "starting" | "running" | "waiting"))
                }),
                Outgoing::PromoteQueued { queued_run_id: run_id, .. } | Outgoing::CancelQueued { run_id, .. } => {
                    here.is_some_and(|t| t.run_status(run_id).is_some_and(|s| s != "queued"))
                }
                Outgoing::ResumeQueue { .. } => here.is_some_and(|t| t.queued().iter().all(|q| !q.held)),
                Outgoing::SetRuntimeMode { thread_id, runtime_mode } => {
                    shell_current
                        && self
                            .shell
                            .threads
                            .get(thread_id)
                            .is_some_and(|t| t.runtime_mode.as_deref() == Some(runtime_mode))
                }
            };
            if done {
                let thread_id =
                    matches!(request.outgoing, Outgoing::Launch { .. }).then(|| request.thread_id().to_owned());
                entry.state = OutboxState::Accepted { observed: true, thread_id };
                changed = true;
            }
        }
        changed
    }

    fn history_in_flight(&self) -> bool {
        match (&self.history_pending, &self.thread) {
            (Some((incarnation, cursor)), Some(thread)) => {
                thread.incarnation == *incarnation && thread.history_cursor.as_deref() == Some(cursor.as_str())
            }
            _ => false,
        }
    }

    fn view(&self) -> EnvironmentView {
        let mut projects: Vec<ProjectShell> = self.shell.projects.values().cloned().collect();
        projects.sort_by_key(|p| p.title.to_lowercase());
        let mut threads: Vec<ThreadShell> = self.shell.threads.values().cloned().collect();
        threads.sort_by(|a, b| activity_key(b).cmp(activity_key(a)).then_with(|| b.id.cmp(&a.id)));
        let status = match &self.status {
            ConnectionStatus::Connected { .. } => ConnectionStatus::Connected { current: self.shell.synchronized },
            other => other.clone(),
        };
        EnvironmentView {
            saved: self.saved.clone(),
            status,
            server_version: self.server_version.clone(),
            projects,
            threads,
            shell_counts: self.shell.counts.clone(),
            providers: self.providers.clone(),
            thread: self.thread.as_ref().map(|t| ThreadView {
                thread_id: t.thread_id.clone(),
                title: t.title.clone(),
                rows: t.visible_rows(),
                revision: t.revision,
                loaded: t.loaded(),
                current: t.synchronized,
                has_more_history: t.has_more_history,
                loading_history: self.history_in_flight(),
                history_error: self.history_error.clone(),
                removed: t.removed,
                working: t.working(),
                counts: t.counts.clone(),
                unknown_event_types: t.unknown_event_types.iter().cloned().collect(),
                error: self.thread_error.clone(),
                last_sequence: t.last_sequence,
                active_run: t.active_run().map(|r| r.id.clone()),
                pending: t.pending_requests(),
                queued: t.queued(),
                run_status: t.runs().map(|r| (r.id.clone(), r.status.clone())).collect(),
            }),
            outbox: self.outbox.clone(),
            connections: self.connections,
            requests_sent: self.requests_sent,
            unknown_frames: self.unknown_frames,
        }
    }
}

enum Wake {
    Retry,
    Forget,
}

/// Wait out a delay (or forever) while still taking commands.
async fn wait(
    shared: &Shared,
    state: &mut EnvState,
    commands: &mut mpsc::UnboundedReceiver<EnvCommand>,
    delay: Option<Duration>,
) -> Wake {
    let sleep = async {
        match delay {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            () = &mut sleep => return Wake::Retry,
            command = commands.recv() => match command {
                None | Some(EnvCommand::Forget) => return Wake::Forget,
                Some(EnvCommand::Reconnect) => return Wake::Retry,
                Some(EnvCommand::Dispatch(request)) => {
                    state.record(request, OutboxState::Rejected("Not connected to T3, so nothing was sent.".into()));
                    shared.publish(state.generation, state.view());
                }
                // An unconfirmed command waits for the connection; Retry needs one.
                Some(EnvCommand::Retry(_)) => {}
                Some(EnvCommand::DismissCommand(id)) => {
                    state.outbox.retain(|e| e.request.command_id != id || e.state == OutboxState::Sending);
                    shared.publish(state.generation, state.view());
                }
                Some(EnvCommand::OpenThread(id)) => {
                    state.thread = Some(ThreadState::new(&id));
                    state.thread_error = None;
                    shared.publish(state.generation, state.view());
                }
                Some(EnvCommand::CloseThread) => {
                    state.thread = None;
                    shared.publish(state.generation, state.view());
                }
                Some(EnvCommand::LoadOlder) => {}
            },
        }
    }
}

async fn connect(shared: &Shared, saved: &SavedEnvironment) -> Result<(Session, crate::model::Descriptor), T3Error> {
    connect_saved(&shared.http, shared.vault.as_ref(), saved, shared.log.clone()).await
}

/// The saved address and token, after checking that the address still
/// answers as the saved environment with the supported protocol and that
/// the sign-in has not expired. Every request that carries the token goes
/// through this first.
pub async fn checked_token(
    http: &Http,
    vault: &dyn TokenVault,
    saved: &SavedEnvironment,
) -> Result<(Url, crate::secret::Secret), T3Error> {
    checked(http, vault, saved).await.map(|(base, token, _)| (base, token))
}

async fn checked(
    http: &Http,
    vault: &dyn TokenVault,
    saved: &SavedEnvironment,
) -> Result<(Url, crate::secret::Secret, crate::model::Descriptor), T3Error> {
    let base = Url::parse(&saved.address).map_err(|e| T3Error::BadAddress { detail: e.to_string() })?;
    let descriptor = http.descriptor(&base).await?;
    if descriptor.environment_id != saved.environment_id {
        return Err(T3Error::WrongEnvironment {
            expected: saved.environment_id.clone(),
            actual: descriptor.environment_id,
        });
    }
    if time::now_epoch_secs() >= saved.token_expires_at {
        return Err(T3Error::SignInExpired);
    }
    let token = vault.load(&saved.environment_id)?.ok_or(T3Error::NotPaired)?;
    Ok((base, token, descriptor))
}

/// Open a socket to a saved environment. The server's identity, protocol and
/// the sign-in's expiry are checked before the token leaves this computer,
/// so a different server now at the saved address never receives it.
pub async fn connect_saved(
    http: &Http,
    vault: &dyn TokenVault,
    saved: &SavedEnvironment,
    log: Log,
) -> Result<(Session, crate::model::Descriptor), T3Error> {
    let (base, token, descriptor) = checked(http, vault, saved).await?;
    let ticket = http.websocket_ticket(&base, &token).await?;
    let session = Session::connect(&socket_url(&base, &ticket), log).await?;
    Ok((session, descriptor))
}

fn shell_request(shell: &ShellState) -> Value {
    match shell.last_sequence {
        Some(after) => json!({"afterSequence": after, "requestCompletionMarker": true}),
        None => json!({"requestCompletionMarker": true}),
    }
}

fn thread_request(thread: &ThreadState) -> Value {
    let mut request =
        json!({"threadId": thread.thread_id, "requestCompletionMarker": true, "acceptBoundedSnapshot": true});
    if let Some(after) = thread.last_sequence {
        request["afterSequence"] = json!(after);
    }
    request
}

async fn next_event(subscription: &mut Option<Subscription>) -> Option<StreamEvent> {
    match subscription {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

async fn run_environment(
    shared: Arc<Shared>,
    generation: u64,
    saved: SavedEnvironment,
    open_thread: Option<String>,
    mut commands: mpsc::UnboundedReceiver<EnvCommand>,
) {
    let log = shared.log.clone();
    let name = saved.label.clone();
    let mut state = EnvState {
        generation,
        saved: saved.clone(),
        status: ConnectionStatus::Connecting,
        server_version: None,
        shell: ShellState::default(),
        providers: Vec::new(),
        thread: open_thread.as_deref().map(ThreadState::new),
        thread_error: None,
        history_pending: None,
        history_error: None,
        outbox: Vec::new(),
        connections: 0,
        requests_sent: 0,
        unknown_frames: 0,
    };
    let (history_tx, mut history_rx) = mpsc::unbounded_channel::<HistoryReply>();
    let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<CommandReply>();
    let mut attempt: u32 = 0;

    loop {
        state.status = if attempt == 0 {
            ConnectionStatus::Connecting
        } else {
            match &state.status {
                ConnectionStatus::Reconnecting { .. } => state.status.clone(),
                _ => ConnectionStatus::Connecting,
            }
        };
        shared.publish(state.generation, state.view());
        log(&format!("env {name}: connecting (attempt {})", attempt + 1));

        let (session, descriptor) = match connect(&shared, &saved).await {
            Ok(connected) => connected,
            Err(e) => {
                log(&format!("env {name}: connect failed: {e:?}"));
                let wake = if e.needs_pairing() || e == T3Error::PairingRejected {
                    state.status = ConnectionStatus::NeedsPairing { reason: e.user_message() };
                    shared.publish(state.generation, state.view());
                    wait(&shared, &mut state, &mut commands, None).await
                } else if e.is_retryable() {
                    attempt += 1;
                    let delay = backoff(attempt);
                    state.status = ConnectionStatus::Reconnecting {
                        attempt,
                        reason: e.user_message(),
                        retry_in_ms: delay.as_millis() as u64,
                    };
                    shared.publish(state.generation, state.view());
                    wait(&shared, &mut state, &mut commands, Some(delay)).await
                } else {
                    state.status = ConnectionStatus::Blocked { reason: e.user_message() };
                    shared.publish(state.generation, state.view());
                    wait(&shared, &mut state, &mut commands, None).await
                };
                match wake {
                    Wake::Retry => continue,
                    Wake::Forget => return,
                }
            }
        };
        state.server_version = Some(descriptor.server_version.clone());
        log(&format!("env {name}: socket open to {} ({})", descriptor.label, descriptor.server_version));

        let channels = Channels {
            history_tx: &history_tx,
            history_rx: &mut history_rx,
            reply_tx: &reply_tx,
            reply_rx: &mut reply_rx,
        };
        let outcome = serve(&shared, &mut state, &session, &mut commands, channels).await;
        state.requests_sent += session.stats.requests_out.load(Ordering::Relaxed);
        state.unknown_frames += session.stats.unknown_frames.load(Ordering::Relaxed);
        drop(session);
        // Replies that never came are in doubt from here on.
        while let Ok(reply) = reply_rx.try_recv() {
            apply_reply(&mut state, reply);
        }
        state.connection_lost();
        match outcome {
            Served::Forget => {
                log(&format!("env {name}: removed"));
                return;
            }
            Served::Lost { error, healthy } => {
                log(&format!("env {name}: connection lost: {error:?}"));
                if error.needs_pairing() {
                    state.status = ConnectionStatus::NeedsPairing { reason: error.user_message() };
                    shared.publish(state.generation, state.view());
                    match wait(&shared, &mut state, &mut commands, None).await {
                        Wake::Retry => continue,
                        Wake::Forget => return,
                    }
                }
                if !error.is_retryable() {
                    state.status = ConnectionStatus::Blocked { reason: error.user_message() };
                    shared.publish(state.generation, state.view());
                    match wait(&shared, &mut state, &mut commands, None).await {
                        Wake::Retry => {
                            attempt = 0;
                            continue;
                        }
                        Wake::Forget => return,
                    }
                }
                attempt = if healthy { 1 } else { attempt + 1 };
                let delay = backoff(attempt);
                state.status = ConnectionStatus::Reconnecting {
                    attempt,
                    reason: error.user_message(),
                    retry_in_ms: delay.as_millis() as u64,
                };
                shared.publish(state.generation, state.view());
                if let Wake::Forget = wait(&shared, &mut state, &mut commands, Some(delay)).await {
                    return;
                }
            }
        }
    }
}

fn backoff(attempt: u32) -> Duration {
    let factor = 1.6_f64.powi(attempt.saturating_sub(1).min(20) as i32);
    BACKOFF_START.mul_f64(factor).min(BACKOFF_MAX)
}

/// A history page reply, tied to the load of the chat and the cursor it was
/// asked for, so a late reply for an earlier load is discarded.
struct HistoryReply {
    incarnation: u64,
    cursor: String,
    result: Result<crate::model::HistoryPage, T3Error>,
}

/// T3's answer to one command.
struct CommandReply {
    command_id: String,
    result: Result<Value, T3Error>,
}

fn apply_reply(state: &mut EnvState, reply: CommandReply) {
    let Some(entry) = state.entry(&reply.command_id) else { return };
    // A reply only settles a command still waiting for one.
    if !matches!(entry.state, OutboxState::Sending | OutboxState::Unconfirmed) {
        return;
    }
    entry.state = match reply.result {
        Ok(value) => OutboxState::Accepted {
            observed: false,
            thread_id: value.get("threadId").and_then(Value::as_str).map(str::to_owned),
        },
        // The socket went before the reply: it may or may not have run.
        Err(T3Error::Disconnected { .. }) => OutboxState::Unconfirmed,
        Err(T3Error::SignInRejected) => OutboxState::Rejected(
            "T3 refused this computer's sign-in for sending. Pair the server again to send from Bukno.".into(),
        ),
        Err(T3Error::Rpc { detail }) => OutboxState::Rejected(format!("T3 refused it: {}", short_reason(&detail))),
        Err(other) => OutboxState::Rejected(other.user_message()),
    };
}

/// `orchestration.dispatchCommand: SomeError: the message` as `the message`.
fn short_reason(detail: &str) -> String {
    let without_method = detail.split_once(": ").map_or(detail, |(_, rest)| rest);
    let reason = match without_method.split_once(": ") {
        Some((tag, message)) if !tag.contains(' ') => message,
        _ => without_method,
    };
    reason.trim().trim_end_matches('.').to_owned() + "."
}

struct Channels<'a> {
    history_tx: &'a mpsc::UnboundedSender<HistoryReply>,
    history_rx: &'a mut mpsc::UnboundedReceiver<HistoryReply>,
    reply_tx: &'a mpsc::UnboundedSender<CommandReply>,
    reply_rx: &'a mut mpsc::UnboundedReceiver<CommandReply>,
}

/// Write a command and wait for its reply in its own task.
fn send_command(
    shared: &Shared,
    state: &mut EnvState,
    session: &Session,
    reply_tx: &mpsc::UnboundedSender<CommandReply>,
    request: CommandRequest,
) {
    if !state.saved.can_operate() {
        state.record(
            request,
            OutboxState::Rejected("This server was paired to read only. Pair it again to send from Bukno.".into()),
        );
        return;
    }
    let caller = session.caller();
    let method = request.method();
    let payload = request.payload();
    let command_id = request.command_id.clone();
    (shared.log)(&format!("env {}: command {} ({})", state.saved.label, command_id, request.describe()));
    state.record(request, OutboxState::Sending);
    let tx = reply_tx.clone();
    tokio::spawn(async move {
        let result = caller.call(method, payload).await;
        let _ = tx.send(CommandReply { command_id, result });
    });
}

/// Chat list decode failures in a row on one socket before reconnecting it.
const SHELL_RETRY_LIMIT: u32 = 3;

enum Served {
    Forget,
    /// `healthy` is true when the connection had fully synchronized, so the
    /// next attempt starts with a short delay again.
    Lost {
        error: T3Error,
        healthy: bool,
    },
}

async fn serve(
    shared: &Shared,
    state: &mut EnvState,
    session: &Session,
    commands: &mut mpsc::UnboundedReceiver<EnvCommand>,
    channels: Channels<'_>,
) -> Served {
    let Channels { history_tx, history_rx, reply_tx, reply_rx } = channels;
    let log = shared.log.clone();
    let name = state.saved.label.clone();
    let lost = |error: T3Error, state: &EnvState| Served::Lost { error, healthy: state.shell.synchronized };

    // Until this connection catches up, a failure is not after a healthy run.
    state.shell.connection_started();
    // Which environment answered on the socket must match the saved one too.
    let config = match session.call(Method::GetConfig, json!({})).await {
        Ok(value) => match ServerConfig::decode(&value) {
            Ok(config) => config,
            Err(detail) => return lost(T3Error::Decode { detail: format!("server config: {detail}") }, state),
        },
        Err(e) => return lost(e, state),
    };
    if config.environment.environment_id != state.saved.environment_id {
        return lost(
            T3Error::WrongEnvironment {
                expected: state.saved.environment_id.clone(),
                actual: config.environment.environment_id,
            },
            state,
        );
    }
    state.providers = config.providers;

    let mut shell = match session.subscribe(Method::SubscribeShell, shell_request(&state.shell)).await {
        Ok(s) => s,
        Err(e) => return lost(e, state),
    };
    let mut thread_sub = None;
    if let Some(thread) = state.thread.as_mut() {
        thread.subscription_started();
        match session.subscribe(Method::SubscribeThread, thread_request(thread)).await {
            Ok(s) => thread_sub = Some(s),
            Err(e) => return lost(e, state),
        }
    }
    state.connections += 1;
    state.status = ConnectionStatus::Connected { current: false };
    shared.publish(state.generation, state.view());
    let mut thread_failures: u32 = 0;
    let mut shell_failures: u32 = 0;
    let mut closed = session.closed.clone();

    loop {
        tokio::select! {
            event = shell.next() => match event {
                Some(StreamEvent::Values(values)) => {
                    let mut changed = false;
                    let mut failed = None;
                    for value in &values {
                        match ShellItem::decode(value) {
                            Ok(item) => {
                                if let ShellItem::Unknown { kind } = &item {
                                    log(&format!("env {name}: unknown shell item kind {kind}"));
                                }
                                changed |= state.shell.apply(item);
                            }
                            Err(detail) => {
                                failed = Some(detail);
                                break;
                            }
                        }
                    }
                    if let Some(detail) = failed {
                        // Keep showing the list, and ask for a fresh snapshot of it.
                        log(&format!("env {name}: unreadable shell item: {detail}; reloading the chat list"));
                        shell_failures += 1;
                        if shell_failures > SHELL_RETRY_LIMIT {
                            return lost(T3Error::Disconnected { detail: format!("the chat list kept failing ({detail})") }, state);
                        }
                        state.shell.last_sequence = None;
                        state.shell.connection_started();
                        drop(shell); // Interrupt the old stream first.
                        shell = match session.subscribe(Method::SubscribeShell, shell_request(&state.shell)).await {
                            Ok(s) => s,
                            Err(e) => return lost(e, state),
                        };
                        shared.publish(state.generation, state.view());
                        continue;
                    }
                    shell.ack();
                    if state.shell.synchronized {
                        shell_failures = 0;
                    }
                    changed |= state.reconcile();
                    if changed {
                        shared.publish(state.generation, state.view());
                    }
                }
                Some(StreamEvent::End) | None => {
                    return lost(T3Error::Disconnected { detail: "the chat list stream ended".into() }, state);
                }
                Some(StreamEvent::Failed(e)) => return lost(e, state),
            },
            event = next_event(&mut thread_sub) => {
                let Some(thread) = state.thread.as_mut() else {
                    thread_sub = None;
                    continue;
                };
                match event {
                    Some(StreamEvent::Values(values)) => {
                        let mut changed = false;
                        let mut failed = None;
                        for value in &values {
                            match ThreadItem::decode(value) {
                                Ok(item) => {
                                    if let ThreadItem::UnknownEvent { event_type, .. } = &item {
                                        log(&format!("env {name}: unknown thread event type {event_type}"));
                                    }
                                    changed |= thread.apply(item);
                                }
                                Err(detail) => {
                                    failed = Some(detail);
                                    break;
                                }
                            }
                        }
                        if let Some(detail) = failed {
                            log(&format!("env {name}: unreadable thread item: {detail}"));
                            state.thread_error = Some(T3Error::Decode { detail }.user_message());
                            // Start this chat over from a fresh snapshot.
                            let id = thread.thread_id.clone();
                            *thread = ThreadState::new(&id);
                            drop(thread_sub.take()); // Interrupt the old stream first.
                            thread_failures += 1;
                            if thread_failures > THREAD_RETRY_LIMIT {
                                return lost(T3Error::Disconnected { detail: "the open chat kept failing".into() }, state);
                            }
                            match session.subscribe(Method::SubscribeThread, thread_request(state.thread.as_ref().unwrap())).await {
                                Ok(s) => thread_sub = Some(s),
                                Err(e) => return lost(e, state),
                            }
                            shared.publish(state.generation, state.view());
                            continue;
                        }
                        if let Some(s) = &thread_sub {
                            s.ack();
                        }
                        if thread.synchronized {
                            thread_failures = 0;
                            state.thread_error = None;
                        }
                        changed |= state.reconcile();
                        if changed {
                            shared.publish(state.generation, state.view());
                        }
                    }
                    Some(StreamEvent::End) | None => {
                        log(&format!("env {name}: the open chat's stream ended"));
                        thread_sub = None;
                        shared.publish(state.generation, state.view());
                    }
                    Some(StreamEvent::Failed(e)) => {
                        log(&format!("env {name}: the open chat's stream failed: {e:?}"));
                        // A lost socket or sign-in ends the whole connection; only
                        // server-side stream failures are retried on this socket.
                        if e.needs_pairing() || matches!(e, T3Error::Disconnected { .. }) {
                            return lost(e, state);
                        }
                        thread_failures += 1;
                        if thread_failures > THREAD_RETRY_LIMIT {
                            return lost(e, state);
                        }
                        // Resume from the last applied event on the same socket.
                        tokio::time::sleep(BACKOFF_START * thread_failures).await;
                        thread.subscription_started();
                        match session.subscribe(Method::SubscribeThread, thread_request(thread)).await {
                            Ok(s) => thread_sub = Some(s),
                            Err(e) => return lost(e, state),
                        }
                    }
                }
            }
            command = commands.recv() => match command {
                None | Some(EnvCommand::Forget) => return Served::Forget,
                Some(EnvCommand::Reconnect) => {
                    return lost(T3Error::Disconnected { detail: "reconnect requested".into() }, state);
                }
                Some(EnvCommand::Dispatch(request)) => {
                    // A second press of the same action is one command.
                    if state.entry(&request.command_id).is_none() {
                        send_command(shared, state, session, reply_tx, request);
                        shared.publish(state.generation, state.view());
                    }
                }
                Some(EnvCommand::Retry(id)) => {
                    let request = state.entry(&id).filter(|e| e.state == OutboxState::Unconfirmed).map(|e| e.request.clone());
                    if let Some(request) = request {
                        send_command(shared, state, session, reply_tx, request);
                        shared.publish(state.generation, state.view());
                    }
                }
                Some(EnvCommand::DismissCommand(id)) => {
                    state.outbox.retain(|e| e.request.command_id != id || e.state == OutboxState::Sending);
                    shared.publish(state.generation, state.view());
                }
                Some(EnvCommand::OpenThread(id)) => {
                    if state.thread.as_ref().is_some_and(|t| t.thread_id == id) {
                        continue;
                    }
                    drop(thread_sub.take()); // Interrupt the old stream first.
                    thread_failures = 0;
                    state.thread_error = None;
                    state.history_error = None;
                    state.history_pending = None;
                    let thread = ThreadState::new(&id);
                    match session.subscribe(Method::SubscribeThread, thread_request(&thread)).await {
                        Ok(s) => thread_sub = Some(s),
                        Err(e) => {
                            state.thread = Some(thread);
                            return lost(e, state);
                        }
                    }
                    state.thread = Some(thread);
                    shared.publish(state.generation, state.view());
                }
                Some(EnvCommand::CloseThread) => {
                    thread_sub = None;
                    state.thread = None;
                    shared.publish(state.generation, state.view());
                }
                Some(EnvCommand::LoadOlder) => {
                    let Some(thread) = state.thread.as_ref() else { continue };
                    let Some(cursor) = thread.history_cursor.clone() else { continue };
                    if state.history_in_flight() || !thread.has_more_history {
                        continue;
                    }
                    state.history_pending = Some((thread.incarnation, cursor.clone()));
                    state.history_error = None;
                    shared.publish(state.generation, state.view());
                    let http = shared.http.clone();
                    let vault = shared.vault.clone();
                    let saved = state.saved.clone();
                    let thread_id = thread.thread_id.clone();
                    let incarnation = thread.incarnation;
                    let tx = history_tx.clone();
                    tokio::spawn(async move {
                        let result = async {
                            // Same checks as opening a socket before the token is sent.
                            let (base, token) = checked_token(&http, vault.as_ref(), &saved).await?;
                            http.history_page(&base, &token, &thread_id, &cursor).await
                        }.await;
                        let _ = tx.send(HistoryReply { incarnation, cursor, result });
                    });
                }
            },
            reply = reply_rx.recv() => {
                let Some(reply) = reply else { continue };
                apply_reply(state, reply);
                // A reply can arrive after the chat already shows the effect.
                state.reconcile();
                shared.publish(state.generation, state.view());
            }
            page = history_rx.recv() => {
                let Some(HistoryReply { incarnation, cursor, result }) = page else { continue };
                let key = (incarnation, cursor);
                if state.history_pending.as_ref() == Some(&key) {
                    state.history_pending = None;
                }
                // Only a reply for this load of the chat, at its current cursor, applies.
                let Some(thread) = state
                    .thread
                    .as_mut()
                    .filter(|t| t.incarnation == key.0 && t.history_cursor.as_deref() == Some(key.1.as_str()))
                else {
                    log(&format!("env {name}: discarded a history page for an earlier load of the chat"));
                    shared.publish(state.generation, state.view());
                    continue;
                };
                match result {
                    Ok(page) => {
                        log(&format!("env {name}: loaded {} older items", page.items.len()));
                        thread.merge_history(page);
                    }
                    Err(T3Error::Server { status: 400, .. }) => {
                        // The cursor is no longer valid: start over from a fresh snapshot.
                        log(&format!("env {name}: history cursor rejected; reloading the chat"));
                        state.history_error = Some("Older messages could not be loaded, so the chat was reloaded.".into());
                        let id = thread.thread_id.clone();
                        *thread = ThreadState::new(&id);
                        drop(thread_sub.take()); // Interrupt the old stream first.
                        match session.subscribe(Method::SubscribeThread, thread_request(thread)).await {
                            Ok(s) => thread_sub = Some(s),
                            Err(e) => return lost(e, state),
                        }
                    }
                    Err(e) => state.history_error = Some(e.user_message()),
                }
                shared.publish(state.generation, state.view());
            }
            _ = closed.changed() => {
                let reason = closed.borrow().clone().unwrap_or_else(|| "the socket closed".into());
                return lost(T3Error::Disconnected { detail: reason }, state);
            }
        }
    }
}
