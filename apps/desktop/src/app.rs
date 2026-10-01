//! UI state and the connection to the coordinator.

use std::sync::mpsc::{Receiver, Sender};

use bukno_core::event::ViewUpdate;
use bukno_core::ids::RunId;
use bukno_core::message::Provider;
use bukno_core::run::RunState;
use bukno_platform::paths::AppPaths;
use bukno_runtime::synthetic::{self, Scenario};
use bukno_runtime::{Coordinator, UiCommand, UiEvent};
use egui::{Key, KeyboardShortcut, Modifiers, Ui};

use crate::components::composer::{ComposerState, composer_id};
use crate::evidence::Evidence;
use crate::theme::Theme;
use crate::transcript::TranscriptView;
use crate::transcript::document::Document;
use crate::{navigation, screens};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The scenario's chat.
    Chat,
    /// A new, empty chat waiting for its first message.
    NewChat,
}

pub struct ActiveRun {
    pub run: RunId,
    pub state: RunState,
    pub started: f64,
    pub has_output: bool,
}

pub struct BuknoApp {
    pub theme: Theme,
    pub scenario: Scenario,
    pub paths: AppPaths,
    backend: Backend,
    events: Receiver<UiEvent>,
    pub doc: Document,
    pub transcript: TranscriptView,
    pub composer: ComposerState,
    pub run: Option<ActiveRun>,
    pub view: View,
    pub sidebar_open: bool,
    pub projects_open: [bool; 3],
    pub reduce_motion: bool,
    pub notice: Option<String>,
    pub evidence: Evidence,
    window_size: egui::Vec2,
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
        theme.install(ctx);
        // Reduced motion comes from the system, with an override for checks.
        let reduce_motion = match std::env::var("BUKNO_REDUCED_MOTION").ok().as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => bukno_platform::reduce_motion().unwrap_or(false),
        };
        Self {
            theme,
            view: if scenario.messages == 0 && scenario.auto_submit.is_none() { View::NewChat } else { View::Chat },
            scenario,
            paths,
            backend,
            events,
            doc: Document::default(),
            transcript: TranscriptView::default(),
            composer: ComposerState::default(),
            run: None,
            sidebar_open: true,
            projects_open: [true, false, false],
            reduce_motion,
            notice: None,
            evidence: Evidence::from_env(),
            window_size: egui::Vec2::ZERO,
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

    pub fn provider(&self) -> Provider {
        Provider::Codex
    }

    pub fn chat_title(&self) -> &str {
        match self.view {
            View::Chat => self.scenario.title,
            View::NewChat => "New chat",
        }
    }

    fn apply_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::View(ViewUpdate::ConversationLoaded { task, items }) if task == synthetic::TASK => {
                    self.doc.load(&items);
                }
                UiEvent::View(ViewUpdate::ItemUpserted(item)) if item.task == synthetic::TASK => {
                    if matches!(item.kind, bukno_core::message::ItemKind::AgentMessage { .. })
                        && let Some(run) = self.run.as_mut()
                        && item.run == Some(run.run)
                    {
                        run.has_output = true;
                    }
                    self.doc.upsert(&item);
                }
                UiEvent::View(ViewUpdate::RunStateChanged { run, state, .. }) => {
                    let now = ctx.input(|i| i.time);
                    match self.run.as_mut() {
                        Some(active) if active.run == run => active.state = state,
                        _ => self.run = Some(ActiveRun { run, state, started: now, has_output: false }),
                    }
                    if state.is_terminal() {
                        self.run = None;
                    }
                }
                UiEvent::View(_) => {}
                UiEvent::Rejected { reason, .. } => {
                    self.notice = Some(format!("Not sent: {reason:?}"));
                }
            }
        }
    }

    pub fn submit(&mut self) {
        let body = self.composer.text.trim().to_owned();
        if body.is_empty() {
            return;
        }
        self.backend.send(UiCommand::Submit {
            task: synthetic::TASK,
            provider: self.provider(),
            draft_revision: self.composer.revision,
            body,
        });
        // Pass 0 has no outbox acknowledgement for drafts; the synthetic
        // coordinator accepts immediately, so the draft clears on send.
        self.composer.text.clear();
        self.composer.revision += 1;
        self.view = View::Chat;
        self.transcript.scroll_to_end();
    }

    pub fn stop(&mut self) {
        if let Some(run) = &self.run {
            self.backend.send(UiCommand::Interrupt { task: synthetic::TASK, run: run.run });
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let new_chat = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
        if ctx.input_mut(|i| i.consume_shortcut(&new_chat)) {
            self.view = View::NewChat;
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
        crate::components::update_keyboard_mode(&ctx);
        self.shortcuts(&ctx);

        let full = ui.max_rect();
        let sidebar = navigation::sidebar_rect(self, full);
        if let Some(rect) = sidebar {
            navigation::sidebar(self, ui, rect);
        }
        let canvas =
            egui::Rect::from_min_max(egui::pos2(sidebar.map_or(full.left(), |r| r.right()), full.top()), full.max);
        ui.painter().rect_filled(canvas, 0.0, self.theme.color.surface_canvas);
        screens::chat::titlebar(self, ui, canvas, sidebar.is_some());
        screens::chat::show(self, ui, canvas);

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
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = frame;
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
