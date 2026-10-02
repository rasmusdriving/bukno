//! Setup (journey screen 1.1): the engines Bukno found, how they are signed
//! in, and the work folder for chats without a project. Also the screen for
//! when Bukno cannot run at all.

use bukno_runtime::engines::{EngineState, EngineView, Revert};
use bukno_runtime::{EngineAction, UiCommand};
use egui::{Align2, Id, Rect, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

use crate::app::BuknoApp;
use crate::components::icons::{self, Icon};
use crate::components::{focus_ring, provider_mark};
use crate::theme::Theme;

const COLUMN: f32 = 560.0;

/// Ask for a folder with the system dialog.
pub fn pick_folder(title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

fn pick_file(title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_file()
}

/// A text button: filled for the primary action, raised otherwise.
pub fn button(ui: &mut Ui, theme: &Theme, id: Id, at: egui::Pos2, text: &str, primary: bool) -> (egui::Response, f32) {
    let c = &theme.color;
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme.font(&theme.text.t_ui_strong), c.text_primary);
    let width = galley.size().x + 24.0;
    let rect = Rect::from_min_size(at, vec2(width, 32.0));
    let response = ui.interact(rect, id, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    let (fill, ink) = if primary { (c.action, c.on_action) } else { (c.surface_raised, c.text_primary) };
    let fill = if !enabled {
        c.surface_hover
    } else if response.hovered() {
        fill.gamma_multiply(1.08)
    } else {
        fill
    };
    let ink = if enabled { ink } else { c.text_disabled };
    ui.painter().rect_filled(rect, theme.radius.radius_md, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme.font(&theme.text.t_ui_strong), ink);
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    (response, width)
}

fn wrapped(
    ui: &Ui,
    theme: &Theme,
    text: &str,
    style: &crate::theme::TypeStyle,
    color: egui::Color32,
    at: egui::Pos2,
    width: f32,
) -> f32 {
    let galley = ui.painter().layout_job(theme.job(text, style, color, width));
    let h = galley.size().y;
    ui.painter().galley(at, galley, color);
    h
}

pub fn show(app: &mut BuknoApp, ui: &mut Ui, full: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let left = full.center().x - COLUMN / 2.0;
    let mut y = full.top() + (full.height() * 0.1).clamp(44.0, 110.0);
    ui.painter().text(pos2(left, y), Align2::LEFT_TOP, "bukno", theme.font(&theme.text.t_ui_strong), c.text_secondary);
    y += 30.0;
    ui.painter().text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Set up your engines",
        theme.font(&theme.text.t_display),
        c.text_primary,
    );
    y += 50.0;
    y += wrapped(
        ui,
        &theme,
        "Bukno runs the Codex and Claude Code already installed on this computer. You sign in inside each engine, and Bukno never stores those credentials.",
        &theme.text.t_body,
        c.text_secondary,
        pos2(left, y),
        COLUMN,
    ) + 24.0;

    let engine = app.engine.clone();
    y = codex_card(app, ui, &theme, pos2(left, y), engine.as_ref()) + 12.0;
    y = claude_card(ui, &theme, pos2(left, y)) + 24.0;

    // Work folder.
    ui.painter().text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Chat workspaces",
        theme.font(&theme.text.t_small),
        c.text_secondary,
    );
    y += 22.0;
    let setup = app.setup.clone();
    let folder = setup.as_ref().and_then(|s| s.work_folder.clone());
    let missing = setup.as_ref().is_some_and(|s| s.work_folder_missing);
    let row = Rect::from_min_size(pos2(left, y), vec2(COLUMN, 44.0));
    ui.painter().rect_filled(row, theme.radius.radius_lg, c.surface_composer);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(row.left() + 22.0, row.center().y), vec2(16.0, 16.0)),
        Icon::Folder,
        16.0,
        c.text_secondary,
    );
    let shown = folder.clone().unwrap_or_else(|| "Choose a folder for chats without a project".into());
    let mut job = theme.job(
        shown,
        &theme.text.t_mono_small,
        if folder.is_some() { c.text_primary } else { c.text_tertiary },
        COLUMN - 140.0,
    );
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(row.left() + 40.0, row.center().y - galley.size().y / 2.0), galley, c.text_primary);
    let label = if folder.is_some() { "Change" } else { "Choose…" };
    let (change, _) =
        button(ui, &theme, Id::new("setup-work-folder"), pos2(row.right() - 90.0, row.top() + 6.0), label, false);
    if change.clicked()
        && let Some(path) = pick_folder("Choose the work folder for chats without a project")
    {
        app.send_command(UiCommand::SetWorkFolder { path });
    }
    y += 52.0;
    let hint = if missing {
        "This folder is missing (is its drive connected?). Chats in it are unavailable until it returns."
    } else {
        "Each chat without a project gets its own folder here. App state stays on this Mac's internal drive."
    };
    y += wrapped(
        ui,
        &theme,
        hint,
        &theme.text.t_small,
        if missing { c.negative } else { c.text_tertiary },
        pos2(left, y),
        COLUMN,
    ) + 8.0;
    if let Some(setup) = setup.as_ref().filter(|s| s.overridden) {
        y += wrapped(
            ui,
            &theme,
            &format!("Development override: app state in {}.", setup.state_dir),
            &theme.text.t_small,
            c.attention,
            pos2(left, y),
            COLUMN,
        ) + 8.0;
    }
    y += 20.0;
    let ready = folder.is_some();
    ui.add_enabled_ui(ready, |ui| {
        let (go, w) = button(ui, &theme, Id::new("setup-continue"), pos2(left, y), "Continue with Codex", true);
        if go.clicked() {
            app.show_setup = false;
        }
        ui.painter().text(
            pos2(left + w + 16.0, y + 16.0),
            Align2::LEFT_CENTER,
            "Claude Code arrives in the next version of Bukno.",
            theme.font(&theme.text.t_ui),
            c.text_secondary,
        );
    });
    if let Some(banner) = app.banner.clone() {
        let at = pos2(left, y + 52.0);
        wrapped(ui, &theme, &banner, &theme.text.t_small, c.text_secondary, at, COLUMN);
    }
}

fn status_words(view: Option<&EngineView>) -> (&'static str, Option<Icon>) {
    match view.map(|v| &v.state) {
        None | Some(EngineState::Found) => ("Found", None),
        Some(EngineState::Starting) => ("Checking…", None),
        Some(EngineState::Ready) => ("Ready", Some(Icon::Check)),
        Some(EngineState::SignedOut) => ("Sign in needed", Some(Icon::Alert)),
        Some(EngineState::NotFound) => ("Not found", Some(Icon::Alert)),
        Some(EngineState::NotWorking) => ("Not working", Some(Icon::Alert)),
    }
}

fn codex_card(app: &mut BuknoApp, ui: &mut Ui, theme: &Theme, at: egui::Pos2, view: Option<&EngineView>) -> f32 {
    let c = &theme.color;
    // Lay out the lines first to size the card.
    let mut lines = Vec::new();
    if let Some(v) = view {
        if let Some(version) = &v.version {
            let tested = if v.tested { "tested with Bukno" } else { "not yet tested with Bukno" };
            lines.push(format!("codex-cli {version} · {tested}"));
        }
        if let Some(path) = &v.path {
            lines.push(path.clone());
        }
        if let Some(account) = &v.account {
            lines.push(account.clone());
        }
        if let Some(pin) = &v.pinned {
            lines.push(format!("Pinned to {pin}"));
        }
    }
    let detail = view.and_then(|v| v.detail.clone()).or_else(|| view.and_then(|v| v.note.clone()));
    let mut text = detail.unwrap_or_default();
    if let Some(v) = view
        && v.state == EngineState::NotWorking
        && let (Some(version), Some(last)) = (&v.version, &v.last_working)
        && version != last
    {
        text = format!("Codex {version} is not working with Bukno. The last working version is {last}. {text}");
    }
    let width = COLUMN - 32.0;
    let text_h = if text.is_empty() {
        0.0
    } else {
        ui.painter().layout_job(theme.job(text.clone(), &theme.text.t_ui, c.text_secondary, width)).size().y + 10.0
    };
    let height = 52.0 + lines.len() as f32 * 20.0 + text_h + 48.0;
    let card = Rect::from_min_size(at, vec2(COLUMN, height));
    ui.painter().rect_filled(card, theme.radius.radius_lg, c.surface_composer);
    let x = card.left() + 16.0;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(x + 9.0, card.top() + 26.0), vec2(18.0, 18.0)),
        provider_mark(bukno_core::message::Provider::Codex),
        18.0,
        c.codex,
    );
    ui.painter().text(
        pos2(x + 32.0, card.top() + 26.0),
        Align2::LEFT_CENTER,
        "Codex",
        theme.font(&theme.text.t_ui_strong),
        c.text_primary,
    );
    let (status, icon) = status_words(view);
    let right = card.right() - 16.0;
    let status_galley = ui.painter().layout_no_wrap(status.to_owned(), theme.font(&theme.text.t_ui), c.text_secondary);
    let sw = status_galley.size().x;
    ui.painter().galley(
        pos2(right - sw, card.top() + 26.0 - status_galley.size().y / 2.0),
        status_galley,
        c.text_secondary,
    );
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(right - sw - 14.0, card.top() + 26.0), vec2(14.0, 14.0)),
            icon,
            13.0,
            c.text_secondary,
        );
    }
    let mut y = card.top() + 48.0;
    for line in &lines {
        let mut job = theme.job(line.clone(), &theme.text.t_mono_small, c.text_secondary, width);
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(pos2(x, y), galley, c.text_secondary);
        y += 20.0;
    }
    if !text.is_empty() {
        y += 6.0;
        y += wrapped(ui, theme, &text, &theme.text.t_ui, c.text_secondary, pos2(x, y), width) + 4.0;
    }
    // Actions.
    let mut bx = x;
    let bar = y + 8.0;
    let mut action = |ui: &mut Ui, label: &str, key: &str, primary: bool| -> bool {
        let (response, w) = button(ui, theme, Id::new(("engine-action", key)), pos2(bx, bar), label, primary);
        bx += w + 8.0;
        response.clicked()
    };
    let revert = view.and_then(|v| v.revert.clone());
    match &revert {
        Some(Revert::UseFile { version, .. }) => {
            if action(ui, &format!("Use {version}"), "use", true) {
                app.engine_action(EngineAction::UsePrevious);
            }
        }
        Some(Revert::Command { shown, .. }) => {
            if action(ui, &format!("Run {shown}"), "revert", true) {
                app.engine_action(EngineAction::RunRevert);
            }
        }
        Some(Revert::Manual { .. }) | None => {}
    }
    if action(ui, "Check again", "check", revert.is_none() && view.is_none_or(|v| v.state != EngineState::Ready)) {
        app.engine_action(EngineAction::CheckAgain);
    }
    if view.is_some_and(|v| v.pinned.is_some()) && action(ui, "Try latest version", "latest", false) {
        app.engine_action(EngineAction::TryLatest);
    }
    if view.is_none_or(|v| matches!(v.state, EngineState::NotFound | EngineState::NotWorking))
        && action(ui, "Choose file…", "choose", false)
        && let Some(path) = pick_file("Choose the Codex executable")
    {
        app.engine_action(EngineAction::Choose(path));
    }
    if let Some(Revert::Manual { shown }) = &revert {
        ui.painter().text(
            pos2(bx + 8.0, bar + 16.0),
            Align2::LEFT_CENTER,
            format!("To go back, run {shown} in Terminal."),
            theme.font(&theme.text.t_small),
            c.text_secondary,
        );
    }
    card.bottom()
}

fn claude_card(ui: &mut Ui, theme: &Theme, at: egui::Pos2) -> f32 {
    let c = &theme.color;
    let card = Rect::from_min_size(at, vec2(COLUMN, 76.0));
    ui.painter().rect_filled(card, theme.radius.radius_lg, c.surface_composer);
    let x = card.left() + 16.0;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(x + 9.0, card.top() + 26.0), vec2(18.0, 18.0)),
        provider_mark(bukno_core::message::Provider::Claude),
        18.0,
        c.claude,
    );
    ui.painter().text(
        pos2(x + 32.0, card.top() + 26.0),
        Align2::LEFT_CENTER,
        "Claude Code",
        theme.font(&theme.text.t_ui_strong),
        c.text_primary,
    );
    ui.painter().text(
        pos2(card.right() - 16.0, card.top() + 26.0),
        Align2::RIGHT_CENTER,
        "Not connected yet",
        theme.font(&theme.text.t_ui),
        c.text_tertiary,
    );
    ui.painter().text(
        pos2(x, card.top() + 54.0),
        Align2::LEFT_CENTER,
        "Bukno does not run Claude Code yet. This version works with Codex only.",
        theme.font(&theme.text.t_ui),
        c.text_secondary,
    );
    card.bottom()
}

/// Bukno cannot run: another copy holds the state folder, or the database
/// needs attention. Nothing is changed.
pub fn blocked(app: &mut BuknoApp, ui: &mut Ui, full: Rect, title: &str, detail: &str) {
    let theme = app.theme.clone();
    let c = &theme.color;
    ui.painter().rect_filled(full, 0.0, c.surface_canvas);
    let left = full.center().x - COLUMN / 2.0;
    let mut y = full.top() + 140.0;
    ui.painter().text(pos2(left, y), Align2::LEFT_TOP, title, theme.font(&theme.text.t_title), c.text_primary);
    y += 44.0;
    wrapped(ui, &theme, detail, &theme.text.t_body, c.text_secondary, pos2(left, y), COLUMN);
}
