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

use bukno_core::event::ViewUpdate;
use bukno_core::ids::{ItemId, RunId};
use bukno_core::message::{ItemKind, Provider, TranscriptItem};
use bukno_core::run::RunState;
use bukno_desktop::app::{Backend, BuknoApp};
use bukno_desktop::components::composer::composer_id;
use bukno_desktop::theme::Theme;
use bukno_desktop::transcript::selection::{Selection, TextPos};
use bukno_desktop::transcript::transcript_id;
use bukno_platform::paths::AppPaths;
use bukno_runtime::synthetic::{self, GenBlock, Scenario};
use bukno_runtime::{UiCommand, UiEvent};
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

impl Check {
    fn new(name: &str, scenario: &str, size: [f32; 2], reduce_motion: bool) -> Self {
        let scenario = Scenario::named(scenario).expect("scenario");
        let (events, rx) = mpsc::channel();
        let (cmd_tx, commands) = mpsc::channel();
        let dir = evidence_root().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let state = dir.join("state");
        let work = dir.join("work");
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
        let options =
            SnapshotOptions::new().output_path(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"));
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
    assert!(c.app().composer.text.is_empty());
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

/// Covers F10: no repaint when idle, orb capped at 20 fps, reduced motion still.
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
    // The orb adds back egui's predicted frame time, so the reported delay is the cadence.
    let orb_delay = repaint_delay(&idle.harness);
    idle.record("orb_repaint_delays_ms", json!(delays));
    assert!(orb_delay >= Duration::from_millis(49), "orb never asks for more than 20 fps: {orb_delay:?}");
    assert!(orb_delay <= Duration::from_millis(51), "orb keeps its 20 fps cadence: {orb_delay:?}");
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
        "orb_repaint_delay_ms": orb_delay.as_secs_f64() * 1000.0,
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
