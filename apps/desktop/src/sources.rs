//! The connection layer the sidebar and transcript read from.
//!
//! A chat is either a local chat run by Bukno's own coordinator (the direct
//! Codex path, unchanged) or a chat that lives on a T3 server. T3 chat
//! references always carry the environment they belong to, so two servers
//! can never be confused, and every command carries the chat it is for.
//!
//! T3 owns the chats. Bukno keeps only drafts and which chat was open, in
//! `t3-client-state.json` in the state folder.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bukno_core::ids::{ItemId, TaskId};
use bukno_core::message::{ItemKind, Provider, TranscriptItem};
use bukno_t3_client::command::{CommandRequest, Outgoing};
use bukno_t3_client::hub::{OutboxEntry, ThreadView};
use bukno_t3_client::model::TurnKind;
use bukno_t3_client::secret::SystemKeychain;
use bukno_t3_client::thread::Row;
use bukno_t3_client::{EnvironmentView, Hub, HubView, Log};

/// Which chat is meant, wherever it lives.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChatRef {
    Local(TaskId),
    T3(T3ChatRef),
}

/// A chat on a T3 server: the environment ID plus the thread ID in it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct T3ChatRef {
    pub environment: String,
    pub thread: String,
}

/// The Add environment form. The pairing link is cleared as soon as it is
/// handed to the client, and never saved or logged.
#[derive(Default)]
pub struct AddForm {
    pub address: String,
    pub link: String,
}

/// What Bukno keeps about T3 chats on this computer.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ClientState {
    version: u32,
    /// Draft text by [`draft_key`].
    drafts: BTreeMap<String, String>,
    last_open: Option<(String, String)>,
    preferred_project: Option<(String, String)>,
    /// Sends T3 has not settled yet, kept across a restart so their outcome
    /// is checked with their original IDs instead of the draft being sent
    /// again as a new message.
    pending: Vec<PendingSend>,
}

/// A message or new chat sent from a T3 composer and not settled yet.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSend {
    pub environment: String,
    /// The [`draft_key`] of the composer it was sent from.
    pub key: String,
    /// The text that was sent, trimmed. Only exactly this text is cleared.
    pub draft: String,
    pub request: CommandRequest,
}

/// The draft key of a chat, or of a new chat in a project.
pub fn draft_key(environment: &str, thread: Option<&str>, project: Option<&str>) -> String {
    match (thread, project) {
        (Some(thread), _) => format!("{environment}/{thread}"),
        (None, Some(project)) => format!("{environment}/new/{project}"),
        (None, None) => format!("{environment}/new"),
    }
}

pub struct T3Source {
    hub: Hub,
    local_config: bukno_t3_client::local::LocalConfig,
    pub advanced_setup: bool,
    pub view: HubView,
    pub open: Option<T3ChatRef>,
    pub form: AddForm,
    state_file: PathBuf,
    client: ClientState,
    /// Unsaved draft changes since this time.
    dirty_since: Option<f64>,
    pub save_error: Option<String>,
    /// Per transcript item: what it was built from, and the revision given.
    signatures: HashMap<ItemId, (Vec<u64>, u64)>,
    next_revision: u64,
    /// Transcript items as last handed to the document, and the thread revision.
    built: Option<(T3ChatRef, u64)>,
    order: Vec<ItemId>,
}

/// Append-only log of the T3 client, with timestamps. Never contains tokens.
fn file_log(state_dir: &Path) -> Log {
    let path = state_dir.join("t3-client.log");
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok().map(Mutex::new);
    let echo = std::env::var_os("BUKNO_T3_LOG_STDERR").is_some();
    Arc::new(move |line: &str| {
        let line = format!("{} {line}\n", bukno_t3_client::time::now_utc());
        if let Some(file) = &file {
            let _ = file.lock().unwrap().write_all(line.as_bytes());
        }
        if echo {
            eprint!("{line}");
        }
    })
}

impl T3Source {
    pub fn start(state_dir: &Path, ctx: &egui::Context) -> Self {
        let repaint = ctx.clone();
        let hub = Hub::start(
            state_dir,
            Arc::new(SystemKeychain),
            Arc::new(move || repaint.request_repaint()),
            file_log(state_dir),
        );
        let local_config = bukno_t3_client::local::LocalConfig::for_app(state_dir);
        if std::env::var("BUKNO_T3_AUTO_SETUP").as_deref() != Ok("0") {
            hub.setup_local(local_config.clone(), false);
        }
        let view = hub.view();
        let state_file = state_dir.join("t3-client-state.json");
        // A file that cannot be read starts empty; drafts are a convenience.
        let client: ClientState =
            std::fs::read(&state_file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        // Sends from before the restart come back unconfirmed; the client
        // settles them from the chat or offers Send again with the same IDs.
        for pending in &client.pending {
            hub.adopt(&pending.environment, pending.request.clone());
        }
        Self {
            hub,
            local_config,
            advanced_setup: false,
            view,
            open: None,
            form: AddForm::default(),
            state_file,
            client,
            dirty_since: None,
            save_error: None,
            signatures: HashMap::new(),
            next_revision: 0,
            built: None,
            order: Vec::new(),
        }
    }

    /// Pick up anything the client published since the last frame.
    pub fn poll(&mut self) -> bool {
        if self.hub.revision() == self.view.revision {
            return false;
        }
        self.view = self.hub.view();
        true
    }

    pub fn prefer_project(&mut self, environment: &str, project: &str) {
        self.client.preferred_project = Some((environment.to_owned(), project.to_owned()));
        let _ = self.save();
    }

    pub fn preferred_project(&self) -> Option<(String, String)> {
        self.client.preferred_project.clone()
    }

    pub fn setup_local(&self, install: bool) {
        self.hub.setup_local(self.local_config.clone(), install);
    }

    pub fn add_local_project(&self, path: PathBuf) {
        self.hub.add_local_project(path);
    }

    pub fn ready_environment(&self) -> Option<&EnvironmentView> {
        let preferred = match &self.view.local_setup {
            bukno_t3_client::local::SetupStatus::Ready { environment_id, .. } => Some(environment_id.as_str()),
            _ => None,
        };
        let ready = |e: &&Arc<EnvironmentView>| {
            e.can_operate() && matches!(e.status, bukno_t3_client::ConnectionStatus::Connected { current: true })
        };
        self.view
            .environments
            .iter()
            .filter(ready)
            .find(|e| Some(e.saved.environment_id.as_str()) == preferred)
            .or_else(|| self.view.environments.iter().find(ready))
            .map(Arc::as_ref)
    }

    pub fn environment(&self, id: &str) -> Option<&EnvironmentView> {
        self.view.environments.iter().find(|e| e.saved.environment_id == id).map(Arc::as_ref)
    }

    pub fn open_chat(&mut self, chat: T3ChatRef) {
        if let Some(previous) = self.open.as_ref().filter(|p| p.environment != chat.environment) {
            self.hub.close_thread(&previous.environment);
        }
        self.hub.open_thread(&chat.environment, &chat.thread);
        self.client.last_open = Some((chat.environment.clone(), chat.thread.clone()));
        self.open = Some(chat);
        self.built = None;
        let _ = self.save();
    }

    pub fn close_chat(&mut self) {
        if let Some(previous) = self.open.take() {
            self.hub.close_thread(&previous.environment);
        }
        self.built = None;
        if self.client.last_open.take().is_some() {
            let _ = self.save();
        }
    }

    /// The T3 chat that was open when Bukno last quit.
    pub fn last_open(&self) -> Option<T3ChatRef> {
        let (environment, thread) = self.client.last_open.clone()?;
        Some(T3ChatRef { environment, thread })
    }

    pub fn draft(&self, key: &str) -> String {
        self.client.drafts.get(key).cloned().unwrap_or_default()
    }

    /// Keep a draft; it is written to disk by [`Self::save_drafts`].
    pub fn set_draft(&mut self, key: &str, text: &str, now: f64) {
        let changed = if text.is_empty() {
            self.client.drafts.remove(key).is_some()
        } else {
            self.client.drafts.insert(key.to_owned(), text.to_owned()).as_deref() != Some(text)
        };
        if changed && self.dirty_since.is_none() {
            self.dirty_since = Some(now);
        }
    }

    /// Write drafts once typing has paused for `debounce` seconds, or now.
    pub fn save_drafts(&mut self, now: f64, debounce: f64) -> bool {
        match self.dirty_since {
            Some(since) if now - since >= debounce => {
                let _ = self.save();
                false
            }
            Some(_) => true,
            None => false,
        }
    }

    fn save(&mut self) -> Result<(), String> {
        self.dirty_since = None;
        self.client.version = 1;
        let result = serde_json::to_vec_pretty(&self.client).map_err(|e| e.to_string()).and_then(|bytes| {
            let temp = self.state_file.with_extension("json.tmp");
            std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
            std::fs::rename(&temp, &self.state_file).map_err(|e| e.to_string())
        });
        self.save_error = result.clone().err();
        result
    }

    /// Send a command to the environment; returns its command ID.
    pub fn dispatch(&self, environment: &str, outgoing: Outgoing) -> String {
        let request = CommandRequest::new(outgoing);
        let id = request.command_id.clone();
        self.hub.dispatch(environment, request);
        id
    }

    /// Send a message or new chat from the composer with this draft key, and
    /// keep it on disk until T3 settles it.
    /// Nothing is sent unless the send is on disk first: a send T3 runs
    /// whose IDs were never saved could be sent again after a restart.
    pub fn send_tracked(
        &mut self,
        environment: &str,
        key: &str,
        draft: &str,
        outgoing: Outgoing,
    ) -> Result<String, String> {
        let request = CommandRequest::new(outgoing);
        let id = request.command_id.clone();
        self.client.pending.push(PendingSend {
            environment: environment.to_owned(),
            key: key.to_owned(),
            draft: draft.to_owned(),
            request: request.clone(),
        });
        if let Err(e) = self.save() {
            self.client.pending.retain(|p| p.request.command_id != id);
            return Err(e);
        }
        self.hub.dispatch(environment, request);
        Ok(id)
    }

    /// Hand a command the client lost (re-pairing replaced its task) back as unconfirmed.
    pub fn adopt(&self, environment: &str, request: CommandRequest) {
        self.hub.adopt(environment, request);
    }

    /// Write anything not yet on disk, now.
    pub fn flush(&mut self) {
        if self.dirty_since.is_some() {
            let _ = self.save();
        }
    }

    pub fn pending_sends(&self) -> &[PendingSend] {
        &self.client.pending
    }

    pub fn untrack(&mut self, command_id: &str) {
        let before = self.client.pending.len();
        self.client.pending.retain(|p| p.request.command_id != command_id);
        if self.client.pending.len() != before {
            let _ = self.save();
        }
    }

    /// Send an unconfirmed command again, with its original command ID.
    pub fn retry(&self, environment: &str, command_id: &str) {
        self.hub.retry(environment, command_id);
    }

    pub fn dismiss_command(&self, environment: &str, command_id: &str) {
        self.hub.dismiss_command(environment, command_id);
    }

    pub fn command(&self, environment: &str, command_id: &str) -> Option<&OutboxEntry> {
        self.environment(environment)?.command(command_id)
    }

    /// The open chat's thread view, once its first data has arrived.
    pub fn open_thread(&self) -> Option<&ThreadView> {
        let chat = self.open.as_ref()?;
        self.environment(&chat.environment)?.thread.as_ref().filter(|t| t.thread_id == chat.thread)
    }

    pub fn open_summary(&self) -> Option<&bukno_t3_client::model::ThreadShell> {
        let chat = self.open.as_ref()?;
        self.environment(&chat.environment)?.threads.iter().find(|t| t.id == chat.thread)
    }

    pub fn load_older(&self) {
        if let Some(chat) = &self.open {
            self.hub.load_older(&chat.environment);
        }
    }

    pub fn pair(&mut self) {
        let link = std::mem::take(&mut self.form.link);
        let label = format!("Bukno on {}", host_name());
        self.hub.pair(&self.form.address, &link, &label);
    }

    pub fn reconnect(&self, environment: &str) {
        self.hub.reconnect(environment);
    }

    pub fn forget(&mut self, environment: &str) {
        if self.open.as_ref().is_some_and(|c| c.environment == environment) {
            self.open = None;
            self.built = None;
        }
        self.hub.forget(environment);
    }

    pub fn clear_pairing_status(&self) {
        self.hub.clear_pairing_status();
    }

    /// Bring the document up to date with the open chat. Returns whether
    /// anything changed, and whether the chat was rebuilt from scratch.
    pub fn sync_document(&mut self, doc: &mut crate::transcript::document::Document) -> Option<bool> {
        let chat = self.open.clone()?;
        let provider = self.open_summary().map_or(Provider::Codex, |s| provider_of(s.model_selection.instance()));
        let model = self.open_summary().map(|s| s.model_selection.model.clone());
        let thread = self.open_thread()?.clone();
        if self.built.as_ref().is_some_and(|(c, rev)| *c == chat && *rev == thread.revision) {
            return None;
        }
        let items = self.transcript_items(&thread, provider, model.as_deref());
        let ids: Vec<ItemId> = items.iter().map(|i| i.id).collect();
        let fresh = self.built.as_ref().is_none_or(|(c, _)| *c != chat);
        // Rows only grow at the end while a chat streams; anything else
        // (older history, a hidden row, a reordering) rebuilds the document.
        let appended = !fresh && ids.len() >= self.order.len() && ids[..self.order.len()] == self.order[..];
        if appended {
            for item in &items {
                doc.upsert(item);
            }
        } else {
            doc.load(&items);
        }
        self.order = ids;
        self.built = Some((chat, thread.revision));
        Some(!appended)
    }

    /// Turn timeline rows into transcript items: user and assistant messages
    /// as they are, and the work between them grouped into one activity list,
    /// as T3 groups its work log.
    fn transcript_items(
        &mut self,
        thread: &ThreadView,
        provider: Provider,
        model: Option<&str>,
    ) -> Vec<TranscriptItem> {
        let task = TaskId(stable_id(&format!("t3-thread:{}", thread.thread_id)));
        let mut out = Vec::new();
        let mut group: Vec<&Arc<Row>> = Vec::new();
        let rows = thread.rows.clone();
        for row in &rows {
            match &row.item.kind {
                TurnKind::UserMessage { .. } | TurnKind::AssistantMessage { .. } | TurnKind::ProposedPlan { .. } => {
                    if !group.is_empty() {
                        let item = self.activity_item(task, &group, provider);
                        out.push(item);
                        group.clear();
                    }
                    out.push(self.message_item(task, thread, row, provider, model));
                }
                _ => group.push(row),
            }
        }
        if !group.is_empty() {
            out.push(self.activity_item(task, &group, provider));
        }
        out
    }

    fn revision_for(&mut self, id: ItemId, signature: Vec<u64>) -> u64 {
        match self.signatures.get(&id) {
            Some((seen, revision)) if *seen == signature => *revision,
            _ => {
                self.next_revision += 1;
                self.signatures.insert(id, (signature, self.next_revision));
                self.next_revision
            }
        }
    }

    fn message_item(
        &mut self,
        task: TaskId,
        thread: &ThreadView,
        row: &Row,
        provider: Provider,
        model: Option<&str>,
    ) -> TranscriptItem {
        let id = ItemId(stable_id(&format!("{}/{}", row.source_thread_id, row.source_item_id)));
        // A queued message's caption follows its run, so the run's status is part of it.
        let run_word =
            row.item.run_id.as_ref().and_then(|r| thread.run_status.get(r)).map_or(0, |s| stable_id(s) as u64);
        let revision = self.revision_for(id, vec![row.revision, run_word]);
        let (kind, text, completed) = match &row.item.kind {
            TurnKind::UserMessage { text, .. } => (ItemKind::UserMessage, text.clone(), true),
            TurnKind::AssistantMessage { text, streaming } => {
                (ItemKind::AgentMessage { provider }, text.clone(), !streaming)
            }
            TurnKind::ProposedPlan { markdown } => (ItemKind::AgentMessage { provider }, markdown.clone(), true),
            _ => unreachable!("only messages are passed here"),
        };
        let meta = match &row.item.kind {
            TurnKind::UserMessage { intent, .. } => {
                let run = row.item.run_id.as_ref().and_then(|r| thread.run_status.get(r)).map(String::as_str);
                user_caption(intent, run, provider)
            }
            TurnKind::ProposedPlan { .. } => Some("Proposed plan".to_owned()),
            _ => model.map(str::to_owned),
        };
        TranscriptItem { id, task, run: None, kind, text, meta, completed, revision }
    }

    fn activity_item(&mut self, task: TaskId, rows: &[&Arc<Row>], provider: Provider) -> TranscriptItem {
        let first = rows[0];
        let id = ItemId(stable_id(&format!("{}/{}#work", first.source_thread_id, first.source_item_id)));
        let revision = self.revision_for(id, rows.iter().map(|r| r.revision).collect());
        let lines: Vec<String> = rows.iter().map(|r| activity_line(r)).collect();
        let completed = rows.iter().all(|r| !matches!(r.item.status.as_str(), "running" | "pending" | "in_progress"));
        TranscriptItem {
            id,
            task,
            run: None,
            kind: ItemKind::AgentMessage { provider },
            text: lines.join("\n"),
            meta: Some(work_summary(rows)),
            completed,
            revision,
        }
    }
}

/// What happened to a message sent while a turn was running, from T3's
/// record of it (`inputIntent` and the run's status), never from what was asked.
fn user_caption(intent: &str, run: Option<&str>, provider: Provider) -> Option<String> {
    let name = crate::components::provider_name(provider);
    match (intent, run) {
        ("queued_turn", Some("queued")) => Some(format!("Queued for {name}'s next turn")),
        ("queued_turn", Some("cancelled")) => Some("Removed from the queue".into()),
        ("steer", _) => Some("Added to the running turn".into()),
        ("promoted_queued_to_steer", _) => Some("Moved into the running turn".into()),
        _ => None,
    }
}

/// The provider a T3 instance belongs to, for colour and the provider mark.
/// Drivers other than Claude show with the Codex mark in this version.
pub fn provider_of(instance: &str) -> Provider {
    if instance.to_ascii_lowercase().contains("claude") { Provider::Claude } else { Provider::Codex }
}

/// A 128-bit FNV-1a hash, so T3's string IDs fit Bukno's numeric item IDs.
pub fn stable_id(text: &str) -> u128 {
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    for byte in text.bytes() {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(0x0000000001000000000000000000013B);
    }
    hash
}

fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| std::env::consts::OS.to_owned())
}

fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut out: String = line.chars().take(max).collect();
    if line.chars().count() > max || text.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        out.push('…');
    }
    out.replace('`', "'")
}

/// `/bin/bash -lc "npm test"` as `npm test`, as T3's work log shows it.
pub fn unwrap_shell(command: &str) -> String {
    let trimmed = command.trim();
    let mut parts = trimmed.splitn(3, ' ');
    let (Some(program), Some(flag), Some(script)) = (parts.next(), parts.next(), parts.next()) else {
        return trimmed.to_owned();
    };
    let name = program.rsplit('/').next().unwrap_or(program);
    let shell = matches!(name, "sh" | "bash" | "zsh" | "dash" | "ksh");
    let runs_script = flag.starts_with('-') && flag.ends_with('c') && flag.len() <= 4;
    if !shell || !runs_script {
        return trimmed.to_owned();
    }
    let script = script.trim();
    for quote in ['"', '\''] {
        if script.len() >= 2 && script.starts_with(quote) && script.ends_with(quote) {
            return script[1..script.len() - 1].to_owned();
        }
    }
    script.to_owned()
}

fn activity_line(row: &Row) -> String {
    let running = matches!(row.item.status.as_str(), "running" | "pending" | "in_progress");
    let failed = row.item.status == "failed";
    let text = match &row.item.kind {
        TurnKind::CommandExecution { input, exit_code } => {
            let code = match exit_code {
                Some(code) if *code != 0 => format!(" (exit {code})"),
                _ => String::new(),
            };
            format!("Ran `{}`{code}", first_line(&unwrap_shell(input), 90))
        }
        TurnKind::FileChange { file_name, additions, deletions } => {
            let counts = match (additions, deletions) {
                (Some(a), Some(d)) => format!(" (+{a} −{d})"),
                _ => String::new(),
            };
            format!("Edited `{}`{counts}", file_name.replace('`', "'"))
        }
        TurnKind::FileSearch { pattern } => match pattern {
            Some(p) => format!("Searched files for `{}`", first_line(p, 60)),
            None => "Searched files".into(),
        },
        TurnKind::WebSearch { patterns } => match patterns.first() {
            Some(p) => format!("Searched the web for “{}”", first_line(p, 70)),
            None => "Searched the web".into(),
        },
        TurnKind::Reasoning { .. } => "Thought".into(),
        TurnKind::TodoList { steps } => {
            let done = steps.iter().filter(|(_, s)| s == "completed").count();
            format!("Updated the to-do list ({done} of {} done)", steps.len())
        }
        TurnKind::ApprovalRequest { request_kind, prompt, .. } => match prompt {
            Some(p) => format!("Asked for approval: `{}`", first_line(&unwrap_shell(p), 70)),
            None => format!("Asked for approval ({request_kind})"),
        },
        TurnKind::UserInputRequest { questions, .. } => match questions.len() {
            1 => format!("Asked: {}", first_line(&questions[0].question, 80)),
            n => format!("Asked {n} questions"),
        },
        TurnKind::Subagent { prompt, result } => {
            let state = if result.is_some() { "finished" } else { "working" };
            format!("Delegated a task, {state}: {}", first_line(prompt, 70))
        }
        TurnKind::Notice { text } => first_line(text, 100),
        TurnKind::Error { text } => format!("Error: {}", first_line(text, 100)),
        TurnKind::Quiet => match row.item.type_name.as_str() {
            "dynamic_tool" => format!("Used {}", row.item.title.clone().unwrap_or_else(|| "a tool".into())),
            "checkpoint" => "Saved a checkpoint".into(),
            "compaction" => "Compacted the context".into(),
            "handoff" => "Handed the chat to another model".into(),
            "fork" => "Forked the chat".into(),
            "thread_created" => "Started another chat".into(),
            "notification" => row.item.title.clone().unwrap_or_else(|| "Notification".into()),
            other => other.replace('_', " "),
        },
        TurnKind::Unknown => format!("Unsupported item: `{}`", row.item.type_name),
        TurnKind::UserMessage { .. } | TurnKind::AssistantMessage { .. } | TurnKind::ProposedPlan { .. } => {
            String::new()
        }
    };
    let suffix = if running {
        " (running)"
    } else if failed {
        " (failed)"
    } else {
        ""
    };
    format!("- {text}{suffix}")
}

fn work_summary(rows: &[&Arc<Row>]) -> String {
    let commands = rows.iter().filter(|r| matches!(r.item.kind, TurnKind::CommandExecution { .. })).count();
    let edits = rows.iter().filter(|r| matches!(r.item.kind, TurnKind::FileChange { .. })).count();
    let mut parts = Vec::new();
    if commands > 0 {
        parts.push(format!("{commands} command{}", if commands == 1 { "" } else { "s" }));
    }
    if edits > 0 {
        parts.push(format!("{edits} edit{}", if edits == 1 { "" } else { "s" }));
    }
    if parts.is_empty() { "Work".to_owned() } else { format!("Work · {}", parts.join(", ")) }
}

/// Status of an environment in a few words, for the sidebar and settings.
pub fn status_words(status: &bukno_t3_client::ConnectionStatus) -> (String, bool) {
    use bukno_t3_client::ConnectionStatus as S;
    match status {
        S::Connecting => ("Connecting…".into(), false),
        S::Connected { current: true } => ("Connected".into(), false),
        S::Connected { current: false } => ("Catching up…".into(), false),
        S::Reconnecting { attempt, retry_in_ms, .. } => {
            (format!("Reconnecting (try {attempt}, in {:.0} s)…", (*retry_in_ms as f64 / 1000.0).ceil()), true)
        }
        S::NeedsPairing { .. } => ("Pair again".into(), true),
        S::Blocked { .. } => ("Cannot connect".into(), true),
    }
}
