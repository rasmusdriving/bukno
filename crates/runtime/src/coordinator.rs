//! The coordinator task: one inbox, one state machine, effects executed in order.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bukno_core::decision::DecisionAnswer;
use bukno_core::event::{
    Command, Effect, EngineEvent, EngineEventKind, EngineRequest, Input, PersistRequest, RejectReason, StorageResult,
    ViewUpdate,
};
use bukno_core::ids::{DecisionId, ItemId, MessageId, ProjectId, RunId, TaskId, WorkspaceId};
use bukno_core::machine::Machine;
use bukno_core::message::{Provider, TranscriptItem};
use bukno_core::task::{ProjectInfo, RunSettings, TaskInfo, WorkspaceInfo, WorkspaceKind};
use bukno_platform::paths::AppPaths;
use bukno_platform::process::{self, ProcessRecord};
use bukno_providers::codex::{self, CodexAdapter, ShutdownReport, Status, presets};
use bukno_storage::worker::Job;
use bukno_storage::{Store, Worker, repository};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::config::{self, Config};
use crate::engines::{EngineView, Engines, Revert};
use crate::synthetic::{self, Scenario, SyntheticEngine};
use crate::workspace;

/// Longest a streamed text change waits before it is shown (specification section 21).
pub const TEXT_BATCH: Duration = Duration::from_millis(50);
/// Streaming text is written to storage in batches this far apart, not per token.
const ITEM_BATCH: Duration = Duration::from_millis(500);
/// Overrides the reasoning effort for development and end-to-end runs.
pub const EFFORT_ENV: &str = "BUKNO_CODEX_EFFORT";
/// Bound on queued engine and storage results.
const INBOX_CAPACITY: usize = 1_024;

/// What the interface asks for. The runtime turns these into core commands
/// with freshly captured identifiers.
#[derive(Clone, Debug)]
pub enum UiCommand {
    /// Send a message in an existing chat. `preset` is the permission preset ID.
    Submit {
        task: TaskId,
        provider: Provider,
        draft_revision: u64,
        body: String,
        preset: Option<String>,
    },
    /// Create a chat (in a project, or projectless) and send its first message.
    SubmitNew {
        project: Option<ProjectId>,
        provider: Provider,
        body: String,
        preset: Option<String>,
    },
    Interrupt {
        task: TaskId,
        run: RunId,
    },
    ForceStop {
        task: TaskId,
        run: RunId,
    },
    SelectChat {
        task: TaskId,
    },
    SaveDraft {
        task: TaskId,
        revision: u64,
        text: String,
    },
    AddProject {
        path: PathBuf,
    },
    Answer {
        decision: DecisionId,
        run: RunId,
        generation: u64,
        answer: DecisionAnswer,
    },
    RunAnyway {
        run: RunId,
    },
    SendQueued {
        run: RunId,
    },
    Resend {
        unknown: RunId,
        preset: Option<String>,
    },
    Dismiss {
        run: RunId,
    },
    SetWorkFolder {
        path: PathBuf,
    },
    SetPreset {
        preset: String,
    },
    Engine(EngineAction),
    Quit {
        stop: bool,
    },
}

#[derive(Clone, Debug)]
pub enum EngineAction {
    /// Find the engine again and start it to check the login.
    CheckAgain,
    UsePrevious,
    TryLatest,
    /// Run the engine's own install command for the last working version.
    RunRevert,
    Choose(PathBuf),
}

/// What the interface receives.
#[derive(Clone, Debug)]
pub enum UiEvent {
    View(ViewUpdate),
    Rejected {
        task: TaskId,
        reason: RejectReason,
    },
    Engine(EngineView),
    Setup(SetupView),
    /// A new chat was created for the user's first message; show it.
    Opened {
        task: TaskId,
    },
    /// Problems the user should see once, such as engines left over from a crash.
    Notice(String),
    QuitDone(QuitReport),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupView {
    pub work_folder: Option<String>,
    /// The work folder is set but missing (for example an unplugged drive).
    pub work_folder_missing: bool,
    pub state_dir: String,
    /// Locations come from BUKNO_STATE_DIR or BUKNO_WORK_DIR.
    pub overridden: bool,
    pub preset: String,
}

#[derive(Clone, Debug, Default)]
pub struct QuitReport {
    pub engine: ShutdownReport,
    pub saved: bool,
}

pub struct Coordinator {
    commands: Option<mpsc::UnboundedSender<UiCommand>>,
    runtime: Option<tokio::runtime::Runtime>,
    done: Option<oneshot::Receiver<()>>,
}

fn new_id() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("bukno-coordinator")
        .enable_all()
        .build()
        .expect("start the Tokio runtime")
}

type Wake = Arc<dyn Fn() + Send + Sync>;

impl Coordinator {
    /// Start the coordinator for a synthetic scenario. `wake` is called after
    /// events are queued, so the interface can schedule a repaint.
    pub fn start_synthetic(scenario: Scenario, events: std::sync::mpsc::Sender<UiEvent>, wake: Wake) -> Self {
        let runtime = tokio_runtime();
        let (commands, command_rx) = mpsc::unbounded_channel();
        runtime.spawn(run_synthetic(Arc::new(scenario), command_rx, events, wake));
        Self { commands: Some(commands), runtime: Some(runtime), done: None }
    }

    /// Start the real coordinator: the store, the Codex adapter and the state machine.
    pub fn start(paths: AppPaths, store: Store, events: std::sync::mpsc::Sender<UiEvent>, wake: Wake) -> Self {
        let runtime = tokio_runtime();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (done_tx, done) = oneshot::channel();
        runtime.spawn(async move {
            Real::run(paths, store, command_rx, events, wake).await;
            let _ = done_tx.send(());
        });
        Self { commands: Some(commands), runtime: Some(runtime), done: Some(done) }
    }

    pub fn send(&self, command: UiCommand) {
        if let Some(commands) = &self.commands {
            // The loop only ends when the coordinator is dropped.
            let _ = commands.send(command);
        }
    }
}

impl Drop for Coordinator {
    fn drop(&mut self) {
        // Closing the command channel ends the loop, which closes the store.
        self.commands = None;
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        match self.done.take() {
            Some(done) => {
                let _ = runtime.block_on(async { tokio::time::timeout(Duration::from_secs(5), done).await });
                runtime.shutdown_timeout(Duration::from_millis(500));
            }
            // Do not block the UI thread on in-flight synthetic streams.
            None => {
                thread::spawn(move || runtime.shutdown_timeout(Duration::from_millis(200)));
            }
        }
    }
}

// ----- Real mode -------------------------------------------------------------

struct Real {
    machine: Machine,
    paths: AppPaths,
    config: Config,
    publisher: Publisher,
    inbox: mpsc::Sender<Input>,
    worker: Worker,
    engines: Engines,
    codex: CodexAdapter,
    records: Vec<ProcessRecord>,
    records_path: PathBuf,
    /// Streaming items waiting to be written, newest version per item.
    pending_items: HashMap<ItemId, PersistRequest>,
    items_at: Option<Instant>,
    git: Option<PathBuf>,
    quit_started: bool,
}

impl Real {
    async fn run(
        paths: AppPaths,
        mut store: Store,
        mut commands: mpsc::UnboundedReceiver<UiCommand>,
        events: std::sync::mpsc::Sender<UiEvent>,
        wake: Wake,
    ) {
        let (inbox, mut inbox_rx) = mpsc::channel::<Input>(INBOX_CAPACITY);
        let mut publisher = Publisher { events, wake, pending: HashMap::new(), flush_at: None };
        let mut snapshot = match repository::load_snapshot(store.connection()) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                publisher.send(UiEvent::Notice(format!("Bukno could not read its saved chats: {e}")));
                Default::default()
            }
        };
        workspace::check_available(&mut snapshot.workspaces);
        let reply = inbox.clone();
        let worker = Worker::spawn(
            store,
            Box::new(move |result| {
                let _ = reply.blocking_send(Input::Stored(result));
            }),
        );
        let records_path = paths.state_dir.join("engine-processes.tsv");
        // Engines a crashed Bukno left behind must not keep working unseen.
        let leftovers = process::clean_up_leftovers(&records_path);
        if !leftovers.is_empty() {
            publisher.send(UiEvent::Notice(format!(
                "Bukno stopped {} Codex process{} left running by an earlier session.",
                leftovers.len(),
                if leftovers.len() == 1 { "" } else { "es" }
            )));
        }
        let engines = Engines::load(&paths.state_dir);
        let (status_tx, mut status_rx) = mpsc::unbounded_channel::<Status>();
        let resolve_engines = engines.clone();
        let codex = CodexAdapter::spawn(
            inbox.clone(),
            Arc::new(move || resolve_engines.launch()),
            Arc::new(move |status| {
                let _ = status_tx.send(status);
            }),
        );
        let mut config = config::load(&paths.state_dir);
        if let Some(work) = &paths.work_dir {
            config.work_folder = Some(work.clone());
        }
        let mut real = Real {
            machine: Machine::new(),
            paths,
            config,
            publisher,
            inbox,
            worker,
            engines,
            codex,
            records: Vec::new(),
            records_path,
            pending_items: HashMap::new(),
            items_at: None,
            git: bukno_platform::discovery::find_git().ok(),
            quit_started: false,
        };
        real.publish_setup();
        let view = real.engines.view();
        real.publisher.send(UiEvent::Engine(view));
        real.step(Input::Stored(StorageResult::Restored(snapshot))).await;

        loop {
            let (text_at, items_at) = (real.publisher.flush_at, real.items_at);
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => real.on_command(command).await,
                    None => break,
                },
                Some(input) = inbox_rx.recv() => real.step(input).await,
                Some(status) = status_rx.recv() => real.on_status(status),
                _ = sleep_until(text_at) => real.publisher.flush(),
                _ = sleep_until(items_at) => real.flush_items(),
            }
        }
        real.flush_items();
        let Real { worker, codex, .. } = real;
        codex.shutdown().await;
        let _ = tokio::task::spawn_blocking(move || worker.shutdown()).await;
    }

    async fn step(&mut self, input: Input) {
        let mut queue = VecDeque::from([input]);
        while let Some(input) = queue.pop_front() {
            for effect in self.machine.step(input) {
                self.effect(effect, &mut queue).await;
            }
        }
    }

    async fn effect(&mut self, effect: Effect, _queue: &mut VecDeque<Input>) {
        match effect {
            Effect::Persist(PersistRequest::Item { item, provider_item }) if !item.completed => {
                self.pending_items.insert(item.id, PersistRequest::Item { item, provider_item });
                self.items_at.get_or_insert_with(|| Instant::now() + ITEM_BATCH);
            }
            Effect::Persist(request) => {
                // Keep order: streamed text first, then whatever follows it.
                if let PersistRequest::Item { item, .. } = &request {
                    self.pending_items.remove(&item.id);
                }
                self.flush_items();
                self.worker.send(Job::Persist(Box::new(request)));
            }
            Effect::Load(request) => self.worker.send(Job::Load(request)),
            Effect::Engine(Provider::Codex, request) => self.codex.request(request),
            Effect::Engine(Provider::Claude, request) => {
                if let EngineRequest::Connect = request {
                    let event = EngineEvent {
                        provider: Provider::Claude,
                        connection_generation: 1,
                        kind: EngineEventKind::ConnectFailed { reason: "Claude arrives in a later pass.".into() },
                    };
                    // Never wait on the coordinator's own inbox from inside the loop.
                    let _ = self.inbox.try_send(Input::Engine(event));
                }
            }
            Effect::Publish(ViewUpdate::ItemUpserted(item)) if !item.completed => self.publisher.queue_text(item),
            Effect::Publish(update) => self.publisher.send(UiEvent::View(update)),
            Effect::Timer { key, after_ms } => {
                let inbox = self.inbox.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(after_ms)).await;
                    let _ = inbox.send(Input::Timer(key)).await;
                });
            }
            Effect::Rejected { task, reason } => self.publisher.send(UiEvent::Rejected { task, reason }),
            Effect::QuitReady => self.quit().await,
        }
    }

    fn flush_items(&mut self) {
        self.items_at = None;
        for (_, request) in self.pending_items.drain() {
            self.worker.send(Job::Persist(Box::new(request)));
        }
    }

    fn settings(&self, preset: Option<String>) -> RunSettings {
        let preset = presets::find(
            &preset.or_else(|| self.config.codex_preset.clone()).unwrap_or_else(|| presets::DEFAULT.id.to_owned()),
        );
        // Development and end-to-end runs can ask for a lighter effort; the
        // model control that sets this for real lands in Pass 3.
        let effort = std::env::var(EFFORT_ENV).ok().filter(|e| !e.is_empty());
        RunSettings { preset: preset.id.to_owned(), writes: preset.writes, model: None, effort }
    }

    async fn on_command(&mut self, command: UiCommand) {
        let input = match command {
            UiCommand::Submit { task, provider, draft_revision, body, preset } => Command::SubmitMessage {
                task,
                provider,
                message: MessageId(new_id()),
                run: RunId(new_id()),
                item: ItemId(new_id()),
                draft_revision,
                body,
                settings: self.settings(preset),
            },
            UiCommand::SubmitNew { project, provider, body, preset } => {
                let task = TaskId(new_id());
                let Some(workspace) = self.workspace_for_new_chat(project, task) else {
                    return;
                };
                let info = TaskInfo {
                    id: task,
                    title: String::new(),
                    provider,
                    project,
                    workspace: workspace.id,
                    session: None,
                    created: now(),
                };
                self.step(Input::Command(Command::CreateChat { task: info, workspace })).await;
                self.publisher.send(UiEvent::Opened { task });
                self.step(Input::Command(Command::SelectChat { task })).await;
                Command::SubmitMessage {
                    task,
                    provider,
                    message: MessageId(new_id()),
                    run: RunId(new_id()),
                    item: ItemId(new_id()),
                    draft_revision: 0,
                    body,
                    settings: self.settings(preset),
                }
            }
            UiCommand::Interrupt { task, run } => Command::InterruptRun { task, run },
            UiCommand::ForceStop { task, run } => Command::ForceStop { task, run },
            UiCommand::SelectChat { task } => Command::SelectChat { task },
            UiCommand::SaveDraft { task, revision, text } => Command::SaveDraft { task, revision, text },
            UiCommand::AddProject { path } => {
                let Some((project, workspace)) = self.prepare_project(&path) else {
                    return;
                };
                Command::AddProject { project, workspace }
            }
            UiCommand::Answer { decision, run, generation, answer } => {
                Command::AnswerDecision { decision, run, generation, answer }
            }
            UiCommand::RunAnyway { run } => Command::RunAnyway { run },
            UiCommand::SendQueued { run } => Command::SendQueued { run },
            UiCommand::Resend { unknown, preset } => Command::Resend {
                unknown,
                message: MessageId(new_id()),
                run: RunId(new_id()),
                item: ItemId(new_id()),
                settings: self.settings(preset),
            },
            UiCommand::Dismiss { run } => Command::Dismiss { run },
            UiCommand::SetWorkFolder { path } => {
                match std::fs::create_dir_all(&path).and_then(|_| std::fs::canonicalize(&path)) {
                    Ok(path) => {
                        self.config.work_folder = Some(path);
                        if let Err(e) = config::save(&self.paths.state_dir, &self.config) {
                            self.publisher.send(UiEvent::Notice(format!("The work folder could not be saved: {e}")));
                        }
                    }
                    Err(e) => self.publisher.send(UiEvent::Notice(format!("That folder cannot be used: {e}"))),
                }
                self.publish_setup();
                return;
            }
            UiCommand::SetPreset { preset } => {
                self.config.codex_preset = Some(presets::find(&preset).id.to_owned());
                let _ = config::save(&self.paths.state_dir, &self.config);
                self.publish_setup();
                return;
            }
            UiCommand::Engine(action) => return self.on_engine_action(action).await,
            UiCommand::Quit { stop } => Command::Quit { stop },
        };
        self.step(Input::Command(input)).await;
    }

    fn workspace_for_new_chat(&mut self, project: Option<ProjectId>, task: TaskId) -> Option<WorkspaceInfo> {
        if let Some(project) = project {
            return self.machine.project(project).map(|(_, w)| w.clone());
        }
        let Some(work) = self.config.work_folder.clone().filter(|w| w.is_dir()) else {
            // Never create a replacement work folder somewhere else.
            self.publisher
                .send(UiEvent::Notice("The work folder is not available, so a new chat cannot start.".into()));
            return None;
        };
        let folder = workspace::chat_folder(&work, task);
        if let Err(e) = std::fs::create_dir_all(&folder) {
            self.publisher.send(UiEvent::Notice(format!("The chat folder could not be created: {e}")));
            return None;
        }
        match workspace::resolve(&folder, WorkspaceKind::Chat, None) {
            Ok(info) => Some(info),
            Err(e) => {
                self.publisher.send(UiEvent::Notice(format!("The chat folder cannot be used: {e}")));
                None
            }
        }
    }

    fn prepare_project(&mut self, path: &std::path::Path) -> Option<(ProjectInfo, WorkspaceInfo)> {
        let mut info = match workspace::resolve(path, WorkspaceKind::Project, self.git.as_deref()) {
            Ok(info) => info,
            Err(e) => {
                self.publisher.send(UiEvent::Notice(format!("That folder cannot be added: {e}")));
                return None;
            }
        };
        if let Some(existing) = self.machine.same_workspace(&info) {
            info.id = existing.id;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| info.path.clone());
        Some((ProjectInfo { id: ProjectId(new_id()), name, workspace: info.id }, info))
    }

    async fn on_engine_action(&mut self, action: EngineAction) {
        let view = match action {
            EngineAction::CheckAgain => {
                let view = self.engines.discover();
                self.codex.request(EngineRequest::Connect);
                view
            }
            EngineAction::UsePrevious => match self.engines.use_previous() {
                Ok(view) => view,
                Err(message) => {
                    self.publisher.send(UiEvent::Notice(message));
                    self.engines.view()
                }
            },
            EngineAction::TryLatest => self.engines.try_latest(),
            EngineAction::Choose(path) => self.engines.choose(path),
            EngineAction::RunRevert => {
                if let Some(Revert::Command { program, args, shown, .. }) = self.engines.view().revert {
                    self.publisher.send(UiEvent::Notice(format!("Running {shown}…")));
                    let result = tokio::process::Command::new(&program).args(&args).output().await;
                    let message = match result {
                        Ok(out) if out.status.success() => format!("{shown} finished. Choose Check again."),
                        Ok(out) => format!(
                            "{shown} failed: {}",
                            String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("no details")
                        ),
                        Err(e) => format!("{shown} could not run: {e}"),
                    };
                    self.publisher.send(UiEvent::Notice(message));
                }
                self.engines.discover()
            }
        };
        self.publisher.send(UiEvent::Engine(view));
    }

    fn on_status(&mut self, status: Status) {
        let view = match status {
            Status::Starting { .. } => self.engines.view(),
            Status::Spawned { record } => {
                self.records.push(record);
                let _ = process::save_records(&self.records_path, &self.records);
                return;
            }
            Status::Ready { account, default_model, note, .. } => self.engines.ready(&account, default_model, note),
            Status::Failed { reason, .. } => self.engines.failed(&reason),
            Status::Exited { pid, .. } => {
                self.records.retain(|r| r.pid != pid);
                let _ = process::save_records(&self.records_path, &self.records);
                self.engines.stopped()
            }
            Status::TurnCompleted { .. } => self.engines.worked(),
        };
        self.publisher.send(UiEvent::Engine(view));
    }

    fn publish_setup(&mut self) {
        let work = self.config.work_folder.clone();
        self.publisher.send(UiEvent::Setup(SetupView {
            work_folder: work.as_ref().map(|w| w.display().to_string()),
            work_folder_missing: work.as_ref().is_some_and(|w| !w.is_dir()),
            state_dir: self.paths.state_dir.display().to_string(),
            overridden: self.paths.overridden,
            preset: self.config.codex_preset.clone().unwrap_or_else(|| presets::DEFAULT.id.to_owned()),
        }));
    }

    /// Every run has settled: end the engine, write everything, then report.
    async fn quit(&mut self) {
        if self.quit_started {
            return;
        }
        self.quit_started = true;
        self.flush_items();
        let engine = self.codex.shutdown().await;
        let worker_flushed = {
            let (tx, rx) = std::sync::mpsc::channel();
            self.worker.send(Job::Flush(tx));
            tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(5)).is_ok()).await.unwrap_or(false)
        };
        self.publisher.send(UiEvent::QuitDone(QuitReport { engine, saved: worker_flushed }));
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

/// The runtime's view of the Codex presets, for the interface.
pub fn codex_presets() -> &'static [codex::presets::Preset] {
    presets::PRESETS
}

// ----- Synthetic mode --------------------------------------------------------

async fn run_synthetic(
    scenario: Arc<Scenario>,
    mut commands: mpsc::UnboundedReceiver<UiCommand>,
    events: std::sync::mpsc::Sender<UiEvent>,
    wake: Wake,
) {
    let (inbox_tx, mut inbox) = mpsc::channel::<Input>(INBOX_CAPACITY);
    let engine = SyntheticEngine::new(inbox_tx, scenario.clone());
    let mut machine = Machine::new();
    let mut publisher = Publisher { events, wake, pending: HashMap::new(), flush_at: None };

    // Results the coordinator produces for itself (the synthetic store's
    // answers). They are handled before the inbox is read again, so the loop
    // never waits on its own bounded channel.
    let mut local = VecDeque::new();
    let workspace = WorkspaceInfo {
        id: WorkspaceId(synthetic::TASK.0),
        path: std::env::temp_dir().display().to_string(),
        key: vec!["synthetic".into()],
        kind: WorkspaceKind::Chat,
        git_root: None,
        identity: None,
        available: true,
    };
    let task = TaskInfo {
        id: synthetic::TASK,
        title: scenario.title.to_owned(),
        provider: Provider::Codex,
        project: None,
        workspace: workspace.id,
        session: None,
        created: 0,
    };
    local.push_back(Input::Command(Command::CreateChat { task, workspace }));
    local.push_back(Input::Stored(StorageResult::ChatSaved { task: synthetic::TASK }));
    // The synthetic engine's one connection. A real adapter reports this
    // after its handshake.
    local.push_back(Input::Engine(EngineEvent {
        provider: Provider::Codex,
        connection_generation: synthetic::GENERATION,
        kind: EngineEventKind::Connected,
    }));
    local.push_back(Input::Stored(StorageResult::HistoryLoaded {
        task: synthetic::TASK,
        items: scenario.history(),
        draft: None,
    }));
    if let Some(body) = scenario.auto_submit.clone() {
        local.push_back(synthetic_submit(body, 0));
    }

    loop {
        let flush_at = publisher.flush_at;
        let input = if let Some(input) = local.pop_front() {
            input
        } else {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(UiCommand::Submit { body, draft_revision, .. }) => synthetic_submit(body, draft_revision),
                    Some(UiCommand::Interrupt { task, run }) => Input::Command(Command::InterruptRun { task, run }),
                    Some(_) => continue,
                    None => break,
                },
                Some(input) = inbox.recv() => input,
                _ = sleep_until(flush_at) => {
                    publisher.flush();
                    continue;
                }
            }
        };

        for effect in machine.step(input) {
            match effect {
                // Synthetic mode persists nothing. The outbox commit is
                // simulated so the send path runs through the real machine.
                Effect::Persist(PersistRequest::RecordDelivery { message, .. })
                | Effect::Persist(PersistRequest::MarkAboutToSend { message }) => {
                    local.push_back(Input::Stored(StorageResult::DeliveryRecorded { message }));
                }
                Effect::Persist(_) | Effect::Load(_) | Effect::Timer { .. } | Effect::QuitReady => {}
                Effect::Engine(_, EngineRequest::StartTurn { run, .. }) => {
                    engine.start_turn(run, ItemId(new_id()));
                }
                Effect::Engine(_, EngineRequest::Interrupt { run }) => engine.interrupt(run),
                Effect::Engine(..) => {}
                Effect::Publish(ViewUpdate::ItemUpserted(item)) if !item.completed => {
                    publisher.queue_text(item);
                }
                Effect::Publish(update) => publisher.send(UiEvent::View(update)),
                Effect::Rejected { task, reason } => {
                    publisher.send(UiEvent::Rejected { task, reason });
                }
            }
        }
    }
}

fn synthetic_submit(body: String, draft_revision: u64) -> Input {
    Input::Command(Command::SubmitMessage {
        task: synthetic::TASK,
        provider: Provider::Codex,
        message: MessageId(new_id()),
        run: RunId(new_id()),
        item: ItemId(new_id()),
        draft_revision,
        body,
        settings: RunSettings {
            preset: "synthetic".into(),
            writes: bukno_core::task::Writes::Never,
            model: None,
            effort: None,
        },
    })
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Sends view updates to the interface. Streaming text is coalesced per item
/// for up to [`TEXT_BATCH`]; every other update flushes pending text first so
/// ordering is kept and control events are never dropped.
struct Publisher {
    events: std::sync::mpsc::Sender<UiEvent>,
    wake: Wake,
    pending: HashMap<ItemId, TranscriptItem>,
    flush_at: Option<Instant>,
}

impl Publisher {
    fn queue_text(&mut self, item: TranscriptItem) {
        self.pending.insert(item.id, item);
        self.flush_at.get_or_insert_with(|| Instant::now() + TEXT_BATCH);
    }

    fn flush(&mut self) {
        self.flush_at = None;
        if self.pending.is_empty() {
            return;
        }
        for (_, item) in self.pending.drain() {
            let _ = self.events.send(UiEvent::View(ViewUpdate::ItemUpserted(item)));
        }
        (self.wake)();
    }

    fn send(&mut self, event: UiEvent) {
        if let UiEvent::View(ViewUpdate::ItemUpserted(item)) = &event {
            self.pending.remove(&item.id);
        }
        self.flush();
        let _ = self.events.send(event);
        (self.wake)();
    }
}
