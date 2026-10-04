//! UI state and the connection to the coordinator.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use bukno_core::decision::{DecisionAnswer, DecisionView};
use bukno_core::event::{ChatSummary, NoticeTone, ProjectView, RejectReason, ViewUpdate, WaitReason};
use bukno_core::ids::{ProjectId, RunId, TaskId};
use bukno_core::message::{DeliveryState, ItemKind, Provider};
use bukno_core::run::RunState;
use bukno_platform::paths::AppPaths;
use bukno_runtime::engines::EngineView;
use bukno_runtime::synthetic::{self, Scenario};
use bukno_runtime::{Coordinator, EngineAction, SetupView, UiCommand, UiEvent};
use egui::{Key, KeyboardShortcut, Modifiers, Ui};

use crate::components::composer::{ComposerState, composer_id};
use crate::evidence::Evidence;
use crate::sources::{PendingSend, T3ChatRef, T3Source, draft_key};
use crate::theme::Theme;
use crate::transcript::TranscriptView;
use crate::transcript::document::Document;
use crate::{navigation, screens};
use bukno_t3_client::command::{Delivery, ModelChoice, Outgoing};
use bukno_t3_client::hub::OutboxState;

/// Draft autosave debounce (specification section 8).
const DRAFT_DEBOUNCE: f64 = 0.25;

/// Where commands go: the real coordinator, or a recorder in UI checks.
pub enum Backend {
    Coordinator(Coordinator),
    Recorder(Sender<UiCommand>),
}

impl Backend {
    fn send(&self, command: UiCommand) {
        match self {
            Self::Coordinator(c) => c.send(command),
            Self::Recorder(tx) => {
                let _ = tx.send(command);
            }
        }
    }
}

/// What the window is showing data from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Generated chats and a fake engine; nothing is saved.
    Synthetic,
    /// The user's chats and the installed engines.
    Real,
    /// Bukno cannot run, for example because another copy holds the state folder.
    Blocked { title: String, detail: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The selected chat.
    Chat,
    /// A new, empty chat waiting for its first message.
    NewChat,
    /// A chat on a T3 server ([`BuknoApp::remote`]).
    Remote,
    /// A new chat in a T3 project, before its first message ([`BuknoApp::remote_new`]).
    RemoteNew,
}

/// A new T3 chat being written: where it starts and with which model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteNew {
    pub environment: String,
    pub project: String,
    pub model: Option<ModelChoice>,
    pub runtime_mode: String,
}

/// A message handed to the coordinator and not yet accepted or rejected.
pub struct PendingSubmit {
    /// The draft revision that was sent. Only that revision is cleared.
    pub revision: u64,
}

pub struct ActiveRun {
    pub run: RunId,
    pub state: RunState,
    pub started: f64,
    pub has_output: bool,
}

/// A run whose outcome is unknown, with the coordinator's explanation.
#[derive(Clone, Debug)]
pub struct UnknownView {
    pub run: RunId,
    pub body: String,
    pub explanation: String,
}

/// The selected chat's state besides the composer, run and notice.
#[derive(Default)]
pub struct ChatExtra {
    pub wait: Option<(RunId, WaitReason)>,
    pub decisions: Vec<DecisionView>,
    pub unknown: Option<UnknownView>,
    /// Model and effort the engine reported for this chat.
    pub model: Option<String>,
    pub activity: Option<String>,
    pub stop_slow: Option<Vec<String>>,
    pub notice_problem: bool,
    /// Last draft revision handed to storage, and the last one it confirmed.
    pub draft_sent: u64,
    pub draft_saved: u64,
    pub draft_error: Option<String>,
    pub last_edit: Option<(u64, f64)>,
    /// Free-text answers being typed into question cards, by question ID.
    pub answers: HashMap<String, String>,
    /// The last message the engine refused, so it can go back in the composer.
    pub rejected_body: Option<String>,
}

/// A chat's state while another chat is shown.
#[derive(Default)]
pub struct ChatState {
    pub composer: ComposerState,
    pub run: Option<ActiveRun>,
    pub pending_submit: Option<PendingSubmit>,
    pub notice: Option<String>,
    pub extra: ChatExtra,
}

pub struct BuknoApp {
    pub theme: Theme,
    pub mode: Mode,
    pub scenario: Scenario,
    pub paths: AppPaths,
    backend: Backend,
    events: Receiver<UiEvent>,
    pub doc: Document,
    pub transcript: TranscriptView,
    // The selected chat.
    pub selected: Option<TaskId>,
    pub composer: ComposerState,
    pub run: Option<ActiveRun>,
    pub pending_submit: Option<PendingSubmit>,
    pub notice: Option<String>,
    pub extra: ChatExtra,
    /// Every other chat's state.
    pub stash: HashMap<TaskId, ChatState>,
    pub view: View,
    /// The project a new chat starts in; None is a projectless chat.
    pub new_chat_project: Option<ProjectId>,
    pub chats: Vec<ChatSummary>,
    pub projects: Vec<ProjectView>,
    pub engine: Option<EngineView>,
    pub setup: Option<SetupView>,
    /// Show the setup screen over the chats (from the profile row).
    pub show_setup: bool,
    /// Messages that are not about one chat.
    pub banner: Option<String>,
    pub quit: QuitFlow,
    pub menu: Option<Menu>,
    pub sidebar_open: bool,
    /// Where the sidebar's scrolling list was drawn last frame (for checks).
    pub sidebar_list: Option<egui::Rect>,
    pub projects_open: [bool; 3],
    pub open_projects: HashMap<ProjectId, bool>,
    pub reduce_motion: bool,
    /// Working treatment: the approved orb, or a proposal for comparison.
    pub working_mark: crate::components::orb::Mark,
    pub evidence: Evidence,
    window_size: egui::Vec2,
    /// Chats on T3 servers. Absent in synthetic mode and UI checks.
    pub t3: Option<T3Source>,
    /// The T3 chat shown when the view is [`View::Remote`].
    pub remote: Option<T3ChatRef>,
    pub remote_new: Option<RemoteNew>,
    /// Pending T3 sends the client has reported at least once, by command ID.
    /// Until then a send's absence only means the client has not picked it up.
    pub remote_seen: std::collections::HashSet<String>,
    /// Commands for the open T3 chat's cards, so a card shows "Sending…".
    pub remote_answers: HashMap<String, String>,
    /// The request the typed question answers belong to.
    pub remote_answers_request: Option<String>,
    /// Show the T3 servers screen (Add environment and status).
    pub show_environments: bool,
    /// Which T3 projects are folded open in the sidebar, by (environment, project).
    pub t3_open_projects: HashMap<(String, String), bool>,
    /// The composer revision last handed to the T3 draft store.
    remote_draft_revision: u64,
    /// The input time of the latest frame, for draft saves outside a frame.
    last_time: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QuitFlow {
    #[default]
    None,
    /// Work is active; Keep working or Stop and quit.
    Asking,
    /// Stopping runs and closing the engine.
    Closing,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Permission,
    NewChatProject,
}

impl BuknoApp {
    pub fn new(
        ctx: &egui::Context,
        theme: Theme,
        scenario: Scenario,
        paths: AppPaths,
        backend: Backend,
        events: Receiver<UiEvent>,
    ) -> Self {
        Self::with_mode(ctx, theme, Mode::Synthetic, scenario, paths, backend, events)
    }

    pub fn with_mode(
        ctx: &egui::Context,
        theme: Theme,
        mode: Mode,
        scenario: Scenario,
        paths: AppPaths,
        backend: Backend,
        events: Receiver<UiEvent>,
    ) -> Self {
        theme.install(ctx);
        // Reduced motion comes from the system, with an override for checks.
        let reduce_motion = match std::env::var("BUKNO_REDUCED_MOTION").ok().as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => bukno_platform::reduce_motion().unwrap_or(false),
        };
        let synthetic = mode == Mode::Synthetic;
        Self {
            theme,
            view: if !synthetic || (scenario.messages == 0 && scenario.auto_submit.is_none()) {
                View::NewChat
            } else {
                View::Chat
            },
            selected: synthetic.then_some(synthetic::TASK),
            mode,
            scenario,
            paths,
            backend,
            events,
            doc: Document::default(),
            transcript: TranscriptView::default(),
            composer: ComposerState::default(),
            run: None,
            pending_submit: None,
            notice: None,
            extra: ChatExtra::default(),
            stash: HashMap::new(),
            new_chat_project: None,
            chats: Vec::new(),
            projects: Vec::new(),
            engine: None,
            setup: None,
            show_setup: false,
            banner: None,
            quit: QuitFlow::None,
            menu: None,
            sidebar_open: true,
            sidebar_list: None,
            projects_open: [true, false, false],
            open_projects: HashMap::new(),
            reduce_motion,
            working_mark: crate::components::orb::Mark::from_env(),
            evidence: Evidence::from_env(),
            window_size: egui::Vec2::ZERO,
            t3: None,
            remote: None,
            remote_new: None,
            remote_seen: std::collections::HashSet::new(),
            remote_answers: HashMap::new(),
            remote_answers_request: None,
            show_environments: false,
            t3_open_projects: HashMap::new(),
            remote_draft_revision: 0,
            last_time: 0.0,
        }
    }

    /// Start the synthetic coordinator and connect it to a new app.
    pub fn synthetic(ctx: &egui::Context, scenario: Scenario, paths: AppPaths) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let coordinator =
            Coordinator::start_synthetic(scenario.clone(), tx, std::sync::Arc::new(move || repaint.request_repaint()));
        Self::new(ctx, Theme::load(), scenario, paths, Backend::Coordinator(coordinator), rx)
    }

    /// Start with the user's own state and engines.
    pub fn real(ctx: &egui::Context, paths: AppPaths, store: bukno_storage::Store) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let coordinator =
            Coordinator::start(paths.clone(), store, tx, std::sync::Arc::new(move || repaint.request_repaint()));
        let scenario = Scenario::named("empty").expect("empty scenario");
        let t3 = (std::env::var("BUKNO_T3").as_deref() != Ok("0")).then(|| T3Source::start(&paths.state_dir, ctx));
        let mut app =
            Self::with_mode(ctx, Theme::load(), Mode::Real, scenario, paths, Backend::Coordinator(coordinator), rx);
        let last = t3.as_ref().and_then(T3Source::last_open);
        app.t3 = t3;
        // Pick up where the user left off; nothing is resent.
        if let Some(chat) = last {
            app.select_remote(chat);
        }
        app
    }

    /// A window that only explains why Bukno cannot run.
    pub fn blocked(ctx: &egui::Context, paths: AppPaths, title: String, detail: String) -> Self {
        let (_tx, rx) = std::sync::mpsc::channel();
        let (cmd_tx, _cmd_rx) = std::sync::mpsc::channel();
        let scenario = Scenario::named("empty").expect("empty scenario");
        Self::with_mode(
            ctx,
            Theme::load(),
            Mode::Blocked { title, detail },
            scenario,
            paths,
            Backend::Recorder(cmd_tx),
            rx,
        )
    }

    pub fn is_synthetic(&self) -> bool {
        self.mode == Mode::Synthetic
    }

    pub fn provider(&self) -> Provider {
        self.selected_summary().map_or(Provider::Codex, |c| c.provider)
    }

    pub fn selected_summary(&self) -> Option<&ChatSummary> {
        let task = self.selected?;
        self.chats.iter().find(|c| c.task == task)
    }

    pub fn chat_title(&self) -> &str {
        match self.view {
            View::Chat if self.is_synthetic() => self.scenario.title,
            View::Chat => self.selected_summary().map_or("Chat", |c| c.title.as_str()),
            View::NewChat | View::RemoteNew => "New chat",
            View::Remote => self
                .t3
                .as_ref()
                .and_then(|t| t.open_summary().map(|s| s.title.as_str()))
                .or_else(|| self.t3.as_ref().and_then(|t| t.open_thread()).map(|t| t.title.as_str()))
                .unwrap_or("Chat"),
        }
    }

    pub fn send_command(&self, command: UiCommand) {
        self.backend.send(command);
    }

    // ----- Chat switching -------------------------------------------------

    fn take_current(&mut self) -> ChatState {
        ChatState {
            composer: std::mem::take(&mut self.composer),
            run: self.run.take(),
            pending_submit: self.pending_submit.take(),
            notice: self.notice.take(),
            extra: std::mem::take(&mut self.extra),
        }
    }

    fn put_current(&mut self, state: ChatState) {
        self.composer = state.composer;
        self.run = state.run;
        self.pending_submit = state.pending_submit;
        self.notice = state.notice;
        self.extra = state.extra;
    }

    /// Apply `f` to a chat's state, whether it is shown or not.
    fn with_chat(&mut self, task: TaskId, f: impl FnOnce(&mut ChatState, f64), now: f64) {
        // Synthetic checks drive the scenario chat from the new-chat screen too.
        if self.selected == Some(task) && (self.view == View::Chat || self.is_synthetic()) {
            let mut state = self.take_current();
            f(&mut state, now);
            self.put_current(state);
        } else {
            f(self.stash.entry(task).or_default(), now);
        }
    }

    /// Show `task`, keeping the previous chat's draft and state.
    pub fn select_chat(&mut self, task: TaskId) {
        if self.selected == Some(task) && self.view == View::Chat {
            return;
        }
        self.flush_draft();
        self.leave_remote();
        let current = self.take_current();
        match (self.view, self.selected) {
            (View::Chat, Some(previous)) => {
                self.stash.insert(previous, current);
            }
            // A T3 chat's draft was kept by leave_remote.
            (View::Remote | View::RemoteNew, _) => {}
            // The new-chat composer's draft is kept in memory for the next new chat.
            _ => {
                self.stash.insert(NEW_CHAT, current);
            }
        }
        let next = self.stash.remove(&task).unwrap_or_default();
        self.put_current(next);
        self.selected = Some(task);
        self.view = View::Chat;
        self.menu = None;
        self.doc = Document::default();
        self.transcript = TranscriptView::default();
        self.transcript.scroll_to_end();
        if !self.is_synthetic() {
            self.backend.send(UiCommand::SelectChat { task });
        }
    }

    /// Show the new-chat screen, optionally in a project.
    pub fn new_chat(&mut self, project: Option<ProjectId>) {
        self.flush_draft();
        let was_remote = matches!(self.view, View::Remote | View::RemoteNew);
        self.leave_remote();
        if self.view == View::Chat || was_remote {
            let current = self.take_current();
            if let (View::Chat, Some(previous)) = (self.view, self.selected) {
                self.stash.insert(previous, current);
            }
            let fresh = self.stash.remove(&NEW_CHAT).unwrap_or_default();
            self.put_current(fresh);
        }
        self.view = View::NewChat;
        self.new_chat_project = project;
        self.menu = None;
    }

    /// Show a chat that lives on a T3 server, with its own saved draft. The
    /// local chat's draft is kept for later.
    pub fn select_remote(&mut self, chat: T3ChatRef) {
        if self.view == View::Remote && self.remote.as_ref() == Some(&chat) {
            return;
        }
        self.flush_draft();
        self.keep_remote_draft();
        if matches!(self.view, View::Remote | View::RemoteNew) {
            self.remote_new = None;
        }
        match (self.view, self.selected) {
            (View::Chat, Some(previous)) => {
                let current = self.take_current();
                self.stash.insert(previous, current);
            }
            (View::NewChat, _) => {
                let current = self.take_current();
                self.stash.insert(NEW_CHAT, current);
            }
            _ => {}
        }
        let mut state = ChatState::default();
        if let Some(t3) = self.t3.as_ref() {
            state.composer.text = t3.draft(&draft_key(&chat.environment, Some(&chat.thread), None));
        }
        self.put_current(state);
        self.view = View::Remote;
        self.menu = None;
        self.remote_answers.clear();
        self.doc = Document::default();
        self.transcript = TranscriptView::default();
        self.transcript.scroll_to_end();
        if let Some(t3) = self.t3.as_mut() {
            t3.open_chat(chat.clone());
        }
        self.remote = Some(chat);
    }

    /// Start a new chat in a T3 project, with the server's default model.
    pub fn new_remote_chat(&mut self, environment: String, project: String) {
        self.flush_draft();
        self.keep_remote_draft();
        match (self.view, self.selected) {
            (View::Chat, Some(previous)) => {
                let current = self.take_current();
                self.stash.insert(previous, current);
            }
            (View::NewChat, _) => {
                let current = self.take_current();
                self.stash.insert(NEW_CHAT, current);
            }
            _ => {}
        }
        if self.view == View::Remote
            && let Some(t3) = self.t3.as_mut()
        {
            t3.close_chat();
        }
        self.remote = None;
        let model = self.t3.as_ref().and_then(|t| default_model(t, &environment));
        let mut state = ChatState::default();
        if let Some(t3) = self.t3.as_ref() {
            state.composer.text = t3.draft(&draft_key(&environment, None, Some(&project)));
        }
        self.put_current(state);
        self.remote_new = Some(RemoteNew { environment, project, model, runtime_mode: "approval-required".into() });
        self.view = View::RemoteNew;
        self.menu = None;
    }

    /// The draft key of the T3 composer on screen.
    pub fn remote_draft_key(&self) -> Option<String> {
        match self.view {
            View::Remote => self.remote.as_ref().map(|c| draft_key(&c.environment, Some(&c.thread), None)),
            View::RemoteNew => self.remote_new.as_ref().map(|n| draft_key(&n.environment, None, Some(&n.project))),
            _ => None,
        }
    }

    /// Hand the T3 composer's text to the draft store.
    fn keep_remote_draft(&mut self) {
        let (Some(key), now) = (self.remote_draft_key(), self.last_time) else { return };
        let text = self.composer.text.clone();
        if let Some(t3) = self.t3.as_mut() {
            t3.set_draft(&key, &text, now);
            t3.save_drafts(now, 0.0);
        }
    }

    /// Stop following the open T3 chat when another chat is shown.
    fn leave_remote(&mut self) {
        if !matches!(self.view, View::Remote | View::RemoteNew) {
            return;
        }
        self.keep_remote_draft();
        if let Some(t3) = self.t3.as_mut() {
            t3.close_chat();
        }
        self.remote = None;
        self.remote_new = None;
    }

    /// The environment the T3 view on screen belongs to.
    pub fn remote_environment(&self) -> Option<&str> {
        match self.view {
            View::Remote => self.remote.as_ref().map(|c| c.environment.as_str()),
            View::RemoteNew => self.remote_new.as_ref().map(|n| n.environment.as_str()),
            _ => None,
        }
    }

    /// Why the T3 composer cannot send right now, if it cannot.
    pub fn remote_send_blocked(&self) -> Option<&'static str> {
        let t3 = self.t3.as_ref()?;
        let env = self.remote_environment().and_then(|e| t3.environment(e));
        let Some(env) = env else { return Some("This T3 server is not paired any more.") };
        if !env.can_operate() {
            return Some("Pair this server again to send from Bukno. Your draft is kept.");
        }
        if !env.ready_to_send() {
            return Some("Not connected to T3. Your draft is kept.");
        }
        // Only the composer a pending message was sent from waits for it,
        // from the moment it is sent, before the client has even reported it.
        let key = self.remote_draft_key();
        for pending in t3.pending_sends().iter().filter(|p| Some(&p.key) == key.as_ref()) {
            match t3.command(&pending.environment, &pending.request.command_id).map(|e| &e.state) {
                None | Some(OutboxState::Sending) => return Some("Sending…"),
                Some(OutboxState::Unconfirmed) => {
                    return Some("The last message is not confirmed yet. Your draft is kept.");
                }
                _ => {}
            }
        }
        if self.view == View::RemoteNew && self.remote_new.as_ref().is_some_and(|n| n.model.is_none()) {
            return Some("This T3 server offers no model to start a chat with.");
        }
        if self.view == View::Remote && t3.open_thread().is_some_and(|t| t.removed) {
            return Some("This chat was deleted in T3.");
        }
        None
    }

    /// Send the T3 composer's text: a new chat, or a message in the open chat.
    pub fn remote_submit(&mut self, delivery: Delivery) {
        let body = self.composer.text.trim().to_owned();
        if body.is_empty() || self.remote_send_blocked().is_some() {
            return;
        }
        let Some(key) = self.remote_draft_key() else { return };
        let Some(t3) = self.t3.as_ref() else { return };
        self.notice = None;
        let message_id = bukno_t3_client::command::new_id();
        let (environment, outgoing) = match self.view {
            View::RemoteNew => {
                let Some(new) = self.remote_new.clone() else { return };
                let Some(model) = new.model else { return };
                let outgoing = Outgoing::Launch {
                    thread_id: bukno_t3_client::command::new_id(),
                    message_id,
                    project_id: new.project,
                    title: bukno_t3_client::command::title_from(&body),
                    model,
                    runtime_mode: new.runtime_mode,
                    text: body.clone(),
                };
                (new.environment, outgoing)
            }
            View::Remote => {
                let Some(chat) = self.remote.clone() else { return };
                // While idle every send starts a turn; T3 picks the run itself.
                let running = t3.open_thread().is_some_and(|t| t.active_run.is_some());
                let delivery = if running { delivery } else { Delivery::Auto };
                let outgoing = Outgoing::Send { thread_id: chat.thread, message_id, text: body.clone(), delivery };
                (chat.environment, outgoing)
            }
            _ => return,
        };
        if let Some(t3) = self.t3.as_mut()
            && let Err(e) = t3.send_tracked(&environment, &key, &body, outgoing)
        {
            self.notice = Some(format!("Not sent, because Bukno could not save it first ({e}). Your draft is kept."));
            self.extra.notice_problem = true;
        }
    }

    /// Stop the open T3 chat's running turn. Queued messages wait.
    pub fn remote_stop(&mut self) {
        let (Some(t3), Some(chat)) = (self.t3.as_ref(), self.remote.as_ref()) else { return };
        let Some(run_id) = t3.open_thread().and_then(|t| t.active_run.clone()) else { return };
        t3.dispatch(&chat.environment, Outgoing::Interrupt { thread_id: chat.thread.clone(), run_id });
    }

    /// Send a command about the open T3 chat. `key` ties it to a card, so
    /// the card shows it is being sent.
    pub fn remote_command(&mut self, key: Option<String>, outgoing: Outgoing) {
        let (Some(t3), Some(chat)) = (self.t3.as_ref(), self.remote.as_ref()) else { return };
        let command_id = t3.dispatch(&chat.environment, outgoing);
        if let Some(key) = key {
            self.remote_answers.insert(key, command_id);
        }
    }

    /// Settle the T3 composer's pending send, and keep drafts saved.
    fn poll_remote(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.last_time = now;
        if let Some(key) = self.remote_draft_key()
            && self.composer.revision != self.remote_draft_revision
        {
            self.remote_draft_revision = self.composer.revision;
            let text = self.composer.text.clone();
            if let Some(t3) = self.t3.as_mut() {
                t3.set_draft(&key, &text, now);
            }
        }
        if let Some(t3) = self.t3.as_mut()
            && t3.save_drafts(now, DRAFT_DEBOUNCE)
        {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(DRAFT_DEBOUNCE));
        }
        let pending: Vec<PendingSend> = self.t3.as_ref().map(|t| t.pending_sends().to_vec()).unwrap_or_default();
        for send in pending {
            self.settle_pending(&send, now);
        }
    }

    /// Clear the draft of a send T3 accepted, or show why it refused.
    fn settle_pending(&mut self, pending: &PendingSend, now: f64) {
        let id = pending.request.command_id.clone();
        let Some(t3) = self.t3.as_mut() else { return };
        let known = t3.environment(&pending.environment).is_some();
        let Some(entry) = t3.command(&pending.environment, &id).cloned() else {
            if !known {
                // Its server was removed: nothing can settle it any more.
                t3.untrack(&id);
                self.remote_seen.remove(&id);
            } else if self.remote_seen.contains(&id) {
                // The client lost it (pairing again replaced its task). It is
                // still in doubt: hand it back. Only Dismiss forgets a send.
                t3.adopt(&pending.environment, pending.request.clone());
            }
            return;
        };
        self.remote_seen.insert(id.clone());
        if matches!(entry.state, OutboxState::Sending | OutboxState::Unconfirmed) {
            return;
        }
        let here = self.remote_draft_key().as_ref() == Some(&pending.key);
        match entry.state {
            OutboxState::Rejected(reason) => {
                // Nothing ran; the draft is still there to edit.
                if here {
                    self.notice = Some(reason);
                    self.extra.notice_problem = true;
                }
                if let Some(t3) = self.t3.as_mut() {
                    t3.untrack(&id);
                    t3.dismiss_command(&pending.environment, &id);
                }
                self.remote_seen.remove(&id);
            }
            OutboxState::Accepted { thread_id, .. } => {
                // Clear exactly the text that was sent; anything else typed
                // stays. The cleared draft and the settled send are written
                // together, so a quit cannot keep one without the other.
                if here && self.composer.text.trim() == pending.draft {
                    self.composer.text.clear();
                    self.composer.revision += 1;
                }
                if let Some(t3) = self.t3.as_mut() {
                    if t3.draft(&pending.key).trim() == pending.draft {
                        t3.set_draft(&pending.key, "", now);
                    }
                    t3.untrack(&id);
                }
                self.remote_seen.remove(&id);
                if matches!(entry.request.outgoing, Outgoing::Launch { .. })
                    && here
                    && let Some(thread) = thread_id
                {
                    let chat = T3ChatRef { environment: pending.environment.clone(), thread };
                    self.keep_remote_draft();
                    self.remote_new = None;
                    self.view = View::Remote;
                    self.select_remote_fresh(chat);
                }
            }
            OutboxState::Sending | OutboxState::Unconfirmed => {}
        }
    }

    /// Open a chat that was just created here, scrolled to its end.
    fn select_remote_fresh(&mut self, chat: T3ChatRef) {
        self.remote = None;
        self.doc = Document::default();
        self.transcript = TranscriptView::default();
        self.transcript.scroll_to_end();
        self.remote_answers.clear();
        if let Some(t3) = self.t3.as_mut() {
            t3.open_chat(chat.clone());
        }
        self.remote = Some(chat);
    }

    /// Read what the T3 client published, and keep the open chat's document current.
    fn poll_t3(&mut self, ctx: &egui::Context) {
        let Some(t3) = self.t3.as_mut() else { return };
        t3.poll();
        self.poll_remote(ctx);
        let Some(t3) = self.t3.as_mut() else { return };
        if self.view != View::Remote {
            return;
        }
        if let Some(rebuilt) = t3.sync_document(&mut self.doc)
            && rebuilt
            && self.transcript.is_following()
        {
            self.transcript.scroll_to_end();
        }
    }

    // ----- Events ---------------------------------------------------------

    fn apply_events(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::View(update) => self.apply_view(update, now),
                UiEvent::Rejected { task, reason } => {
                    let words = rejection_words(&reason);
                    let apply = move |chat: &mut ChatState, _: f64| {
                        // The draft was never cleared, so it is still there to edit.
                        chat.pending_submit = None;
                        chat.notice = Some(words);
                        chat.extra.notice_problem = true;
                    };
                    if self.view == View::NewChat && self.pending_submit.is_some() {
                        let mut state = self.take_current();
                        apply(&mut state, now);
                        self.put_current(state);
                    } else {
                        self.with_chat(task, apply, now);
                    }
                }
                UiEvent::Engine(view) => self.engine = Some(view),
                UiEvent::Setup(view) => {
                    // First launch: stay on setup until the user continues.
                    if self.setup.is_none() && view.work_folder.is_none() {
                        self.show_setup = true;
                    }
                    self.setup = Some(view);
                }
                UiEvent::Opened { task } => {
                    // The first message created this chat: its new-chat state moves
                    // with it. If the user moved on before this arrived, that state
                    // was stashed and they stay where they are.
                    if self.view == View::NewChat {
                        let state = self.take_current();
                        self.selected = Some(task);
                        self.view = View::Chat;
                        self.put_current(state);
                        self.doc = Document::default();
                        self.transcript = TranscriptView::default();
                    } else if let Some(state) = self.stash.remove(&NEW_CHAT) {
                        self.stash.insert(task, state);
                    }
                }
                UiEvent::Notice(text) => self.banner = Some(text),
                UiEvent::QuitDone(report) => {
                    self.quit = QuitFlow::Done;
                    eprintln!(
                        "Bukno quit: engine exited by itself {}, forced {}, leftover {:?}, saved {}",
                        report.engine.exited_by_itself, report.engine.forced, report.engine.leftover, report.saved
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn apply_view(&mut self, update: ViewUpdate, now: f64) {
        match update {
            ViewUpdate::Chats { projects, chats } => {
                for project in &projects {
                    self.open_projects.entry(project.id).or_insert(true);
                }
                self.projects = projects;
                self.chats = chats;
            }
            ViewUpdate::ConversationLoaded { task, items } => {
                // A late load for the local chat must not replace a T3 chat on screen.
                if self.selected == Some(task) && self.view != View::Remote {
                    self.doc.load(&items);
                }
            }
            ViewUpdate::DraftLoaded { task, text, revision } => self.with_chat(
                task,
                |chat, _| {
                    // Never replace what the user typed since.
                    if chat.composer.text.is_empty() {
                        chat.composer.text = text;
                        chat.composer.revision = chat.composer.revision.max(revision);
                        chat.extra.draft_sent = chat.composer.revision;
                        chat.extra.draft_saved = chat.composer.revision;
                    } else if chat.composer.revision <= revision {
                        // Typed before the saved draft arrived: number it after the
                        // saved one, or storage would keep the older text. A send of
                        // that same text moves with it, so acceptance still clears it.
                        let rebased = revision + 1;
                        if let Some(pending) = chat.pending_submit.as_mut()
                            && pending.revision == chat.composer.revision
                        {
                            pending.revision = rebased;
                        }
                        chat.composer.revision = rebased;
                    }
                },
                now,
            ),
            ViewUpdate::DraftSaved { task, revision } => self.with_chat(
                task,
                |chat, _| {
                    chat.extra.draft_saved = chat.extra.draft_saved.max(revision);
                    chat.extra.draft_error = None;
                },
                now,
            ),
            ViewUpdate::DraftFailed { task, reason } => {
                self.with_chat(task, |chat, _| chat.extra.draft_error = Some(reason), now);
            }
            ViewUpdate::ItemUpserted(item) => {
                let task = item.task;
                let user = item.kind == ItemKind::UserMessage;
                let shown = self.selected == Some(task) && (self.view == View::Chat || self.is_synthetic());
                let new_chat_send = self.view == View::NewChat && self.pending_submit.is_some() && user;
                if new_chat_send {
                    // Only in synthetic checks: the new chat is the scenario chat.
                    self.view = View::Chat;
                }
                let run_id = item.run;
                self.with_chat(
                    task,
                    |chat, _| {
                        if user && let Some(pending) = chat.pending_submit.take() {
                            // Accepted. Clear only the revision that was sent; anything
                            // typed since stays.
                            if chat.composer.revision == pending.revision {
                                chat.composer.text.clear();
                                chat.composer.revision += 1;
                            }
                            chat.notice = None;
                        }
                        if !user
                            && let Some(run) = chat.run.as_mut()
                            && run_id == Some(run.run)
                        {
                            run.has_output = true;
                        }
                    },
                    now,
                );
                if shown || new_chat_send {
                    if user {
                        self.transcript.scroll_to_end();
                    }
                    self.doc.upsert(&item);
                }
            }
            ViewUpdate::RunStateChanged { task, run, state } => self.with_chat(
                task,
                |chat, now| {
                    match chat.run.as_mut() {
                        Some(active) if active.run == run => active.state = state,
                        _ => chat.run = Some(ActiveRun { run, state, started: now, has_output: false }),
                    }
                    if state.is_terminal() {
                        chat.run = None;
                        chat.extra.activity = None;
                        chat.extra.stop_slow = None;
                        if chat.extra.wait.as_ref().is_some_and(|(r, _)| *r == run) {
                            chat.extra.wait = None;
                        }
                    }
                },
                now,
            ),
            ViewUpdate::RunWaiting { task, run, reason } => {
                self.with_chat(task, |chat, _| chat.extra.wait = reason.map(|r| (run, r)), now);
            }
            ViewUpdate::DeliveryChanged { task, message: _, state } => {
                if state == DeliveryState::Rejected {
                    self.with_chat(
                        task,
                        |chat, _| {
                            // Put a refused message back so it can be edited and sent again.
                            if let Some(body) = chat.extra.rejected_body.take()
                                && chat.composer.text.is_empty()
                            {
                                chat.composer.text = body;
                                chat.composer.revision += 1;
                            }
                        },
                        now,
                    );
                }
            }
            ViewUpdate::Activity { task, run: _, text } => {
                self.with_chat(task, |chat, _| chat.extra.activity = Some(text), now);
            }
            ViewUpdate::Decisions { task, decisions } => {
                self.with_chat(task, |chat, _| chat.extra.decisions = decisions, now);
            }
            ViewUpdate::SessionModel { task, model } => {
                self.with_chat(task, |chat, _| chat.extra.model = Some(model), now);
            }
            ViewUpdate::Notice { task, notice } => self.with_chat(
                task,
                |chat, _| {
                    chat.notice = Some(notice.text);
                    chat.extra.notice_problem = notice.tone == NoticeTone::Problem;
                },
                now,
            ),
            ViewUpdate::StopSlow { task, run: _, shared } => {
                self.with_chat(task, |chat, _| chat.extra.stop_slow = Some(shared), now);
            }
            ViewUpdate::Unknown { task, run, body, explanation } => {
                self.with_chat(task, |chat, _| chat.extra.unknown = Some(UnknownView { run, body, explanation }), now);
            }
            ViewUpdate::UnknownCleared { task, run } => self.with_chat(
                task,
                |chat, _| {
                    if chat.extra.unknown.as_ref().is_some_and(|u| u.run == run) {
                        chat.extra.unknown = None;
                    }
                },
                now,
            ),
            ViewUpdate::StorageProblem { reason } => {
                self.banner = Some(format!("Bukno cannot save right now, so nothing new is sent: {reason}"));
            }
            ViewUpdate::QuitReady => {}
        }
    }

    // ----- Actions --------------------------------------------------------

    /// Why Send is unavailable, if it is.
    pub fn send_blocked(&self) -> Option<&'static str> {
        if self.pending_submit.is_some() {
            Some("Sending…")
        } else if self.run.is_some() {
            // Mid-run direction lands in a later pass.
            Some("You can send when this run finishes. Your draft is kept.")
        } else if self.view == View::Chat && self.selected_summary().is_some_and(|c| !c.available) {
            Some("This chat's folder is missing, so nothing can be sent.")
        } else if self.view == View::NewChat
            && self.new_chat_project.is_none()
            && !self.is_synthetic()
            && self.setup.as_ref().is_some_and(|s| s.work_folder_missing)
        {
            Some("The work folder is missing, so a new chat cannot start.")
        } else {
            None
        }
    }

    pub fn preset(&self) -> Option<String> {
        self.setup.as_ref().map(|s| s.preset.clone())
    }

    pub fn submit(&mut self) {
        let body = self.composer.text.trim().to_owned();
        if body.is_empty() || self.send_blocked().is_some() {
            return;
        }
        self.notice = None;
        self.extra.rejected_body = Some(body.clone());
        self.pending_submit = Some(PendingSubmit { revision: self.composer.revision });
        let preset = self.preset();
        if self.view == View::NewChat && !self.is_synthetic() {
            self.backend.send(UiCommand::SubmitNew {
                project: self.new_chat_project,
                provider: Provider::Codex,
                body,
                preset,
            });
            return;
        }
        let Some(task) = self.selected else {
            return;
        };
        self.backend.send(UiCommand::Submit {
            task,
            provider: self.provider(),
            draft_revision: self.composer.revision,
            body,
            preset,
        });
    }

    pub fn stop(&mut self) {
        if let (Some(run), Some(task)) = (&self.run, self.selected) {
            self.backend.send(UiCommand::Interrupt { task, run: run.run });
        }
    }

    pub fn answer(&mut self, decision: &DecisionView, answer: DecisionAnswer) {
        self.backend.send(UiCommand::Answer {
            decision: decision.id,
            run: decision.run,
            generation: decision.generation,
            answer,
        });
    }

    pub fn engine_action(&self, action: EngineAction) {
        self.backend.send(UiCommand::Engine(action));
    }

    /// Hand the draft to storage once typing pauses, and right away on switch.
    fn autosave(&mut self, ctx: &egui::Context) {
        if self.is_synthetic() || self.view != View::Chat {
            return;
        }
        let now = ctx.input(|i| i.time);
        let revision = self.composer.revision;
        if revision <= self.extra.draft_sent {
            return;
        }
        match self.extra.last_edit {
            Some((r, at)) if r == revision => {
                if now - at >= DRAFT_DEBOUNCE {
                    self.flush_draft();
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(DRAFT_DEBOUNCE - (now - at)));
                }
            }
            _ => {
                self.extra.last_edit = Some((revision, now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(DRAFT_DEBOUNCE));
            }
        }
    }

    pub fn flush_draft(&mut self) {
        if self.is_synthetic() || self.view != View::Chat || self.composer.revision <= self.extra.draft_sent {
            return;
        }
        let Some(task) = self.selected else {
            return;
        };
        self.extra.draft_sent = self.composer.revision;
        self.backend.send(UiCommand::SaveDraft {
            task,
            revision: self.composer.revision,
            text: self.composer.text.clone(),
        });
    }

    /// Any chat still working, waiting on the user, or about to send.
    pub fn work_active(&self) -> bool {
        self.chats.iter().any(|c| {
            matches!(c.activity, bukno_core::event::ChatActivity::Working | bukno_core::event::ChatActivity::NeedsYou)
        })
    }

    /// Closing the window runs the quit flow (section 16).
    fn on_close_request(&mut self, ctx: &egui::Context) {
        if self.mode != Mode::Real || self.quit == QuitFlow::Done {
            return;
        }
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if self.quit == QuitFlow::Closing {
            return;
        }
        self.flush_draft();
        // T3 drafts are written now, not after the typing pause.
        self.keep_remote_draft();
        if let Some(t3) = self.t3.as_mut() {
            t3.flush();
        }
        if self.work_active() {
            self.quit = QuitFlow::Asking;
        } else {
            self.quit = QuitFlow::Closing;
            self.backend.send(UiCommand::Quit { stop: true });
        }
    }

    pub fn confirm_quit(&mut self) {
        self.flush_draft();
        self.keep_remote_draft();
        if let Some(t3) = self.t3.as_mut() {
            t3.flush();
        }
        self.quit = QuitFlow::Closing;
        self.backend.send(UiCommand::Quit { stop: true });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let new_chat = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
        if ctx.input_mut(|i| i.consume_shortcut(&new_chat)) {
            if self.is_synthetic() {
                self.view = View::NewChat;
            } else {
                self.new_chat(None);
            }
            ctx.memory_mut(|m| m.request_focus(composer_id()));
        }
        let toggle = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
        if ctx.input_mut(|i| i.consume_shortcut(&toggle)) {
            self.sidebar_open = !self.sidebar_open;
        }
    }

    /// The whole window: sidebar, titlebar and the chat canvas.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.evidence.frame_start(&ctx);
        self.apply_events(&ctx);
        self.poll_t3(&ctx);
        self.on_close_request(&ctx);
        crate::components::update_keyboard_mode(&ctx);

        let full = ui.max_rect();
        if let Mode::Blocked { title, detail } = &self.mode {
            let (title, detail) = (title.clone(), detail.clone());
            screens::setup::blocked(self, ui, full, &title, &detail);
            return;
        }
        let needs_setup = self.mode == Mode::Real && self.setup.as_ref().is_none_or(|s| s.work_folder.is_none());
        if needs_setup || self.show_setup {
            ui.painter().rect_filled(full, 0.0, self.theme.color.surface_canvas);
            screens::setup::show(self, ui, full);
            return;
        }
        if self.show_environments && self.t3.is_some() {
            ui.painter().rect_filled(full, 0.0, self.theme.color.surface_canvas);
            screens::environments::show(self, ui, full);
            return;
        }
        self.shortcuts(&ctx);
        self.autosave(&ctx);

        let sidebar = navigation::sidebar_rect(self, full);
        if let Some(rect) = sidebar {
            navigation::sidebar(self, ui, rect);
        }
        let canvas =
            egui::Rect::from_min_max(egui::pos2(sidebar.map_or(full.left(), |r| r.right()), full.top()), full.max);
        ui.painter().rect_filled(canvas, 0.0, self.theme.color.surface_canvas);
        screens::chat::titlebar(self, ui, canvas, sidebar.is_some());
        screens::chat::show(self, ui, canvas);
        if self.quit != QuitFlow::None {
            screens::chat::quit_dialog(self, ui, full);
        }

        self.evidence.frame_end(&ctx, &mut self.transcript, &self.doc, &self.theme);
    }

    /// Keep the macOS traffic lights centred in the 44-point titlebar.
    pub fn place_window_buttons(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        let size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()).unwrap_or_default());
        if size == self.window_size {
            return;
        }
        self.window_size = size;
        #[cfg(target_os = "macos")]
        {
            use eframe::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = frame.window_handle()
                && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
            {
                let center = f64::from(self.theme.size.size_titlebar) / 2.0;
                // SAFETY: eframe gives us the live view, and ui() runs on the main thread.
                unsafe { bukno_platform::place_window_buttons(appkit.ns_view, 18.0, center, 20.0) };
                // SAFETY: as above.
                self.evidence.window_buttons = unsafe { bukno_platform::window_button_frames(appkit.ns_view) };
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = frame;
    }
}

/// Stash key for the new-chat composer.
const NEW_CHAT: TaskId = TaskId(0);

/// The model a new chat on this server starts with: the first ready provider's
/// default model.
pub fn default_model(t3: &T3Source, environment: &str) -> Option<ModelChoice> {
    let env = t3.environment(environment)?;
    env.providers.iter().filter(|p| p.enabled && p.installed).find_map(|p| {
        let model = p.models.iter().find(|m| m.is_default == Some(true)).or_else(|| p.models.first())?;
        Some(ModelChoice { instance_id: p.instance_id.clone(), model: model.slug.clone() })
    })
}

pub fn rejection_words(reason: &RejectReason) -> String {
    use RejectReason as R;
    match reason {
        R::EmptyMessage => "Nothing to send.".into(),
        R::DuplicateMessage => "That message was already sent.".into(),
        R::RunActive => "Not sent: this chat is still working. Your draft is kept.".into(),
        R::UnknownRun => "That run is no longer active.".into(),
        R::UnknownChat => "That chat no longer exists. Your draft is kept.".into(),
        R::Unavailable => "Not sent: this chat's folder is missing. Your draft is kept.".into(),
        R::WrongProvider => "Not sent: this chat belongs to the other engine. Your draft is kept.".into(),
        R::StaleDecision => "That request was already answered or has expired.".into(),
        R::NotSaved(why) => format!("Not sent, because it could not be saved first ({why}). Your draft is kept."),
        R::StorageUnsafe => "Not sent: Bukno cannot save right now. Your draft is kept.".into(),
        R::Quitting => "Not sent: Bukno is quitting.".into(),
    }
}

impl eframe::App for BuknoApp {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.place_window_buttons(frame, &ctx);
        self.show(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.color.surface_canvas.to_normalized_gamma_f32()
    }
}
