//! Pass 0 UI checks: the real Bukno app driven through egui_kittest, with
//! screenshots and accessibility results saved as evidence.
//!
//! These drive the whole app (sidebar, transcript, composer) through
//! pointer, keyboard and IME events, and read results back from the same
//! AccessKit tree a screen reader uses. They do not replace the packaged-app
//! checks for VoiceOver, real IME input and Finder launch.
//!
//! Ways the transcript and shell could fail, enumerated before writing the
//! checks; each check names the failures it covers:
//!
//! F1 Selection is lost or moves when its start block scrolls out of view.
//! F2 Selection breaks while the streaming block re-parses.
//! F3 Corrected height estimates make visible text jump.
//! F4 Copy uses the wrong block separators, drops code, or stops at the
//!    viewport edge because offscreen blocks were never laid out.
//! F5 Dragging past the edge does not scroll or does not extend the selection.
//! F6 The keyboard caret cannot cross blocks or messages, or Shift loses the anchor.
//! F7 New output steals the scroll position while reading older messages.
//! F8 The accessibility tree has no message boundaries, no selection, or a
//!    selection that points at nodes that do not exist.
//! F9 Enter sends during IME composition; Swedish text is altered.
//! F10 The app repaints when idle, the orb exceeds its frame cap, or
//!     reduced motion still animates.
//! F11 Narrow windows overlap the sidebar and the conversation.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use bukno_core::decision::{DecisionKind, DecisionState, DecisionView};
use bukno_core::event::{ChatActivity, ChatSummary, PersistRequest, ViewUpdate};
use bukno_core::ids::{DecisionId, ItemId, RunId, TaskId, WorkspaceId};
use bukno_core::message::{ItemKind, Provider, TranscriptItem};
use bukno_core::run::RunState;
use bukno_core::task::{TaskInfo, WorkspaceInfo, WorkspaceKind};
use bukno_desktop::app::{Backend, BuknoApp, Mode};
use bukno_desktop::components::composer::composer_id;
use bukno_desktop::theme::Theme;
use bukno_desktop::transcript::selection::{Selection, TextPos};
use bukno_desktop::transcript::transcript_id;
use bukno_platform::paths::AppPaths;
use bukno_runtime::synthetic::{self, GenBlock, Scenario};
use bukno_runtime::{SetupView, UiCommand, UiEvent};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, MouseWheelUnit, OutputCommand, PointerButton, Pos2, TouchPhase, Vec2, ViewportId};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, SnapshotOptions};
use serde_json::json;

const STREAM_ITEM: ItemId = ItemId(0x5b5e_0000_0000_0000_0002_0000_0000_0001);
/// One 60 Hz frame. egui subtracts the predicted frame time from repaint delays.
const STEP_DT: f32 = 1.0 / 60.0;
const STREAM_RUN: RunId = RunId(0x5b5e_0000_0000_0000_0003_0000_0000_0001);

struct Check {
    harness: Harness<'static, BuknoApp>,
    events: Sender<UiEvent>,
    commands: Receiver<UiCommand>,
    stream_words: usize,
    dir: PathBuf,
    log: Vec<serde_json::Value>,
}

fn evidence_root() -> PathBuf {
    std::env::var_os("BUKNO_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ui-evidence"))
}

/// SQLite and other runtime files stay on the internal build drive. Only
/// reports and rendered evidence may go to the shared artifact directory.
fn runtime_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("bukno-ui-{}", std::process::id())).join(name)
}

impl Check {
    fn new(name: &str, scenario: &str, size: [f32; 2], reduce_motion: bool) -> Self {
        let scenario = Scenario::named(scenario).expect("scenario");
        let (events, rx) = mpsc::channel();
        let (cmd_tx, commands) = mpsc::channel();
        let dir = evidence_root().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let state = runtime_root(name).join("state");
        let work = runtime_root(name).join("work");
        let paths = AppPaths { state_dir: state, work_dir: Some(work), overridden: true };
        let history = scenario.history();
        let harness = Harness::builder()
            .with_size(Vec2::from(size))
            .with_pixels_per_point(1.0)
            .with_max_steps(10_000)
            .with_step_dt(STEP_DT)
            .wgpu()
            .build_eframe(move |cc| {
                let mut app =
                    BuknoApp::new(&cc.egui_ctx, Theme::load(), scenario, paths, Backend::Recorder(cmd_tx), rx);
                app.reduce_motion = reduce_motion;
                app
            });
        let check = Self { harness, events, commands, stream_words: 0, dir, log: Vec::new() };
        if !history.is_empty() {
            check.send(ViewUpdate::ConversationLoaded { task: synthetic::TASK, items: history });
        }
        check
    }

    /// The app in real mode, with events standing in for the coordinator.
    fn real(name: &str, size: [f32; 2]) -> Self {
        let (events, rx) = mpsc::channel();
        let (cmd_tx, commands) = mpsc::channel();
        let dir = evidence_root().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let work = runtime_root(name).join("work");
        std::fs::create_dir_all(&work).unwrap();
        let paths =
            AppPaths { state_dir: runtime_root(name).join("state"), work_dir: Some(work.clone()), overridden: true };
        let harness = Harness::builder()
            .with_size(Vec2::from(size))
            .with_pixels_per_point(1.0)
            .with_max_steps(10_000)
            .with_step_dt(STEP_DT)
            .wgpu()
            .build_eframe(move |cc| {
                let scenario = Scenario::named("empty").expect("scenario");
                let mut app = BuknoApp::with_mode(
                    &cc.egui_ctx,
                    Theme::load(),
                    Mode::Real,
                    scenario,
                    paths,
                    Backend::Recorder(cmd_tx),
                    rx,
                );
                app.reduce_motion = true;
                app
            });
        let check = Self { harness, events, commands, stream_words: 0, dir, log: Vec::new() };
        check
            .events
            .send(UiEvent::Setup(SetupView {
                work_folder: Some(work.display().to_string()),
                work_folder_missing: false,
                state_dir: "fixture".into(),
                overridden: true,
                preset: "codex.workspace-write.untrusted".into(),
            }))
            .unwrap();
        check
    }

    /// Save a screenshot as evidence only, for checks without a committed snapshot.
    fn picture(&mut self, name: &str) {
        let image = self.harness.render().expect("render");
        image.save(self.dir.join(format!("{name}.png"))).unwrap();
    }

    fn send(&self, update: ViewUpdate) {
        self.events.send(UiEvent::View(update)).unwrap();
    }

    fn app(&mut self) -> &mut BuknoApp {
        self.harness.state_mut()
    }

    fn step(&mut self, n: usize) {
        for _ in 0..n {
            self.harness.step();
        }
    }

    /// Start a run and stream words into a reply at the end of the chat.
    fn start_stream(&mut self) {
        self.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
        self.stream(1);
    }

    /// Add `n` streamed words (one batch per frame), like the runtime does.
    fn stream(&mut self, frames: usize) {
        for _ in 0..frames {
            self.stream_words += 3;
            let text = (0..self.stream_words)
                .map(|i| if i % 50 == 49 { "räksmörgås\n\n" } else { "strömmande " })
                .collect::<String>();
            self.send(ViewUpdate::ItemUpserted(TranscriptItem {
                id: STREAM_ITEM,
                task: synthetic::TASK,
                run: Some(STREAM_RUN),
                kind: ItemKind::AgentMessage { provider: Provider::Codex },
                text,
                meta: Some("Synthetic model · Medium".into()),
                completed: false,
                revision: self.stream_words as u64,
            }));
            self.harness.step();
        }
    }

    /// What the coordinator publishes when it accepts a submitted message.
    fn accept_user_message(&mut self, body: &str) {
        self.send(ViewUpdate::ItemUpserted(TranscriptItem {
            id: ItemId(0x5b5e_0000_0000_0000_0004_0000_0000_0000 + self.stream_words as u128 + body.len() as u128),
            task: synthetic::TASK,
            run: None,
            kind: ItemKind::UserMessage,
            text: body.trim().to_owned(),
            meta: None,
            completed: true,
            revision: 1,
        }));
        self.step(2);
    }

    fn type_in_composer(&mut self, text: &str) {
        self.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
        self.step(1);
        self.harness.event(Event::Text(text.into()));
        self.step(1);
    }

    fn wheel(&mut self, dy: f32) {
        let center = self.app().transcript.viewport().center();
        self.harness.event(Event::PointerMoved(center));
        self.harness.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(0.0, dy),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
    }

    fn key(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.harness.step();
    }

    fn pos_of(&mut self, message: usize, offset: usize) -> TextPos {
        let app = self.harness.state();
        let block = app.doc.messages[message - 1].first_block;
        TextPos { block: app.doc.blocks[block].id, offset }
    }

    fn screen(&mut self, pos: TextPos) -> Option<Pos2> {
        let app = self.harness.state_mut();
        let theme = app.theme.clone();
        let BuknoApp { transcript, doc, .. } = app;
        transcript.screen_pos(doc, &theme, pos)
    }

    fn visible(&mut self, pos: TextPos) -> Option<Pos2> {
        let viewport = self.app().transcript.viewport().shrink(30.0);
        self.screen(pos).filter(|p| viewport.contains(*p))
    }

    fn selection(&self) -> Option<Selection> {
        self.harness.state().transcript.selection
    }

    /// Press Cmd+C and return what that frame put on the clipboard.
    fn copy(&mut self) -> Option<String> {
        self.harness.event(Event::ModifiersChanged(Modifiers::COMMAND));
        self.harness.step();
        self.harness.event(Event::Key {
            key: Key::C,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        self.harness.step();
        let copied = self.harness.output().platform_output.commands.iter().find_map(|c| match c {
            OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        });
        self.harness.event(Event::Key {
            key: Key::C,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        self.harness.event(Event::ModifiersChanged(Modifiers::NONE));
        self.harness.step();
        copied
    }

    fn shot(&mut self, name: &str) {
        let image = self.harness.render().expect("render");
        image.save(self.dir.join(format!("{name}.png"))).unwrap();
        let mut references = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        // Linux uses Ctrl labels and Vulkan rather than the Mac/Metal reference.
        // Keep its reviewed images separate, without replacing Mac snapshots.
        if cfg!(target_os = "linux") {
            references = references.join("linux");
        }
        let options = SnapshotOptions::new().output_path(references);
        if let Err(err) = self.harness.try_snapshot_options(name, &options) {
            // Snapshot drift is reported in the evidence, not hidden.
            self.log.push(json!({ "snapshot": name, "status": "differs", "detail": err.to_string() }));
        } else {
            self.log.push(json!({ "snapshot": name, "status": "matches" }));
        }
    }

    fn record(&mut self, step: &str, value: serde_json::Value) {
        self.log.push(json!({ "step": step, "observed": value }));
    }

    /// Write the evidence, then fail if any screenshot drifted from its
    /// committed snapshot (accept intended changes with UPDATE_SNAPSHOTS=1).
    fn finish(self, result: serde_json::Value) {
        let drift: Vec<_> = self.log.iter().filter(|l| l["status"] == "differs").cloned().collect();
        let report = json!({ "result": result, "snapshot_drift": drift, "log": self.log });
        std::fs::write(self.dir.join("result.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
        assert!(drift.is_empty(), "screenshots differ from tests/snapshots: {drift:#?}");
    }
}

/// Plain text the transcript should copy between two positions, computed
/// from the scenario's structure, not from the widget's document.
fn expected_text(from: (usize, usize), to: (usize, usize)) -> String {
    // (message, is list item, plain text) for the first block of `from` onwards.
    let mut blocks: Vec<(usize, bool, String)> = Vec::new();
    for n in from.0..=to.0 {
        for block in synthetic::blocks(n) {
            match &block {
                GenBlock::List(items) => blocks.extend(items.iter().map(|i| (n, true, i.clone()))),
                other => blocks.push((n, false, synthetic::plain_blocks(std::slice::from_ref(other)).remove(0))),
            }
        }
    }
    let last = blocks.iter().position(|b| b.0 == to.0).unwrap();
    let mut out = String::new();
    for (i, (n, list, text)) in blocks.iter().enumerate().take(last + 1) {
        if i > 0 {
            let prev = &blocks[i - 1];
            out.push_str(if prev.0 == *n && prev.1 && *list { "\n" } else { "\n\n" });
        }
        let chars: Vec<char> = text.chars().collect();
        let start = if i == 0 { from.1 } else { 0 };
        let end = if i == last { to.1 } else { chars.len() };
        out.extend(&chars[start..end]);
    }
    out
}

fn first_block_chars(n: usize) -> usize {
    let blocks = synthetic::blocks(n);
    let first = match &blocks[0] {
        GenBlock::List(items) => items[0].clone(),
        other => synthetic::plain_blocks(std::slice::from_ref(other)).remove(0),
    };
    first.chars().count()
}

/// Covers F1, F2, F3, F4, F5, F7 and part of F8, with the mouse.
#[test]
fn transcript_mouse_selection_across_messages_while_streaming() {
    let mut c = Check::new("transcript-mouse", "long-chat", [1440.0, 900.0], true);
    c.step(3);
    c.start_stream();
    assert!(c.app().transcript.is_following(), "a new chat opens at its end");

    // Scroll to the top with the wheel, as a person would.
    for _ in 0..4 {
        c.wheel(1.0e7);
        c.stream(2);
    }
    let mid3 = first_block_chars(3) / 2;
    let mid40 = first_block_chars(40) / 2;
    let start = c.pos_of(3, mid3);
    let p3 = c.visible(start).expect("message 3 is in view at the top");
    c.shot("mouse-01-top");

    // Press in the middle of message 3 and drag below the transcript edge.
    c.harness.event(Event::PointerMoved(p3));
    c.harness.event(Event::PointerButton {
        pos: p3,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    c.stream(1);
    let below = Pos2::new(p3.x, c.app().transcript.viewport().bottom() + 60.0);
    let end = c.pos_of(40, mid40);
    let mut frames = 0;
    let target = loop {
        c.harness.event(Event::PointerMoved(below));
        c.stream(1);
        frames += 1;
        if let Some(p) = c.visible(end) {
            break p;
        }
        assert!(frames < 2_000, "dragging past the edge never reached message 40");
    };
    c.record("autoscroll", json!({ "frames_dragging_at_edge": frames }));
    // Point at the middle of message 40 (twice: the first move stops the
    // autoscroll), then release.
    c.harness.event(Event::PointerMoved(target));
    c.stream(1);
    let target = c.visible(end).expect("message 40 still in view");
    c.harness.event(Event::PointerMoved(target));
    c.stream(1);
    c.harness.event(Event::PointerButton {
        pos: target,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    c.stream(2);

    let selection = c.selection().expect("a selection exists");
    assert_eq!(selection.anchor, start, "anchor stays in the middle of message 3");
    assert_eq!(selection.focus, end, "focus lands in the middle of message 40");
    c.shot("mouse-02-selected-at-40");

    // Scroll far away (to the streaming end) and back.
    c.wheel(-1.0e7);
    c.stream(10);
    assert_eq!(c.selection(), Some(selection), "selection survives scrolling to the end");
    c.shot("mouse-03-scrolled-away-streaming");
    for _ in 0..3 {
        c.wheel(1.0e7);
        c.stream(2);
    }
    assert_eq!(c.selection(), Some(selection), "selection survives scrolling back");
    c.shot("mouse-04-back-at-3");

    // What a screen reader sees: the selection on the Document node.
    let a11y = accessibility_selection(&c.harness);
    c.record("accessibility_after_mouse_selection", a11y.clone());

    // Copy, then paste into the composer.
    let copied = c.copy().expect("Cmd+C copies");
    let expected = expected_text((3, mid3), (40, mid40));
    assert_eq!(copied, expected, "copied text is exactly the selected plain text");
    std::fs::write(c.dir.join("copied.txt"), &copied).unwrap();
    std::fs::write(c.dir.join("expected.txt"), &expected).unwrap();

    let composer = c.harness.get_by_role(Role::MultilineTextInput).rect().center();
    c.harness.event(Event::PointerButton {
        pos: composer,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    c.harness.event(Event::PointerButton {
        pos: composer,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    c.stream(2);
    c.harness.event(Event::Paste(copied.clone()));
    c.stream(2);
    assert_eq!(c.app().composer.text, expected, "pasted text matches exactly");
    let selected_text = a11y["selected_text"].as_str().unwrap_or_default().to_owned();
    assert!(
        !selected_text.is_empty() && expected.starts_with(&selected_text),
        "the accessible selection is the visible part of the copied text: {a11y:#}"
    );

    let copied_chars = copied.chars().count();
    let streamed_words = c.stream_words;
    c.finish(json!({
        "status": "pass",
        "anchor": "message 3, character ".to_owned() + &mid3.to_string(),
        "focus": "message 40, character ".to_owned() + &mid40.to_string(),
        "copied_characters": copied_chars,
        "copied_equals_expected": true,
        "pasted_equals_expected": true,
        "streamed_words_during_check": streamed_words,
    }));
}

/// The Document node's selection as AccessKit reports it to VoiceOver.
fn accessibility_selection(harness: &Harness<'_, BuknoApp>) -> serde_json::Value {
    let doc = harness.get_by(|n| n.role() == Role::Document);
    let node = doc.accesskit_node();
    let articles: Vec<String> = doc
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == Role::Article)
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    let selected = node.text_selection().map(|range| range.text());
    json!({
        "document_label": node.label(),
        "document_description": node.description(),
        "articles_in_view": articles.len(),
        "first_articles": articles.iter().take(4).collect::<Vec<_>>(),
        "has_text_selection": node.has_text_selection(),
        "selected_text_chars": selected.as_ref().map(|s| s.chars().count()),
        "selected_text": selected,
    })
}

/// Covers F1, F4 and F6 with the keyboard only.
#[test]
fn transcript_keyboard_selection_across_messages() {
    let mut c = Check::new("transcript-keyboard", "long-chat", [1440.0, 900.0], true);
    c.step(3);
    c.start_stream();

    // Tab into the transcript.
    let mut tabs = 0;
    while !c.harness.ctx.memory(|m| m.has_focus(transcript_id())) {
        c.harness.key_press(Key::Tab);
        c.stream(1);
        tabs += 1;
        assert!(tabs < 40, "Tab never reached the transcript");
    }
    c.record("tabs_to_transcript", json!(tabs));

    // To the start, then block by block into message 3.
    c.key(Modifiers::COMMAND, Key::ArrowUp);
    let msg3_block = c.app().doc.messages[2].first_block;
    let mut presses = 0;
    loop {
        let app = c.harness.state();
        let focus = app.transcript.selection.unwrap().focus;
        if app.doc.index_of(focus.block) == Some(msg3_block) {
            break;
        }
        c.key(Modifiers::ALT, Key::ArrowDown);
        c.stream(1);
        presses += 1;
        assert!(presses < 50);
    }
    c.key(Modifiers::ALT, Key::ArrowUp);
    let half3 = first_block_chars(3) / 2;
    while c.selection().unwrap().focus.offset < half3 {
        c.key(Modifiers::ALT, Key::ArrowRight);
    }
    let anchor = c.selection().unwrap().focus;
    c.shot("keyboard-01-caret-in-3");

    // Extend with Shift into message 40.
    let msg40_block = c.app().doc.messages[39].first_block;
    let shift_alt = Modifiers::SHIFT | Modifiers::ALT;
    let mut presses = 0;
    loop {
        let app = c.harness.state();
        if app.doc.index_of(app.transcript.selection.unwrap().focus.block) == Some(msg40_block) {
            break;
        }
        c.key(shift_alt, Key::ArrowDown);
        c.stream(1);
        presses += 1;
        assert!(presses < 400, "Shift+Option+Down never reached message 40");
    }
    c.key(shift_alt, Key::ArrowUp);
    let half40 = first_block_chars(40) / 2;
    while c.selection().unwrap().focus.offset < half40 {
        c.key(shift_alt, Key::ArrowRight);
    }
    let selection = c.selection().unwrap();
    assert_eq!(selection.anchor, anchor, "Shift keeps the anchor in message 3");
    c.shot("keyboard-02-extended-to-40");

    // Page away and back with the keyboard; the selection stays.
    for _ in 0..30 {
        c.key(Modifiers::NONE, Key::PageDown);
        c.stream(1);
    }
    assert_eq!(c.selection(), Some(selection));
    for _ in 0..30 {
        c.key(Modifiers::NONE, Key::PageUp);
        c.stream(1);
    }
    assert_eq!(c.selection(), Some(selection));

    let copied = c.copy().expect("Cmd+C copies");
    let expected = expected_text((3, anchor.offset), (40, selection.focus.offset));
    assert_eq!(copied, expected);
    std::fs::write(c.dir.join("copied.txt"), &copied).unwrap();

    // Tab to the composer and paste.
    let mut tabs = 0;
    while !c.harness.ctx.memory(|m| m.has_focus(composer_id())) {
        c.harness.key_press(Key::Tab);
        c.stream(1);
        tabs += 1;
        assert!(tabs < 40, "Tab never reached the composer");
    }
    c.harness.event(Event::Paste(copied.clone()));
    c.stream(2);
    assert_eq!(c.app().composer.text, expected);
    c.shot("keyboard-03-pasted");
    c.finish(json!({
        "status": "pass",
        "anchor_offset_in_message_3": anchor.offset,
        "focus_offset_in_message_40": selection.focus.offset,
        "copied_characters": copied.chars().count(),
        "copied_equals_expected": true,
        "pasted_equals_expected": true,
    }));
}

/// Covers F8: message boundaries, roles and readable text.
#[test]
fn transcript_accessibility_tree() {
    let mut c = Check::new("transcript-accessibility", "long-chat", [1440.0, 900.0], true);
    c.step(4);
    let doc = c.harness.get_by(|n| n.role() == Role::Document);
    let mut lines = Vec::new();
    for node in doc.children_recursive() {
        let n = node.accesskit_node();
        let depth = std::iter::successors(n.parent(), |p| p.parent()).count();
        let text = n.value().or_else(|| n.label()).unwrap_or_default();
        if n.role() != Role::TextRun {
            lines.push(format!("{}{:?} {}", "  ".repeat(depth.saturating_sub(2)), n.role(), text));
        }
    }
    std::fs::write(c.dir.join("accessibility-tree.txt"), lines.join("\n")).unwrap();
    let articles: Vec<String> = doc
        .children()
        .filter(|n| n.accesskit_node().role() == Role::Article)
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    assert!(articles.len() >= 3, "messages in view are articles");
    assert!(
        articles.iter().any(|l| l.starts_with("You, message"))
            && articles.iter().any(|l| l.starts_with("Codex, message"))
    );
    assert!(articles.last().unwrap().ends_with("message 2000 of 2000"));
    // The document's text is readable through AccessKit text ranges.
    let readable = doc.accesskit_node().document_range().text();
    assert!(readable.contains("Meddelande 2000."));
    // A selection made with Cmd+A is reported, clamped to the exposed text.
    let center = c.app().transcript.viewport().center();
    c.harness.event(Event::PointerButton {
        pos: center,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    c.harness.event(Event::PointerButton {
        pos: center,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    c.step(2);
    c.key(Modifiers::COMMAND, Key::A);
    c.step(2);
    let selection = accessibility_selection(&c.harness);
    assert_eq!(selection["has_text_selection"], json!(true));
    c.record("select_all", selection);
    c.finish(json!({
        "status": "pass",
        "articles_in_view": articles.len(),
        "last_article": articles.last(),
        "readable_characters_in_view": readable.chars().count(),
        "note": "Blocks far outside the viewport are not exposed. VoiceOver itself is checked in the packaged app."
    }));
}

/// Covers F9: IME composition, Enter, Shift+Enter and Swedish text.
#[test]
fn composer_ime_and_swedish_text() {
    let mut c = Check::new("composer-ime", "empty", [1440.0, 900.0], true);
    c.step(2);
    c.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
    c.step(2);

    let swedish = "Hej! Räksmörgås på Åre, ÄÖ åäö ÅÄÖ.";
    c.harness.event(Event::Text(swedish.into()));
    c.step(2);
    assert_eq!(c.app().composer.text, swedish);

    // Compose ä with a dead key: ¨ is preedit, Enter arrives during composition.
    c.harness.event(Event::Ime(egui::ImeEvent::Preedit { text: "¨".into(), active_range_chars: None }));
    c.step(1);
    c.harness.key_press(Key::Enter);
    c.step(1);
    assert!(c.commands.try_recv().is_err(), "Enter during composition must not send");
    c.harness.event(Event::Ime(egui::ImeEvent::Commit("ä".into())));
    c.step(2);
    assert!(c.app().composer.text.ends_with('ä'), "committed text lands: {:?}", c.app().composer.text);

    // Shift+Enter inserts a newline and does not send.
    c.harness.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    c.step(2);
    assert!(c.app().composer.text.contains('\n'));
    assert!(c.commands.try_recv().is_err());
    c.harness.event(Event::Text("Andra raden".into()));
    c.step(2);
    c.shot("composer-01-swedish-multiline");

    // Enter sends the exact text.
    let text = c.app().composer.text.clone();
    c.harness.key_press(Key::Enter);
    c.step(2);
    match c.commands.try_recv() {
        Ok(UiCommand::Submit { body, .. }) => assert_eq!(body, text.trim()),
        other => panic!("expected a submit, got {other:?}"),
    }
    assert_eq!(c.app().composer.text, text, "the draft stays until the coordinator accepts it");
    c.accept_user_message(&text);
    assert!(c.app().composer.text.is_empty(), "accepted, so the sent revision is cleared");
    c.finish(json!({ "status": "pass", "sent": text }));
}

/// Covers keyboard reachability of the shell and visible focus.
#[test]
fn shell_keyboard_navigation() {
    let mut c = Check::new("shell-keyboard", "short", [1440.0, 900.0], true);
    c.step(3);
    let mut order = Vec::new();
    for _ in 0..16 {
        c.harness.key_press(Key::Tab);
        c.step(1);
        let label = c
            .harness
            .query_by(|n| n.is_focused())
            .map(|n| n.accesskit_node().label().unwrap_or_else(|| format!("{:?}", n.accesskit_node().role())));
        order.push(label.unwrap_or_else(|| "(unlabeled)".into()));
        if order.len() == 2 {
            c.shot("shell-01-focus-ring");
        }
    }
    assert!(order.iter().any(|l| l == "New chat"));
    assert!(
        !order.iter().any(|l| l == "Unknown" || l == "(unlabeled)" || l == "MultilineTextInput"),
        "every focus stop has a name: {order:?}"
    );
    assert!(order.iter().any(|l| l == "Follow up with Codex"), "the composer is named by its recipient");
    assert!(order.iter().any(|l| l.contains("Polish the composer")));
    assert!(order.iter().any(|l| l.contains("Conversation") || l == "Document"));
    // Cmd+N opens a new chat with the composer focused.
    c.key(Modifiers::COMMAND, Key::N);
    c.step(2);
    assert!(c.harness.ctx.memory(|m| m.has_focus(composer_id())));
    c.record("tab_order", json!(order));
    c.finish(json!({ "status": "pass", "tab_order": order }));
}

fn repaint_delay(harness: &Harness<'_, BuknoApp>) -> Duration {
    harness.output().viewport_output.get(&ViewportId::ROOT).map_or(Duration::MAX, |v| v.repaint_delay)
}

/// Covers F10: no repaint when idle, the working streak capped at 10 fps, reduced motion still.
#[test]
fn repaint_policy() {
    let mut idle = Check::new("repaint-policy", "short", [1440.0, 900.0], false);
    let steps = idle.harness.run();
    let idle_delay = repaint_delay(&idle.harness);
    assert_eq!(idle_delay, Duration::MAX, "an idle window requests no repaint");

    idle.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
    let mut delays = Vec::new();
    for _ in 0..12 {
        idle.step(1);
        delays.push(repaint_delay(&idle.harness).as_secs_f64() * 1000.0);
    }
    // The indicator adds back egui's predicted frame time, so the reported delay is the cadence.
    let orb_delay = repaint_delay(&idle.harness);
    idle.record("working_repaint_delays_ms", json!(delays));
    assert!(orb_delay >= Duration::from_millis(99), "the streak never asks for more than 10 fps: {orb_delay:?}");
    assert!(orb_delay <= Duration::from_millis(101), "the streak keeps its 10 fps cadence: {orb_delay:?}");
    idle.shot("repaint-01-orb");

    let mut reduced = Check::new("repaint-policy-reduced", "short", [1440.0, 900.0], true);
    reduced.step(2);
    reduced.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
    reduced.step(3);
    let reduced_delay = repaint_delay(&reduced.harness);
    assert!(reduced_delay >= Duration::from_millis(999), "reduced motion only ticks the clock: {reduced_delay:?}");
    reduced.finish(json!({ "status": "pass" }));

    idle.finish(json!({
        "status": "pass",
        "idle_steps_until_settled": steps,
        "idle_repaint_delay": "none requested",
        "working_repaint_delay_ms": orb_delay.as_secs_f64() * 1000.0,
        "reduced_motion_repaint_delay_ms": reduced_delay.as_secs_f64() * 1000.0,
    }));
}

/// Reference layouts at 1440 by 900 and 1024 by 720, plus F11 narrow widths.
#[test]
fn reference_screens() {
    let mut results = Vec::new();
    for (name, scenario, size, running) in [
        ("screen-1-2-new-chat-1440x900", "empty", [1440.0, 900.0], false),
        ("screen-2-1-working-1440x900", "short", [1440.0, 900.0], true),
        ("screen-long-chat-1440x900", "long-chat", [1440.0, 900.0], false),
        ("screen-5-1-working-1024x720", "short", [1024.0, 720.0], true),
        ("screen-narrow-760x720", "short", [760.0, 720.0], true),
    ] {
        let mut c = Check::new(name, scenario, size, true);
        c.step(2);
        if running {
            c.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
            c.step(2);
        }
        c.step(2);
        let sidebar = c.harness.query_by_label("New chat").is_some();
        c.shot(name);
        c.record("sidebar_visible", json!(sidebar));
        if size[0] < 824.0 {
            assert!(!sidebar, "the sidebar collapses before the conversation gets too narrow");
        } else {
            assert!(sidebar);
        }
        results.push(json!({ "screen": name, "sidebar_visible": sidebar }));
        c.finish(json!({ "status": "pass" }));
    }
    let _ = results;
}

/// Compares the approved orb with the two proposed working treatments:
/// frames at fixed moments, and how often each asks the window to redraw.
#[test]
fn working_treatments() {
    use bukno_desktop::components::orb::Mark;
    let mut summary = Vec::new();
    let mut strip: Vec<image::RgbaImage> = Vec::new();
    for (mark, name) in [(Mark::Orb, "orb"), (Mark::Grid, "grid"), (Mark::Streak, "streak")] {
        let mut c = Check::new(&format!("working-{name}"), "short", [1440.0, 900.0], false);
        c.app().working_mark = mark;
        c.step(2);
        c.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
        c.step(2);
        let mut delays = Vec::new();
        // Six moments 0.2 s apart (12 frames at 60 Hz).
        for moment in 0..6 {
            c.step(12);
            delays.push(repaint_delay(&c.harness).as_secs_f64() * 1000.0);
            let image = c.harness.render().expect("render");
            let crop = image::imageops::crop_imm(&image, 460, 660, 560, 80).to_image();
            crop.save(c.dir.join(format!("{name}-{moment}.png"))).unwrap();
            strip.push(crop);
        }
        c.shot(&format!("working-{name}"));
        c.record("repaint_delays_ms", json!(delays));
        summary.push(json!({ "mark": name, "repaint_delays_ms": delays }));
        c.finish(json!({ "status": "pass" }));
    }
    // One image: a row of six moments per treatment.
    let (w, h) = (560, 80);
    let mut out = image::RgbaImage::new(w * 6, h * 3);
    for (i, tile) in strip.iter().enumerate() {
        image::imageops::overlay(&mut out, tile, ((i % 6) as u32 * w).into(), ((i / 6) as u32 * h).into());
    }
    let dir = evidence_root().join("working-treatments");
    std::fs::create_dir_all(&dir).unwrap();
    out.save(dir.join("orb-grid-streak-moments.png")).unwrap();
    std::fs::write(
        dir.join("result.json"),
        serde_json::to_string_pretty(&json!({ "status": "pass", "treatments": summary })).unwrap(),
    )
    .unwrap();
}

/// Records the native StreakLabel at several frame rates as animated GIFs
/// with the real frame timing, so smoothness can be judged by eye.
#[test]
fn streak_frame_rates() {
    use bukno_desktop::components::orb::{self, Mark, OrbState, Working};
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame};

    let dir = evidence_root().join("streak-frame-rates");
    std::fs::create_dir_all(&dir).unwrap();
    let theme = Theme::load();
    let mut results = Vec::new();
    for fps in [10.0_f32, 12.0, 15.0, 60.0] {
        let dt = 1.0 / fps;
        let t = theme.clone();
        let mut installed = false;
        let mut harness = Harness::builder()
            .with_size(Vec2::new(520.0, 64.0))
            .with_pixels_per_point(2.0)
            .with_step_dt(dt)
            .wgpu()
            .build_ui(move |ui| {
                if !installed {
                    // Fonts take effect from the next frame.
                    t.install(ui.ctx());
                    installed = true;
                    return;
                }
                ui.painter().rect_filled(ui.max_rect(), 0.0, t.color.surface_canvas);
                let rect = egui::Rect::from_min_size(ui.max_rect().min + Vec2::new(16.0, 12.0), Vec2::new(480.0, 40.0));
                orb::working_indicator(
                    ui,
                    &t,
                    rect,
                    &Working {
                        accent: t.color.codex,
                        label: "Reading the composer module",
                        elapsed: Some(40.0),
                        summary: None,
                        state: OrbState::Thinking,
                        reduce_motion: false,
                        mark: Mark::Streak,
                    },
                );
            });
        let file = std::fs::File::create(dir.join(format!("streak-{fps:.0}fps.gif"))).unwrap();
        let mut gif = GifEncoder::new_with_speed(file, 10);
        gif.set_repeat(Repeat::Infinite).unwrap();
        // Two full passes of the light (loop-streak is 2.2 s).
        let frames = (4.4 * fps).round() as usize;
        harness.step();
        for _ in 0..frames {
            harness.step();
            let image = harness.render().expect("render");
            gif.encode_frame(Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(1000, fps as u32))).unwrap();
        }
        results.push(json!({ "fps": fps, "frames": frames }));
    }
    std::fs::write(
        dir.join("result.json"),
        serde_json::to_string_pretty(&json!({ "status": "pass", "gifs": results })).unwrap(),
    )
    .unwrap();
}

// Checks added for the PR 1 review (Codex). Each names its finding.
//
// R1 (P1) Pressing Enter while a run is active cleared the draft, although
//    the coordinator rejected the message as RunActive.
// R3 (P2) An IME cancellation sent after the composer lost focus left it
//    "composing", so Enter inserted newlines instead of sending.
// R4 (P2) Finishing streamed Markdown (`**bold` to bold) kept the old
//    character offsets, so a selected "bold" became "ld".
// R5 (P2) At 480 points high the sidebar rows ran under the pinned footer.
// R6 (P2) A long new-chat draft pushed Send below a short window.
//
// Second review pass:
// R7 (P2) Inline code that joins lines (`first\nsecond`) mapped every
//    character to one source position, so a selected "second" widened to
//    "first second" on the next append.
// R8 (P2) Finishing a table split one paragraph into several blocks, and a
//    selection on the header became empty.
// R9 (P2) Completed replies kept up to 32 source maps per block, so
//    repeated replies grew retained memory (19.57 MiB for 40 replies).

fn submits(c: &Check) -> Vec<String> {
    c.commands
        .try_iter()
        .filter_map(|cmd| match cmd {
            UiCommand::Submit { body, .. } => Some(body),
            _ => None,
        })
        .collect()
}

/// R1: the draft survives sending during a run, a rejection, and typing
/// while a send is pending.
#[test]
fn review_draft_survives_send_during_run() {
    let mut c = Check::new("review-draft", "short", [1440.0, 900.0], true);
    c.step(3);
    c.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
    c.step(2);
    c.type_in_composer("Also check the narrow window");
    c.harness.key_press(Key::Enter);
    c.step(2);
    assert!(submits(&c).is_empty(), "nothing is sent while the run is active");
    assert_eq!(c.app().composer.text, "Also check the narrow window", "the draft is kept, without a newline");
    assert!(c.app().send_blocked().is_some());
    c.shot("review-draft-01-blocked-during-run");

    // The run ends; Send works, and the draft stays until accepted.
    c.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Completed });
    c.step(2);
    c.harness.key_press(Key::Enter);
    c.step(2);
    assert_eq!(submits(&c), vec!["Also check the narrow window".to_owned()]);
    assert_eq!(c.app().composer.text, "Also check the narrow window");

    // A rejection keeps the draft and explains why.
    c.events
        .send(UiEvent::Rejected { task: synthetic::TASK, reason: bukno_core::event::RejectReason::RunActive })
        .unwrap();
    c.step(2);
    assert_eq!(c.app().composer.text, "Also check the narrow window", "a rejected message keeps its draft");
    assert!(c.app().notice.as_deref().is_some_and(|n| n.contains("draft is kept")));

    // Typing while a send is pending is not cleared by the acceptance.
    c.harness.key_press(Key::Enter);
    c.step(2);
    assert_eq!(submits(&c).len(), 1);
    c.harness.event(Event::Text(" and the sidebar".into()));
    c.step(2);
    c.accept_user_message("Also check the narrow window");
    assert_eq!(c.app().composer.text, "Also check the narrow window and the sidebar", "a newer revision is kept");
    let kept = c.app().composer.text.clone();
    c.finish(json!({ "status": "pass", "draft_after_newer_typing": kept }));
}

/// R3: losing focus ends composition, so a later cancellation cannot block Enter.
#[test]
fn review_ime_cancel_after_focus_loss() {
    let mut c = Check::new("review-ime-cancel", "empty", [1440.0, 900.0], true);
    c.step(2);
    c.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
    c.step(1);
    c.harness.event(Event::Ime(egui::ImeEvent::Preedit { text: "に".into(), active_range_chars: None }));
    c.step(1);
    assert!(c.app().composer.composing);
    // Focus moves to the sidebar, then the input method cancels.
    c.harness.ctx.memory_mut(|m| m.request_focus(egui::Id::new("nav-new-chat")));
    c.step(2);
    c.harness.event(Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
    c.step(1);
    c.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
    c.step(1);
    assert!(!c.app().composer.composing, "composition ended with the focus loss");
    c.harness.event(Event::Text("Ready to send".into()));
    c.step(1);
    c.harness.key_press(Key::Enter);
    c.step(2);
    let sent = submits(&c);
    assert_eq!(sent.len(), 1, "Enter sends again");
    // Like a native macOS field, text composed before focus left stays in
    // the draft as typed text. It is visible before sending; nothing hidden
    // or half-composed is sent.
    assert_eq!(sent[0], "にReady to send", "exactly the visible draft is sent, without a newline");
    c.finish(json!({ "status": "pass", "sent": sent[0] }));
}

/// R4, R7, R8: selections keep their text while streamed Markdown
/// finishes: emphasis, inline code, a link, inline code across a line
/// break, and a paragraph that becomes a table.
#[test]
fn review_selection_survives_markdown_completion() {
    let mut c = Check::new("review-markdown-selection", "short", [1440.0, 900.0], true);
    c.step(2);
    c.send(ViewUpdate::RunStateChanged { task: synthetic::TASK, run: STREAM_RUN, state: RunState::Running });
    // Each case uses its own reply: the unfinished source, the word to
    // select in its rendered text, and the source after the next append.
    let cases = [
        ("prefix **bold", "bold", "prefix **bold** after"),
        ("use `cod", "cod", "use `code` now"),
        ("see [docs](htt", "docs", "see [docs](https://example.org) then"),
        ("use `first\nsecond`", "second", "use `first\nsecond` more"),
        ("intro\n| Key | Value |\n| --", "Value", "intro\n| Key | Value |\n| -- | -- |\n| a | b |"),
    ];
    let mut observed = Vec::new();
    for (n, (unfinished, word, finished)) in cases.into_iter().enumerate() {
        let reply = |c: &Check, text: &str, revision: u64| {
            c.send(ViewUpdate::ItemUpserted(TranscriptItem {
                id: ItemId(STREAM_ITEM.0 + n as u128),
                task: synthetic::TASK,
                run: Some(STREAM_RUN),
                kind: ItemKind::AgentMessage { provider: Provider::Codex },
                text: text.into(),
                meta: None,
                completed: false,
                revision,
            }));
        };
        reply(&c, unfinished, 1);
        c.step(3);
        let (block, from) = {
            let doc = &c.harness.state().doc;
            let block = doc.blocks.iter().rev().find(|b| b.text.contains(word)).unwrap();
            (block.id, block.text[..block.text.find(word).unwrap()].chars().count())
        };
        let to = from + word.chars().count();
        // Select the word with the mouse, as a person would.
        let start = c.screen(TextPos { block, offset: from }).unwrap();
        let end = c.screen(TextPos { block, offset: to }).unwrap();
        c.harness.event(Event::PointerMoved(start));
        c.harness.event(Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        c.step(1);
        c.harness.event(Event::PointerMoved(end));
        c.step(1);
        c.harness.event(Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        c.step(1);
        let before = c.selection().unwrap().text(&c.harness.state().doc);
        assert_eq!(before, word, "selected the unfinished word");
        reply(&c, finished, 2);
        c.step(3);
        let after = c.selection().unwrap().text(&c.harness.state().doc);
        let blocks: Vec<String> = {
            let doc = &c.harness.state().doc;
            let message = &doc.messages[doc.messages.len() - 1];
            doc.blocks[message.first_block..message.first_block + message.block_count]
                .iter()
                .map(|b| b.text.clone())
                .collect()
        };
        let copied = c.copy();
        assert_eq!(after, word, "the selection keeps its text when {unfinished:?} finishes, blocks {blocks:?}");
        assert_eq!(copied.as_deref(), Some(word));
        observed.push(json!({
            "unfinished": unfinished,
            "finished": finished,
            "selected_before": before,
            "selected_after": after,
            "copied": copied,
            "blocks_after": blocks,
        }));
    }
    c.shot("review-markdown-selection");
    c.finish(json!({ "status": "pass", "cases": observed }));
}

/// R9: replies streamed in many revisions release their source maps once
/// they complete and are drawn, so repeated replies do not grow memory. A
/// selection made during streaming still lands on the final text.
#[test]
fn review_repeated_replies_release_source_maps() {
    const REPLIES: u128 = 40;
    const REVISIONS: u64 = 64;
    let mut c = Check::new("review-repeated-replies", "empty", [1440.0, 900.0], true);
    c.app().view = bukno_desktop::app::View::Chat;
    c.step(2);
    let chunk = "åäö ordinary text for a streamed reply. ".repeat(2);
    let mut peak = 0;
    let mut after_each = Vec::new();
    for n in 0..REPLIES {
        let id = ItemId(STREAM_ITEM.0 + 100 + n);
        let mut text = String::new();
        for revision in 1..=REVISIONS + 1 {
            let completed = revision > REVISIONS;
            if completed {
                text.push_str("Done **here**.");
            } else {
                text.push_str(&chunk);
            }
            c.send(ViewUpdate::ItemUpserted(TranscriptItem {
                id,
                task: synthetic::TASK,
                run: Some(STREAM_RUN),
                kind: ItemKind::AgentMessage { provider: Provider::Codex },
                text: text.clone(),
                meta: None,
                completed,
                revision,
            }));
            c.step(1);
            peak = peak.max(c.harness.state().doc.source_map_bytes());
            if n == REPLIES - 1 && revision == REVISIONS {
                // Select the last "reply" while it still streams; the text ends "reply.".
                let block = c.harness.state().doc.blocks.last().unwrap();
                let (id, end) = (block.id, block.chars);
                c.app().transcript.selection = Some(Selection {
                    anchor: TextPos { block: id, offset: end - 6 },
                    focus: TextPos { block: id, offset: end - 1 },
                });
            }
        }
        c.step(1);
        let doc = &c.harness.state().doc;
        after_each.push(
            json!({ "reply": n + 1, "source_map_bytes": doc.source_map_bytes(), "tracked": doc.tracked_messages() }),
        );
    }
    let doc = &c.harness.state().doc;
    let retained = doc.source_map_bytes();
    let tracked = doc.tracked_messages();
    let chars: usize = doc.blocks.iter().map(|b| b.chars).sum();
    let selected = c.selection().unwrap().text(doc);
    assert_eq!(retained, 0, "completed replies keep no source maps");
    assert_eq!(tracked, 0);
    assert_eq!(selected, "reply", "a selection made while streaming keeps its text when the reply completes");
    c.finish(json!({
        "status": "pass",
        "workload": "40 replies, each streamed in 64 revisions with one frame per revision, then completed",
        "rendered_characters": chars,
        "peak_source_map_bytes_while_streaming": peak,
        "retained_source_map_bytes_after_completion": retained,
        "messages_still_tracking_maps": tracked,
        "selection_after_completion": selected,
        "per_reply": after_each,
    }));
}

/// R5 and R6: at the declared minimum height nothing overlaps or leaves the
/// window, and the sidebar list scrolls above its footer.
#[test]
fn review_short_window_layout() {
    let mut c = Check::new("review-short-window", "short", [1024.0, 480.0], true);
    c.step(3);
    let list = c.app().sidebar_list.expect("sidebar shown at 1024 wide");
    let profile = c.harness.get_by_label("Profile, Synthetic scenario").rect();
    assert!(list.bottom() <= profile.top() - 70.0, "the list ends above the usage meters: {list:?} vs {profile:?}");
    c.shot("review-short-window-01-chat");
    // Keyboard: Tab to a row hidden below the visible list; it scrolls into view.
    let target = "A quick idea, Claude chat";
    let before = c.harness.get_by_label(target).rect();
    assert!(before.top() >= list.bottom(), "the row starts hidden below the list: {before:?}");
    let mut tabs = 0;
    while !c.harness.get_by_label(target).accesskit_node().is_focused() {
        c.harness.key_press(Key::Tab);
        c.step(3);
        tabs += 1;
        assert!(tabs < 20, "Tab never reached {target}");
    }
    let focused = c.harness.get_by_label(target).rect();
    assert!(
        focused.top() >= list.top() - 0.5 && focused.bottom() <= list.bottom() + 0.5,
        "Tab scrolls the row into the list: {focused:?} in {list:?}"
    );
    c.record(
        "tab_scrolls_row_into_view",
        json!({ "before": format!("{before:?}"), "focused": format!("{focused:?}") }),
    );
    // Scroll the list to its end: the last chat sits inside the list, above the footer.
    c.harness.event(Event::PointerMoved(list.center()));
    c.harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, -400.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    c.step(30);
    let last = c.harness.get_by_label("Packing list for Lisbon, Codex chat").rect();
    assert!(
        last.bottom() <= list.bottom() + 0.5 && last.top() >= list.top() - 0.5,
        "the last row scrolls fully into the list: {last:?} in {list:?}"
    );
    c.shot("review-short-window-02-sidebar-scrolled");

    // A 15-line draft in a new chat keeps Send and the toolbar inside the window.
    c.key(Modifiers::COMMAND, Key::N);
    c.step(2);
    let draft: String = (1..=15).map(|i| format!("Line {i} of a long direction\n")).collect();
    c.harness.event(Event::Paste(draft));
    c.step(4);
    let send = c.harness.get_by_label("Send").rect();
    let model = c.harness.get_by(|n| n.label().or_else(|| n.value()).is_some_and(|l| l.starts_with("Model "))).rect();
    assert!(send.bottom() <= 480.0 && send.top() >= 0.0, "Send stays inside the window: {send:?}");
    assert!(model.bottom() <= 480.0, "the model control stays inside the window: {model:?}");
    c.shot("review-short-window-03-long-draft");
    c.record("send_rect", json!(format!("{send:?}")));
    c.finish(json!({ "status": "pass", "list": format!("{list:?}"), "send": format!("{send:?}") }));
}

// Checks added for the PR 2 review (Sol), numbered as in
// e2e/scenarios/pass1-codex-failure-paths.md. C43 to C45 are replays in
// crates/core/tests/replay.rs.
//
// C46 (P1) An empty saved draft lost its revision on relaunch, so later
//     saves were refused by storage while the app showed them as saved.
// C47 (P1) A new chat that opened after the user moved to another chat took
//     that chat's draft.
// C48 (P2) At the 640 x 480 minimum window an approval card was left out,
//     so Allow once and Deny could not be reached.
//
// Second review pass (C49 is a replay):
// C50 (P2) A message sent before the saved draft loaded stayed in the
//     composer after it was accepted, so it could be sent twice.
// C51 (P2) A question with choices and a text field was measured 40 points
//     short, so its last field and the actions ran under the composer.
// C52 (P2) Question cards taller than the window had no scrolling, so the
//     first questions could not be reached.

fn chat_summary(task: TaskId, title: &str) -> ChatSummary {
    ChatSummary {
        task,
        title: title.into(),
        provider: Provider::Codex,
        project: None,
        workspace: WorkspaceId(task.0 + 100),
        path: format!("/work/{title}"),
        available: true,
        activity: ChatActivity::Idle,
        shared_workspace: false,
    }
}

fn draft_saves(c: &Check) -> Vec<(u64, String)> {
    c.commands
        .try_iter()
        .filter_map(|cmd| match cmd {
            UiCommand::SaveDraft { revision, text, .. } => Some((revision, text)),
            _ => None,
        })
        .collect()
}

/// Run frames of a headless app driven by the real coordinator until `until`.
fn frames_until(app: &mut BuknoApp, ctx: &egui::Context, events: Vec<Event>, until: impl Fn(&BuknoApp) -> bool) {
    let start = std::time::Instant::now();
    let mut events = Some(events);
    loop {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
            time: Some(start.elapsed().as_secs_f64()),
            events: events.take().unwrap_or_default(),
            ..Default::default()
        };
        // No renderer here, so drop the texture updates it would consume.
        ctx.run_ui(raw, |ui| app.show(ui)).textures_delta.clear();
        if until(app) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "the app did not reach the expected state");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// C46: a cleared draft keeps its revision across relaunch, through the real
/// coordinator and SQLite, and typing before the saved draft loads is kept.
#[test]
fn review_empty_draft_keeps_its_revision() {
    let dir = evidence_root().join("review-draft-revision");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let runtime = runtime_root("review-draft-revision");
    let (state, work) = (runtime.join("state"), runtime.join("work"));
    std::fs::create_dir_all(&work).unwrap();
    let task = TaskId(0x5b5e_0000_0000_0000_0046_0000_0000_0001);
    let workspace = WorkspaceInfo {
        id: WorkspaceId(0x46),
        path: work.display().to_string(),
        key: vec!["review-draft-revision".into()],
        kind: WorkspaceKind::Chat,
        git_root: None,
        identity: None,
        available: true,
    };
    let info = TaskInfo {
        id: task,
        title: "Cleared draft".into(),
        provider: Provider::Codex,
        project: None,
        workspace: workspace.id,
        session: None,
        created: 1,
    };
    // The state a sent or cleared draft leaves: no text, revision 40.
    let mut store = bukno_storage::Store::open(&state).unwrap();
    for request in [
        PersistRequest::Workspace(workspace),
        PersistRequest::Chat(info),
        PersistRequest::Draft { task, revision: 40, text: String::new() },
    ] {
        bukno_storage::repository::apply(store.connection(), &request).unwrap();
    }

    let ctx = egui::Context::default();
    let paths = AppPaths { state_dir: state.clone(), work_dir: Some(work), overridden: true };
    let mut app = BuknoApp::real(&ctx, paths, store);
    frames_until(&mut app, &ctx, vec![], |a| !a.chats.is_empty());
    app.select_chat(task);
    frames_until(&mut app, &ctx, vec![], |a| a.composer.revision == 40);
    ctx.memory_mut(|m| m.request_focus(composer_id()));
    frames_until(&mut app, &ctx, vec![], |_| true);
    frames_until(&mut app, &ctx, vec![Event::Text("New draft after relaunch".into())], |_| true);
    let typed = app.composer.revision;
    assert!(typed > 40, "typing is numbered after the saved revision, got {typed}");
    app.flush_draft();
    frames_until(&mut app, &ctx, vec![], |a| a.extra.draft_saved == typed);
    drop(app);
    let mut store = bukno_storage::Store::open(&state).unwrap();
    let saved: (String, i64) =
        store.connection().query_row("SELECT text, revision FROM draft", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(
        saved,
        ("New draft after relaunch".to_owned(), typed as i64),
        "storage holds what the app says it saved"
    );
    drop(store);

    // Typing that starts before the saved draft arrives is kept and numbered after it.
    let mut c = Check::real("review-draft-revision-early", [1440.0, 900.0]);
    c.send(ViewUpdate::Chats { projects: vec![], chats: vec![chat_summary(task, "Cleared draft")] });
    c.step(2);
    c.app().select_chat(task);
    c.type_in_composer("Typed early");
    c.send(ViewUpdate::DraftLoaded { task, text: String::new(), revision: 40 });
    c.step(2);
    assert_eq!(c.app().composer.text, "Typed early");
    assert_eq!(c.app().composer.revision, 41);
    c.app().flush_draft();
    let early = draft_saves(&c);
    assert_eq!(early.last(), Some(&(41, "Typed early".to_owned())));

    // C50: sent before the saved draft arrives, the message still leaves the composer when accepted.
    let other = TaskId(0x5b5e_0000_0000_0000_0050_0000_0000_0001);
    c.send(ViewUpdate::Chats {
        projects: vec![],
        chats: vec![chat_summary(task, "Cleared draft"), chat_summary(other, "Sent early")],
    });
    c.app().select_chat(other);
    c.type_in_composer("Sent before the draft loaded");
    c.harness.key_press(Key::Enter);
    c.step(2);
    c.send(ViewUpdate::DraftLoaded { task: other, text: String::new(), revision: 40 });
    c.step(2);
    c.send(ViewUpdate::ItemUpserted(TranscriptItem {
        id: ItemId(0x50),
        task: other,
        run: None,
        kind: ItemKind::UserMessage,
        text: "Sent before the draft loaded".into(),
        meta: None,
        completed: true,
        revision: 1,
    }));
    c.step(2);
    assert_eq!(c.app().composer.text, "", "the sent message is not left behind to send again");
    assert!(c.app().composer.revision > 41, "the cleared composer is numbered after the saved draft");
    let result = json!({
        "status": "pass",
        "relaunch": { "loaded_revision": 40, "saved": saved.0, "saved_revision": saved.1 },
        "typed_before_load": { "saves": format!("{early:?}") },
        "sent_before_load": { "composer_after_acceptance": "" },
    });
    std::fs::write(dir.join("result.json"), serde_json::to_string_pretty(&result).unwrap()).unwrap();
    c.finish(result);
}

/// C47: when a new chat opens after the user moved on, each chat keeps its own draft.
#[test]
fn review_new_chat_opens_after_navigation() {
    let a = TaskId(0x5b5e_0000_0000_0000_0047_0000_0000_000a);
    let b = TaskId(0x5b5e_0000_0000_0000_0047_0000_0000_000b);
    let mut c = Check::real("review-new-chat-late-open", [1440.0, 900.0]);
    c.send(ViewUpdate::Chats { projects: vec![], chats: vec![chat_summary(a, "Chat A")] });
    c.step(2);
    c.app().select_chat(a);
    c.type_in_composer("Private draft from chat A");
    c.app().new_chat(None);
    c.step(2);
    c.type_in_composer("First message of chat B");
    c.harness.key_press(Key::Enter);
    c.step(2);
    let sent: Vec<String> = c
        .commands
        .try_iter()
        .filter_map(|cmd| match cmd {
            UiCommand::SubmitNew { body, .. } => Some(body),
            _ => None,
        })
        .collect();
    assert_eq!(sent, vec!["First message of chat B".to_owned()]);

    // The user moves to chat A before the coordinator says the new chat opened.
    c.app().select_chat(a);
    c.step(1);
    c.events.send(UiEvent::Opened { task: b }).unwrap();
    c.send(ViewUpdate::Chats {
        projects: vec![],
        chats: vec![chat_summary(a, "Chat A"), chat_summary(b, "First message of chat B")],
    });
    c.step(2);
    assert_eq!(c.app().selected, Some(a), "the user stays where they went");
    assert_eq!(c.app().composer.text, "Private draft from chat A", "chat A keeps its draft");
    let moved = c.app().stash.get(&b).map(|s| (s.composer.text.clone(), s.pending_submit.is_some()));
    assert_eq!(moved, Some(("First message of chat B".to_owned(), true)), "the new chat's state went with it");
    c.picture("review-new-chat-late-open-01-chat-a");

    // Acceptance clears the sent text in the new chat only.
    c.send(ViewUpdate::ItemUpserted(TranscriptItem {
        id: ItemId(0x47),
        task: b,
        run: None,
        kind: ItemKind::UserMessage,
        text: "First message of chat B".into(),
        meta: None,
        completed: true,
        revision: 1,
    }));
    c.step(2);
    c.app().select_chat(b);
    c.step(2);
    assert_eq!(c.app().composer.text, "", "the sent message left the new chat's composer");
    assert!(c.app().send_blocked().is_none(), "the new chat is not stuck sending");
    c.app().select_chat(a);
    c.step(2);
    assert_eq!(c.app().composer.text, "Private draft from chat A");
    c.finish(json!({ "status": "pass", "selected_after_late_open": "chat A", "chat_a_draft": "kept" }));
}

/// C48: at the smallest window an approval card's actions stay on screen and
/// above the composer, also with a long command and a long draft.
#[test]
fn review_approval_fits_smallest_window() {
    let task = TaskId(0x5b5e_0000_0000_0000_0048_0000_0000_0001);
    let run = RunId(0x48);
    let mut c = Check::real("review-approval-small-window", [640.0, 480.0]);
    c.send(ViewUpdate::Chats { projects: vec![], chats: vec![chat_summary(task, "Approval")] });
    c.step(2);
    c.app().select_chat(task);
    c.send(ViewUpdate::RunStateChanged { task, run, state: RunState::WaitingForApproval });
    let card = |command: &str| ViewUpdate::Decisions {
        task,
        decisions: vec![DecisionView {
            id: DecisionId(0x48),
            task,
            run,
            generation: 1,
            kind: DecisionKind::Command { command: command.into(), cwd: Some("/work/Approval".into()), reason: None },
            state: DecisionState::Pending,
        }],
    };
    let mut seen = Vec::new();
    let cases = [
        ("short command", "touch file.txt".to_owned(), String::new()),
        (
            "long command and long draft",
            (1..=20).map(|i| format!("echo line {i} of a long script")).collect::<Vec<_>>().join("\n"),
            (1..=15).map(|i| format!("Line {i} of a long direction\n")).collect(),
        ),
    ];
    for (i, (name, command, draft)) in cases.into_iter().enumerate() {
        c.send(card(&command));
        if !draft.is_empty() {
            c.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
            c.step(1);
            c.harness.event(Event::Paste(draft));
        }
        c.step(4);
        let allow = c.harness.get_by_label("Allow once").rect();
        let deny = c.harness.get_by_label("Deny").rect();
        let composer_top = c.harness.get_all_by_label("Stop").map(|n| n.rect().top()).fold(f32::INFINITY, f32::min);
        for (label, rect) in [("Allow once", allow), ("Deny", deny)] {
            assert!(
                rect.top() >= 0.0 && rect.bottom() <= 480.0 && rect.right() <= 640.0,
                "{name}: {label} stays inside the window: {rect:?}"
            );
            assert!(rect.bottom() <= composer_top, "{name}: {label} sits above the composer: {rect:?}");
        }
        c.picture(&format!("review-approval-small-window-0{}", i + 1));
        seen.push(json!({ "case": name, "allow": format!("{allow:?}"), "deny": format!("{deny:?}") }));
    }
    // The card can be answered from there.
    c.harness.get_by_label("Allow once").click();
    c.step(2);
    let answered = c.commands.try_iter().any(|cmd| matches!(cmd, UiCommand::Answer { .. }));
    assert!(answered, "Allow once sends an answer");
    c.finish(json!({ "status": "pass", "cases": seen, "answered": answered }));
}

fn question_card(task: TaskId, run: RunId, choices: bool) -> ViewUpdate {
    let questions = (1..=3)
        .map(|i| bukno_core::decision::Question {
            id: format!("q{i}"),
            header: format!("Question {i}"),
            text: format!("Please answer question {i} before work continues."),
            options: if choices { vec!["Choice A".into(), "Choice B".into()] } else { vec![] },
            other: choices,
            multi: false,
        })
        .collect();
    ViewUpdate::Decisions {
        task,
        decisions: vec![DecisionView {
            id: DecisionId(0x51),
            task,
            run,
            generation: 1,
            kind: DecisionKind::Question { questions },
            state: DecisionState::Pending,
        }],
    }
}

/// The card's answer fields, top to bottom (the composer is a text field too).
fn text_inputs(c: &Check) -> Vec<egui::Rect> {
    let composer = composer_id().accesskit_id();
    c.harness
        .get_all_by_role(Role::MultilineTextInput)
        .filter(|n| n.accesskit_node().locate().0 != composer)
        .map(|n| n.rect())
        .collect()
}

/// C51 and C52: question cards keep every field and both actions reachable,
/// at full size and in the smallest window.
#[test]
fn review_question_cards_fit() {
    let task = TaskId(0x5b5e_0000_0000_0000_0051_0000_0000_0001);
    let run = RunId(0x51);
    let mut seen = Vec::new();
    for (name, size, choices) in [
        ("regular-choices", [1440.0, 900.0], true),
        ("minimum-choices", [640.0, 480.0], true),
        ("minimum-free", [640.0, 480.0], false),
    ] {
        let mut c = Check::real(&format!("review-question-cards-{name}"), size);
        c.send(ViewUpdate::Chats { projects: vec![], chats: vec![chat_summary(task, "Questions")] });
        c.step(2);
        c.app().select_chat(task);
        c.send(ViewUpdate::RunStateChanged { task, run, state: RunState::WaitingForInput });
        c.send(question_card(task, run, choices));
        c.step(4);
        let composer_top = c.harness.get_all_by_label("Stop").map(|n| n.rect().top()).fold(f32::INFINITY, f32::min);
        let (height, width) = (size[1], size[0]);
        let reachable = |r: egui::Rect| r.top() >= 0.0 && r.bottom() <= composer_top && r.right() <= width;
        for label in ["Send answer", "Skip"] {
            let rect = c.harness.get_by_label(label).rect();
            assert!(reachable(rect), "{name}: {label} is on screen above the composer: {rect:?}");
        }
        c.picture(&format!("review-question-cards-{name}-01"));
        let inputs = text_inputs(&c);
        let shown = inputs.iter().filter(|r| reachable(**r)).count();
        if name == "regular-choices" {
            assert_eq!(shown, 3, "{name}: every field fits at full size: {inputs:?}");
        } else {
            // The questions scroll inside the card: the first is at the top, the last one scrolls in.
            assert!(inputs.first().is_some_and(|r| reachable(*r)), "{name}: the first field shows: {inputs:?}");
            let area = c.harness.get_by_label("Send answer").rect();
            c.harness.event(Event::PointerMoved(Pos2::new(width / 2.0, area.top() - 40.0)));
            c.harness.event(Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: Vec2::new(0.0, -2000.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            });
            c.step(20);
            let inputs = text_inputs(&c);
            assert!(
                inputs.last().is_some_and(|r| r.top() >= 0.0 && r.bottom() <= height),
                "{name}: the last field scrolls into view: {inputs:?}"
            );
            c.picture(&format!("review-question-cards-{name}-02-scrolled"));
        }
        seen.push(json!({ "case": name, "fields_on_screen_at_start": shown }));
        c.finish(json!({ "status": "pass", "case": name, "fields_on_screen_at_start": shown }));
    }
    let dir = evidence_root().join("review-question-cards");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("result.json"),
        serde_json::to_string_pretty(&json!({ "status": "pass", "cases": seen })).unwrap(),
    )
    .unwrap();
}

/// A T3 chat is rebuilt with `Document::load` when older history arrives or
/// a row is hidden. The same item can then come back with new text. The
/// layout cache must not draw the text it measured for the old load.
#[test]
fn review_document_rebuild_draws_new_text() {
    let mut c = Check::new("review-document-rebuild", "empty", [1440.0, 900.0], true);
    c.app().view = bukno_desktop::app::View::Chat;
    let item = |text: &str| TranscriptItem {
        id: ItemId(STREAM_ITEM.0 + 9_000),
        task: synthetic::TASK,
        run: None,
        kind: ItemKind::AgentMessage { provider: Provider::Codex },
        text: text.to_owned(),
        meta: None,
        completed: true,
        revision: 1,
    };
    c.app().doc.load(&[item("OLD TEXT in the first load.")]);
    c.step(3);
    c.app().doc.load(&[item("NEW TEXT in the second load.")]);
    c.step(3);
    let doc = c.harness.get_by(|n| n.role() == Role::Document);
    let drawn = doc.accesskit_node().document_range().text();
    assert!(drawn.contains("NEW TEXT"), "drawn text after the rebuild: {drawn:?}");
    assert!(!drawn.contains("OLD TEXT"), "the old layout was reused: {drawn:?}");
}
