//! Automatic T3 onboarding, with the existing direct Codex recovery setup.
//! The native screen only reads published status; setup runs in the client.

use bukno_runtime::engines::{EngineState, EngineView, Revert};
use bukno_runtime::{EngineAction, UiCommand};
use bukno_t3_client::ConnectionStatus;
use bukno_t3_client::local::SetupStatus;
use egui::{Align2, Id, Rect, ScrollArea, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

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

/// The default onboarding uses T3. The direct Codex setup remains available
/// when T3 is explicitly disabled for development or recovery.
pub fn show(app: &mut BuknoApp, ui: &mut Ui, full: Rect) {
    if app.t3.is_none() {
        return show_direct(app, ui, full);
    }
    let theme = app.theme.clone();
    let c = &theme.color;
    let left = full.center().x - COLUMN / 2.0;
    let area = Rect::from_min_max(pos2(left - 24.0, full.top() + 44.0), pos2(left + COLUMN + 24.0, full.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    ScrollArea::vertical().id_salt("setup-t3").auto_shrink([false, false]).show(&mut child, |ui| {
        ui.set_width(area.width());
        let origin = ui.cursor().min;
        let left = origin.x + 24.0;
        let mut y = origin.y + 28.0;
        ui.painter().text(pos2(left, y), Align2::LEFT_TOP, "bukno", theme.font(&theme.text.t_ui_strong), c.text_secondary);
        y += 30.0;
        ui.painter().text(pos2(left, y), Align2::LEFT_TOP, "Let's get you connected", theme.font(&theme.text.t_display), c.text_primary);
        y += 52.0;
        y += wrapped(ui, &theme, "Bukno uses T3 Code to run Codex and Claude. We'll find it on this computer and connect for you. If it's missing, one download gets it ready.", &theme.text.t_body, c.text_secondary, pos2(left, y), COLUMN) + 24.0;
        y = local_card(app, ui, &theme, pos2(left, y), COLUMN) + 24.0;
        let ready = app.t3.as_ref().and_then(|t| t.ready_environment()).map(|e| (e.saved.environment_id.clone(), e.projects.first().map(|p| p.id.clone())));
        let folder = app.setup.as_ref().and_then(|s| s.work_folder.clone());
        let missing = app.setup.as_ref().is_some_and(|s| s.work_folder_missing);
        let needs_folder = ready.as_ref().is_none_or(|(_, project)| project.is_none());
        if needs_folder {
            ui.painter().text(pos2(left, y), Align2::LEFT_TOP, "Your chat folder", theme.font(&theme.text.t_title), c.text_primary);
            y += 32.0;
            y += wrapped(ui, &theme, folder.as_deref().unwrap_or("Choose where chats without a project can keep their files."), &theme.text.t_small, c.text_secondary, pos2(left, y), COLUMN) + 12.0;
            let mut x = left;
            if folder.is_none() {
                let (recommended, width) = button(ui, &theme, Id::new("setup-recommended-folder"), pos2(x, y), "Use recommended folder", false);
                x += width + 8.0;
                if recommended.clicked() {
                    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(std::path::PathBuf::from);
                    if let Some(home) = home {
                        let path = home.join("Documents/Bukno chats");
                        match std::fs::create_dir_all(&path) {
                            Ok(()) => app.send_command(UiCommand::SetWorkFolder { path }),
                            Err(_) => app.banner = Some("The recommended folder could not be created. Choose another folder.".into()),
                        }
                    }
                }
            }
            let (choose, _) = button(ui, &theme, Id::new("setup-work-folder"), pos2(x, y), if folder.is_some() { "Change folder…" } else { "Choose folder…" }, false);
            if choose.clicked() && let Some(path) = pick_folder("Choose your chat folder") {
                app.send_command(UiCommand::SetWorkFolder { path });
            }
            y += 48.0;
            if missing {
                y += wrapped(ui, &theme, "This folder is unavailable. Connect its drive or choose another folder.", &theme.text.t_small, c.negative, pos2(left, y), COLUMN) + 12.0;
            }
        }
        let busy = app.t3.as_ref().is_some_and(|t| t.view.local_setup.busy());
        let local_project = ready.as_ref().is_some_and(|(environment, _)| app.t3.as_ref().is_some_and(|t| matches!(&t.view.local_setup, SetupStatus::Ready { environment_id, .. } if environment_id == environment)));
        let can_continue = ready.is_some() && (!needs_folder || (local_project && folder.is_some() && !missing)) && !busy && app.finish_t3_setup.is_none();
        if let Some(env) = app.t3.as_ref().and_then(|t| t.ready_environment()) {
            let hint = env.projects.first().map(|p| format!("Continue opens a new chat in {}. Your other projects are in the sidebar.", p.title))
                .unwrap_or_else(|| if local_project { "Bukno will prepare this folder for your first chat.".into() } else { "Add your first project in T3 Code before continuing.".into() });
            y += wrapped(ui, &theme, &hint, &theme.text.t_small, c.text_secondary, pos2(left, y), COLUMN) + 16.0;
        }
        ui.add_enabled_ui(can_continue, |ui| {
            let (go, _) = button(ui, &theme, Id::new("setup-continue"), pos2(left, y), if app.finish_t3_setup.is_some() { "Preparing chats…" } else { "Continue" }, true);
            if go.clicked() && let Some((environment, project)) = ready {
                if let Some(project) = project {
                    app.show_setup = false;
                    app.new_remote_chat(environment, project);
                } else if let Some(folder) = folder {
                    let path = bukno_t3_client::local::workspace_path(std::path::Path::new(&folder));
                    app.finish_t3_setup = Some((environment, std::time::Instant::now()));
                    if let Some(t3) = app.t3.as_ref() { t3.add_local_project(path); }
                }
            }
        });
        y += 48.0;
        let (advanced, _) = button(ui, &theme, Id::new("setup-advanced"), pos2(left, y), "Advanced connection", false);
        if advanced.clicked() {
            if let Some(t3) = app.t3.as_mut() { t3.advanced_setup = true; }
            app.show_environments = true;
        }
        y += 48.0;
        if let Some(banner) = &app.banner {
            y += wrapped(ui, &theme, banner, &theme.text.t_small, c.attention, pos2(left, y), COLUMN);
        }
        ui.allocate_space(vec2(area.width(), (y - origin.y + 24.0).max(0.0)));
    });
}

/// Shared local setup card, also shown in connection settings.
pub fn local_card(app: &mut BuknoApp, ui: &mut Ui, theme: &Theme, at: egui::Pos2, width: f32) -> f32 {
    let Some(t3) = app.t3.as_ref() else { return at.y };
    let c = &theme.color;
    let status = t3.view.local_setup.clone();
    let local = match &status {
        SetupStatus::Ready { environment_id, .. } => t3.environment(environment_id),
        _ => None,
    };
    let (message, problem) = if let Some(env) = local {
        match &env.status {
            ConnectionStatus::Connected { current: true } => {
                ("Connected. Your projects, chats and models are ready.".to_owned(), false)
            }
            ConnectionStatus::NeedsPairing { reason }
            | ConnectionStatus::Blocked { reason }
            | ConnectionStatus::Reconnecting { reason, .. } => (reason.clone(), true),
            _ => ("Connecting to T3 Code…".into(), false),
        }
    } else {
        match &status {
            SetupStatus::Idle => ("Find T3 Code on this computer and connect automatically.".into(), false),
            SetupStatus::Working(message) => (message.clone(), false),
            SetupStatus::Missing => ("T3 Code isn't installed. Download it here and Bukno will start it and connect for you. No terminal or extra setup needed.".into(), false),
            SetupStatus::Ready { .. } => ("Connecting to T3 Code…".into(), false),
            SetupStatus::Failed(message) => (message.clone(), true),
        }
    };
    let galley = ui.painter().layout_job(theme.job(
        &message,
        &theme.text.t_ui,
        if problem { c.negative } else { c.text_secondary },
        width - 32.0,
    ));
    let has_action = !status.busy();
    let height = 52.0 + galley.size().y + if has_action { 64.0 } else { 20.0 };
    let card = Rect::from_min_size(at, vec2(width, height));
    ui.painter().rect_filled(card, theme.radius.radius_lg, c.surface_composer);
    ui.painter().text(
        pos2(at.x + 16.0, at.y + 24.0),
        Align2::LEFT_CENTER,
        "T3 Code",
        theme.font(&theme.text.t_ui_strong),
        c.text_primary,
    );
    let y = at.y + 48.0;
    let h = galley.size().y;
    ui.painter().galley(pos2(at.x + 16.0, y), galley, c.text_secondary);
    if has_action {
        let download = status == SetupStatus::Missing;
        let (action, _) = button(
            ui,
            theme,
            Id::new("setup-local-t3"),
            pos2(at.x + 16.0, y + h + 16.0),
            if download { "Download T3 Code" } else { "Check again" },
            download,
        );
        if action.clicked() {
            t3.setup_local(download);
        }
    }
    card.bottom()
}

fn show_direct(app: &mut BuknoApp, ui: &mut Ui, full: Rect) {
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
