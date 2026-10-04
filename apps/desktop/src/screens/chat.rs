//! The chat canvas: titlebar, chat header, transcript, working indicator
//! and composer. Also the empty new-chat screen.

use bukno_core::run::RunState;
use egui::{Align2, Id, Rect, Sense, Ui, ViewportCommand, pos2, vec2};

use bukno_core::decision::{DecisionAnswer, DecisionKind};
use bukno_core::event::WaitReason;

use crate::app::{BuknoApp, Menu, QuitFlow, View};
use crate::components::composer::{self, ComposerAction, ComposerProps};
use crate::components::decision::{self, CardAction};
use crate::components::icons::{self, Icon};
use crate::components::orb::{self, OrbState, Working};
use crate::components::{icon_button, provider_color, provider_name};
use crate::transcript::column_rect;

/// Height reserved after the last message for the working indicator.
pub(crate) const WORKING_HEIGHT: f32 = 56.0;
/// Transcript space left beside a decision card in a small window.
pub(crate) const DECISION_TRANSCRIPT_MIN: f32 = 16.0;
/// Shortest a question card gets: its actions plus room to scroll the questions.
pub(crate) const QUESTION_CARD_MIN: f32 = 220.0;

/// The canvas part of the titlebar: a drag region with the breadcrumb.
pub fn titlebar(app: &mut BuknoApp, ui: &mut Ui, canvas: Rect, sidebar_shown: bool) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let height = theme.size.size_titlebar;
    let bar = Rect::from_min_size(canvas.min, vec2(canvas.width(), height));
    // Not focusable: dragging the window has no keyboard equivalent here.
    let drag = ui.interact(bar, Id::new("titlebar"), Sense::CLICK | Sense::DRAG);
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    // Sidebar toggle: inside the sidebar when it is open, after the traffic lights otherwise.
    let toggle_x = if sidebar_shown { canvas.left() - 24.0 - 16.0 } else { canvas.left() + 84.0 };
    let toggle = Rect::from_center_size(pos2(toggle_x + 16.0, bar.center().y), vec2(28.0, 28.0));
    let label = if app.sidebar_open { "Hide sidebar" } else { "Show sidebar" };
    if icon_button(ui, &theme, Id::new("toggle-sidebar"), toggle, Icon::Sidebar, label).clicked() {
        app.sidebar_open = !app.sidebar_open;
    }

    let mut x = if sidebar_shown { canvas.left() + 24.0 } else { toggle.right() + 16.0 };
    let painter = ui.painter();
    if !app.is_synthetic() {
        let (_, name) = breadcrumb(app);
        icons::paint(
            painter,
            Rect::from_center_size(pos2(x + 7.0, bar.center().y), vec2(14.0, 14.0)),
            Icon::Folder,
            14.0,
            c.text_secondary,
        );
        painter.text(
            pos2(x + 22.0, bar.center().y),
            Align2::LEFT_CENTER,
            name,
            theme.font(&theme.text.t_small),
            c.text_secondary,
        );
        if app.view == View::Chat && app.selected_summary().is_some_and(|s| s.shared_workspace) {
            painter.text(
                pos2(bar.right() - 20.0, bar.center().y),
                Align2::RIGHT_CENTER,
                "Sharing this folder with another chat",
                theme.font(&theme.text.t_small),
                c.attention,
            );
        }
        return;
    }
    match app.view {
        View::Chat => {
            painter.text(
                pos2(x, bar.center().y),
                Align2::LEFT_CENTER,
                "Bukno",
                theme.font(&theme.text.t_small),
                c.text_secondary,
            );
            x += painter.layout_no_wrap("Bukno".into(), theme.font(&theme.text.t_small), c.text_secondary).size().x
                + 8.0;
            painter.text(
                pos2(x, bar.center().y),
                Align2::LEFT_CENTER,
                "/",
                theme.font(&theme.text.t_small),
                c.text_tertiary,
            );
            x += 14.0;
            painter.text(
                pos2(x, bar.center().y),
                Align2::LEFT_CENTER,
                "Synthetic project, no Git",
                theme.font(&theme.text.t_small),
                c.text_tertiary,
            );
        }
        View::NewChat | View::Remote | View::RemoteNew => {
            icons::paint(
                painter,
                Rect::from_center_size(pos2(x + 7.0, bar.center().y), vec2(14.0, 14.0)),
                Icon::Folder,
                14.0,
                c.text_secondary,
            );
            painter.text(
                pos2(x + 22.0, bar.center().y),
                Align2::LEFT_CENTER,
                "Chat workspace",
                theme.font(&theme.text.t_small),
                c.text_secondary,
            );
        }
    }
    // The mode label is always visible, so synthetic content is never mistaken for real work.
    painter.text(
        pos2(bar.right() - 20.0, bar.center().y),
        Align2::RIGHT_CENTER,
        "Synthetic scenario · no engines",
        theme.font(&theme.text.t_small),
        c.text_tertiary,
    );
}

/// The folder a chat works in, for the titlebar: (projectless, name).
fn breadcrumb(app: &BuknoApp) -> (bool, String) {
    let project = match app.view {
        View::Chat => app.selected_summary().and_then(|s| s.project),
        View::NewChat => app.new_chat_project,
        View::Remote => {
            // The T3 server's label and the project, such as "Ubuntu-box / bukno".
            let t3 = app.t3.as_ref();
            let chat = app.remote.as_ref();
            let env = t3.zip(chat).and_then(|(t, r)| t.environment(&r.environment));
            let project = env.and_then(|e| {
                let thread = e.threads.iter().find(|t| Some(&t.id) == chat.map(|r| &r.thread))?;
                e.projects.iter().find(|p| p.id == thread.project_id).map(|p| p.title.clone())
            });
            let label = env.map_or("T3", |e| e.saved.label.as_str());
            return (false, project.map_or_else(|| label.to_owned(), |p| format!("{label} / {p}")));
        }
        View::RemoteNew => {
            let new = app.remote_new.as_ref();
            let env = app.t3.as_ref().zip(new).and_then(|(t, n)| t.environment(&n.environment));
            let project = env.zip(new).and_then(|(e, n)| e.projects.iter().find(|p| p.id == n.project));
            let label = env.map_or("T3", |e| e.saved.label.as_str());
            return (false, project.map_or_else(|| label.to_owned(), |p| format!("{label} / {}", p.title)));
        }
    };
    match project.and_then(|p| app.projects.iter().find(|x| x.id == p)) {
        Some(project) => (false, project.name.clone()),
        None => (true, "Chat workspace".into()),
    }
}

pub fn show(app: &mut BuknoApp, ui: &mut Ui, canvas: Rect) {
    let body = Rect::from_min_max(pos2(canvas.left(), canvas.top() + app.theme.size.size_titlebar), canvas.max);
    match app.view {
        View::NewChat => new_chat(app, ui, body),
        View::Chat => chat(app, ui, body),
        View::Remote => super::remote::chat(app, ui, body),
        View::RemoteNew => super::remote::new_chat(app, ui, body),
    }
}

fn composer_props(app: &BuknoApp) -> (String, bool) {
    let provider = provider_name(app.provider());
    let running = app.run.is_some();
    let placeholder = if running {
        "Add a direction while work continues…".to_owned()
    } else if app.view == View::NewChat {
        format!("Ask {provider} to do something")
    } else {
        format!("Follow up with {provider}")
    };
    (placeholder, running)
}

/// Model and effort as the engine reported them for this chat.
fn model_words(app: &BuknoApp) -> (String, String, u8) {
    if app.is_synthetic() {
        return ("Synthetic model".into(), "Medium".into(), 3);
    }
    let reported = app.extra.model.clone().filter(|_| app.view == View::Chat);
    let (model, effort) = match reported {
        Some(text) => match text.split_once(" · ") {
            Some((m, e)) => (m.to_owned(), e.to_owned()),
            None => (text, "Default".to_owned()),
        },
        None => (
            app.engine.as_ref().and_then(|e| e.default_model.clone()).unwrap_or_else(|| "Codex default".into()),
            "Default".to_owned(),
        ),
    };
    let level = match effort.to_lowercase().as_str() {
        "minimal" | "low" => 1,
        "medium" | "default" => 3,
        "high" => 4,
        _ => 5,
    };
    (model, effort, level)
}

fn preset_label(app: &BuknoApp) -> String {
    if app.is_synthetic() {
        return "Synthetic engine".into();
    }
    let id = app.preset().unwrap_or_default();
    bukno_runtime::coordinator::codex_presets()
        .iter()
        .find(|p| p.id == id)
        .map_or("Ask before commands", |p| p.label)
        .to_owned()
}

fn draw_composer(app: &mut BuknoApp, ui: &mut Ui, rect: Rect) {
    let (placeholder, running) = composer_props(app);
    let theme = app.theme.clone();
    let (model, effort, effort_level) = model_words(app);
    let permission = preset_label(app);
    let props = ComposerProps {
        provider: app.provider(),
        placeholder: &placeholder,
        running,
        model: &model,
        effort: &effort,
        effort_level,
        permission: &permission,
        permission_menu: !app.is_synthetic(),
        send_blocked: app.send_blocked(),
        steer: false,
    };
    match composer::show(ui, &theme, rect, &mut app.composer, &props) {
        Some(ComposerAction::Send | ComposerAction::Steer) => app.submit(),
        Some(ComposerAction::Stop) => app.stop(),
        Some(ComposerAction::Permission) => {
            app.menu = if app.menu == Some(Menu::Permission) { None } else { Some(Menu::Permission) };
        }
        None => {}
    }
    if app.menu == Some(Menu::Permission) {
        permission_menu(app, ui, rect);
    }
}

/// The Codex presets, each with what it allows and what enforces it.
fn permission_menu(app: &mut BuknoApp, ui: &mut Ui, composer: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let presets = bukno_runtime::coordinator::codex_presets();
    let width = 380.0;
    let item_h = 64.0;
    let height = presets.len() as f32 * item_h + 40.0;
    let rect =
        Rect::from_min_size(pos2(composer.left() + 40.0, composer.bottom() - 52.0 - height), vec2(width, height));
    let current = app.preset().unwrap_or_default();
    let mut chosen = None;
    egui::Area::new(Id::new("permission-menu")).order(egui::Order::Foreground).fixed_pos(rect.min).show(
        ui.ctx(),
        |ui| {
            theme.paint_shadow(ui.painter(), rect, theme.radius.radius_lg, &theme.shadow.shadow_popover);
            ui.painter().rect_filled(rect, theme.radius.radius_lg, c.surface_popover);
            let mut y = rect.top() + 8.0;
            for preset in presets {
                let item = Rect::from_min_size(pos2(rect.left() + 6.0, y), vec2(width - 12.0, item_h - 4.0));
                let response = ui.interact(item, Id::new(("preset", preset.id)), Sense::click());
                let selected = preset.id == current;
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, preset.label)
                });
                if response.hovered() || selected {
                    ui.painter().rect_filled(
                        item,
                        theme.radius.radius_md,
                        if selected { c.surface_selected } else { c.surface_hover },
                    );
                }
                ui.painter().text(
                    pos2(item.left() + 10.0, item.top() + 14.0),
                    Align2::LEFT_CENTER,
                    preset.label,
                    theme.font(&theme.text.t_ui_strong),
                    c.text_primary,
                );
                let mut job = theme.job(preset.description, &theme.text.t_small, c.text_secondary, item.width() - 20.0);
                job.wrap.max_rows = 2;
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(pos2(item.left() + 10.0, item.top() + 26.0), galley, c.text_secondary);
                crate::components::focus_ring(ui, &theme, &response, theme.radius.radius_md);
                if response.clicked() {
                    chosen = Some(preset.id);
                }
                y += item_h;
            }
            let mut job = theme.job(presets[0].protection, &theme.text.t_small, c.text_tertiary, width - 28.0);
            job.wrap.max_rows = 2;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(pos2(rect.left() + 16.0, y + 2.0), galley, c.text_tertiary);
        },
    );
    if let Some(id) = chosen {
        app.send_command(bukno_runtime::UiCommand::SetPreset { preset: id.to_owned() });
        app.menu = None;
    } else if ui.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
        && !ui.ctx().pointer_interact_pos().is_some_and(|p| rect.contains(p))
    {
        app.menu = None;
    }
}

fn new_chat(app: &mut BuknoApp, ui: &mut Ui, body: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let column = column_rect(body, &theme);
    // Height budget: greeting, composer and the project line must fit, so
    // short windows shrink the top space first, then drop the greeting.
    const GREETING: f32 = 34.0 + 24.0;
    const META: f32 = 36.0 + 16.0;
    const MARGIN: f32 = 16.0;
    let with_greeting = body.height() - 2.0 * MARGIN - GREETING - META;
    // Keep the greeting while at least the smallest composer fits beside it.
    let minimum = composer::height_for(ui, &theme, &app.composer, column.width(), 0.0);
    let show_greeting = with_greeting >= minimum;
    let budget = if show_greeting { with_greeting } else { body.height() - 2.0 * MARGIN - META };
    let height = composer::height_for(ui, &theme, &app.composer, column.width(), budget);
    let content = height + META + if show_greeting { GREETING } else { 0.0 };
    // 128 points above the greeting, as in the reference, whenever it all
    // fits; only a window too short for that moves it up. Typing never
    // moves the greeting while there is room.
    let top = body.top() + (body.height() - content - MARGIN).clamp(MARGIN, 128.0);
    let composer_top = if show_greeting {
        ui.painter().text(
            pos2(column.center().x, top + 17.0),
            Align2::CENTER_CENTER,
            "What should we work on?",
            theme.font(&theme.text.t_display),
            c.text_primary,
        );
        top + GREETING
    } else {
        top
    };
    let rect = Rect::from_min_size(pos2(column.left(), composer_top), vec2(column.width(), height));
    if let Some(notice) = app.notice.clone() {
        ui.painter().text(
            pos2(column.left(), rect.top() - 14.0),
            Align2::LEFT_CENTER,
            notice,
            theme.font(&theme.text.t_small),
            c.negative,
        );
    }
    draw_composer(app, ui, rect);
    let meta_y = rect.bottom() + 36.0;
    let project = app.new_chat_project.and_then(|p| app.projects.iter().find(|x| x.id == p)).cloned();
    let name = project.as_ref().map_or("No project".to_owned(), |p| p.name.clone());
    let painter = ui.painter();
    icons::paint(
        painter,
        Rect::from_center_size(pos2(column.left() + 21.0, meta_y), vec2(14.0, 14.0)),
        Icon::Folder,
        14.0,
        c.text_secondary,
    );
    let name_galley = painter.layout_no_wrap(name.clone(), theme.font(&theme.text.t_ui), c.text_secondary);
    let name_w = name_galley.size().x;
    painter.galley(pos2(column.left() + 34.0, meta_y - name_galley.size().y / 2.0), name_galley, c.text_secondary);
    match &project {
        Some(p) => {
            let mut job =
                theme.job(p.path.clone(), &theme.text.t_small, c.text_tertiary, column.width() - name_w - 80.0);
            job.wrap.max_rows = 1;
            job.wrap.overflow_character = Some('…');
            let galley = painter.layout_job(job);
            painter.galley(
                pos2(column.right() - 6.0 - galley.size().x, meta_y - galley.size().y / 2.0),
                galley,
                c.text_tertiary,
            );
        }
        None => {
            painter.text(
                pos2(column.right() - 6.0, meta_y),
                Align2::RIGHT_CENTER,
                "Saved in its own workspace folder",
                theme.font(&theme.text.t_small),
                c.text_tertiary,
            );
        }
    }
    if !app.is_synthetic() && !app.projects.is_empty() {
        let pick = Rect::from_min_size(pos2(column.left() + 10.0, meta_y - 14.0), vec2(name_w + 44.0, 28.0));
        let response = ui.interact(pick, Id::new("new-chat-project"), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Project: {name}")));
        if response.hovered() {
            ui.painter().rect_stroke(
                pick,
                theme.radius.radius_md,
                egui::Stroke::new(1.0, c.surface_hover),
                egui::StrokeKind::Inside,
            );
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(pick.right() - 10.0, meta_y), vec2(12.0, 12.0)),
            Icon::ChevronDown,
            11.0,
            c.text_tertiary,
        );
        crate::components::focus_ring(ui, &theme, &response, theme.radius.radius_md);
        if response.clicked() {
            app.menu = if app.menu == Some(Menu::NewChatProject) { None } else { Some(Menu::NewChatProject) };
        }
        if app.menu == Some(Menu::NewChatProject) {
            project_menu(app, ui, pick);
        }
    }
}

fn project_menu(app: &mut BuknoApp, ui: &mut Ui, anchor: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let mut options: Vec<(Option<bukno_core::ids::ProjectId>, String)> = vec![(None, "No project".into())];
    options.extend(app.projects.iter().filter(|p| p.available).map(|p| (Some(p.id), p.name.clone())));
    let rect = Rect::from_min_size(
        pos2(anchor.left(), anchor.bottom() + 4.0),
        vec2(260.0, options.len() as f32 * 32.0 + 12.0),
    );
    let mut chosen = None;
    egui::Area::new(Id::new("project-menu")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ui.ctx(), |ui| {
        theme.paint_shadow(ui.painter(), rect, theme.radius.radius_lg, &theme.shadow.shadow_popover);
        ui.painter().rect_filled(rect, theme.radius.radius_lg, c.surface_popover);
        for (i, (id, name)) in options.iter().enumerate() {
            let item = Rect::from_min_size(
                pos2(rect.left() + 6.0, rect.top() + 6.0 + i as f32 * 32.0),
                vec2(rect.width() - 12.0, 32.0),
            );
            let response = ui.interact(item, Id::new(("project-option", i)), Sense::click());
            let selected = *id == app.new_chat_project;
            response
                .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, name));
            if response.hovered() || selected {
                ui.painter().rect_filled(
                    item,
                    theme.radius.radius_md,
                    if selected { c.surface_selected } else { c.surface_hover },
                );
            }
            ui.painter().text(
                pos2(item.left() + 10.0, item.center().y),
                Align2::LEFT_CENTER,
                name,
                theme.font(&theme.text.t_ui),
                c.text_primary,
            );
            if response.clicked() {
                chosen = Some(*id);
            }
        }
    });
    if let Some(project) = chosen {
        app.new_chat_project = project;
        app.menu = None;
    } else if ui.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
        && !ui.ctx().pointer_interact_pos().is_some_and(|p| rect.contains(p) || anchor.contains(p))
    {
        app.menu = None;
    }
}

/// A docked notice above the composer, with up to three actions. Returns the
/// clicked action's index.
pub(crate) fn notice_card(
    ui: &mut Ui,
    theme: &crate::theme::Theme,
    rect: Rect,
    id: Id,
    text: &str,
    problem: bool,
    actions: &[&str],
) -> Option<usize> {
    let c = &theme.color;
    ui.painter().rect_filled(rect, theme.radius.radius_lg, c.surface_hover);
    let icon = if problem { Icon::Alert } else { Icon::Dots };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 20.0, rect.top() + 20.0), vec2(16.0, 16.0)),
        icon,
        14.0,
        c.text_secondary,
    );
    let mut job = theme.job(text, &theme.text.t_ui, c.text_primary, rect.width() - 52.0);
    job.wrap.max_rows = 3;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.left() + 38.0, rect.top() + 11.0), galley, c.text_primary);
    let mut clicked = None;
    let mut x = rect.left() + 38.0;
    for (i, label) in actions.iter().enumerate() {
        let (response, w) = super::setup::button(ui, theme, id.with(i), pos2(x, rect.bottom() - 42.0), label, i == 0);
        if response.clicked() {
            clicked = Some(i);
        }
        x += w + 8.0;
    }
    clicked
}

pub(crate) fn notice_height(ui: &Ui, theme: &crate::theme::Theme, width: f32, text: &str, actions: usize) -> f32 {
    let mut job = theme.job(text, &theme.text.t_ui, theme.color.text_primary, width - 52.0);
    job.wrap.max_rows = 3;
    let h = ui.painter().layout_job(job).size().y;
    h + 22.0 + if actions > 0 { 44.0 } else { 0.0 }
}

/// What to dock above the composer, most urgent first.
enum Dock {
    Decision(bukno_core::decision::DecisionView, usize),
    Notice { key: &'static str, text: String, problem: bool, actions: Vec<&'static str> },
}

fn docked(app: &BuknoApp) -> Vec<Dock> {
    let mut out = Vec::new();
    if let Some(first) = app.extra.decisions.first() {
        out.push(Dock::Decision(first.clone(), app.extra.decisions.len()));
    }
    if let Some(shared) = &app.extra.stop_slow {
        let others = if shared.is_empty() {
            String::new()
        } else {
            format!(" Force stop also ends the work in {}.", shared.join(", "))
        };
        out.push(Dock::Notice {
            key: "stop-slow",
            text: format!("Stopping is taking longer than expected. Codex has not confirmed the work stopped.{others}"),
            problem: true,
            actions: vec!["Force stop"],
        });
    }
    if let Some((_, reason)) = &app.extra.wait {
        let (text, actions) = match reason {
            WaitReason::Workspace { holder_title, .. } => (
                format!(
                    "Waiting: Codex is changing files in this folder in “{holder_title}”. This sends when that finishes."
                ),
                vec!["Run anyway", "Remove"],
            ),
            WaitReason::Slots { limit } => {
                (format!("Queued: {limit} chats are already working. This sends when one finishes."), vec!["Remove"])
            }
            WaitReason::Paused { why } => (format!("Not sent yet. {why}"), vec!["Send now", "Remove"]),
            WaitReason::Engine => ("Starting Codex…".to_owned(), vec![]),
            WaitReason::Checking => ("Checking whether this chat continued outside Bukno…".to_owned(), vec![]),
        };
        out.push(Dock::Notice { key: "wait", text, problem: false, actions });
    }
    if let Some(unknown) = &app.extra.unknown {
        out.push(Dock::Notice {
            key: "unknown",
            text: format!("Outcome unknown. {}", unknown.explanation),
            problem: true,
            actions: vec!["Send again", "Dismiss"],
        });
    }
    if let Some(banner) = &app.banner {
        out.push(Dock::Notice { key: "banner", text: banner.clone(), problem: false, actions: vec!["Dismiss"] });
    }
    out
}

fn chat(app: &mut BuknoApp, ui: &mut Ui, body: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let column = column_rect(body, &theme);

    // Header: title and, while a run is active, its state in words.
    let header_top = body.top() + 20.0;
    let painter = ui.painter();
    let mut title = theme.job(app.chat_title(), &theme.text.t_title, c.text_primary, column.width() - 160.0);
    title.wrap.max_rows = 1;
    title.wrap.overflow_character = Some('…');
    let title = painter.layout_job(title);
    let title_w = title.size().x;
    painter.galley(pos2(column.left(), header_top), title, c.text_primary);
    let now = ui.input(|i| i.time);
    if let Some(run) = &app.run {
        let chip_x = column.left() + title_w + 16.0;
        let center = pos2(chip_x + 7.0, header_top + 14.0);
        painter.circle_stroke(center, 5.5, egui::Stroke::new(1.5, c.surface_raised));
        let arc: Vec<_> = (0..=12)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 12.0 * std::f32::consts::PI;
                center + vec2(a.cos(), a.sin()) * 5.5
            })
            .collect();
        painter.add(egui::Shape::line(arc, egui::Stroke::new(1.5, provider_color(&theme, app.provider()))));
        let text = format!("{} · {}", state_words(run.state), orb::format_elapsed(now - run.started));
        painter.text(
            pos2(chip_x + 20.0, header_top + 14.0),
            Align2::LEFT_CENTER,
            text,
            theme.font(&theme.text.t_small),
            c.text_secondary,
        );
    }

    // Composer at the bottom, then docked cards, then the transcript fills the space between.
    // The transcript keeps at least 120 points; the composer scrolls inside beyond that.
    // A decision must stay answerable, so it is reserved first and may shrink the
    // transcript to a few lines, even in the smallest window.
    let docks = docked(app);
    let decision_h = match docks.first() {
        Some(Dock::Decision(d, _)) => Some(decision::height(ui, &theme, d, column.width(), decision::CODE_MIN)),
        _ => None,
    };
    let transcript_min = if decision_h.is_some() { DECISION_TRANSCRIPT_MIN } else { 120.0 };
    let composer_max =
        body.height() - 36.0 - transcript_min - theme.size.size_inset_bottom - decision_h.map_or(0.0, |h| h + 18.0);
    let composer_h = composer::height_for(ui, &theme, &app.composer, column.width(), composer_max);
    let composer_rect = Rect::from_min_size(
        pos2(column.left(), body.bottom() - theme.size.size_inset_bottom - composer_h),
        vec2(column.width(), composer_h),
    );
    // Cards leave room for the one-line note above the composer when it shows.
    let note_line = app.notice.is_some()
        || app.extra.draft_error.is_some()
        || (app.send_blocked().is_some() && !app.composer.text.trim().is_empty());
    let mut dock_bottom = composer_rect.top() - if note_line && !docks.is_empty() { 28.0 } else { 10.0 };
    let mut placed = Vec::new();
    for dock in docks {
        let (h, min, code_max) = match &dock {
            Dock::Decision(d, _) => {
                // Shorten the command box, which scrolls, before giving up space it needs.
                let room = dock_bottom - (header_top + 28.0 + DECISION_TRANSCRIPT_MIN);
                let full = decision::height(ui, &theme, d, column.width(), decision::CODE_MAX);
                let code_max = (decision::CODE_MAX - (full - room).max(0.0)).max(decision::CODE_MIN);
                let mut h = decision::height(ui, &theme, d, column.width(), code_max);
                if matches!(d.kind, DecisionKind::Question { .. }) {
                    // Questions scroll inside a card capped to the room left.
                    h = h.min(room.max(QUESTION_CARD_MIN));
                }
                (h, DECISION_TRANSCRIPT_MIN, code_max)
            }
            Dock::Notice { text, actions, .. } => {
                (notice_height(ui, &theme, column.width(), text, actions.len()), 120.0, 0.0)
            }
        };
        // Never push the transcript below its minimum, except for a decision:
        // it is always shown, because the run cannot go on without an answer.
        let decision = matches!(dock, Dock::Decision(..));
        if dock_bottom - h < header_top + 28.0 + min && !decision {
            break;
        }
        let rect = Rect::from_min_size(pos2(column.left(), dock_bottom - h), vec2(column.width(), h));
        dock_bottom = rect.top() - 8.0;
        placed.push((rect, dock, code_max));
    }
    let transcript_top = header_top + 28.0;
    let transcript_rect = Rect::from_min_max(
        pos2(body.left(), transcript_top),
        pos2(body.right(), (dock_bottom - 2.0).max(transcript_top)),
    );

    let trailing = if app.run.is_some() { WORKING_HEIGHT } else { 0.0 };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(transcript_rect));
    let output = app.transcript.show(&mut child, &app.doc, &theme, trailing);
    app.doc.frame_drawn();
    if let (Some(rect), Some(run)) = (output.trailing, &app.run) {
        let state = match run.state {
            RunState::WaitingForApproval | RunState::WaitingForInput => OrbState::Waiting,
            _ => OrbState::Thinking,
        };
        let label = state_words(run.state);
        // Activity words come only from the engine's own events.
        let summary = app
            .extra
            .activity
            .clone()
            .or_else(|| (app.is_synthetic() && run.has_output).then(|| "Writing a reply".to_owned()));
        orb::working_indicator(
            &mut child,
            &theme,
            rect,
            &Working {
                accent: provider_color(&theme, app.provider()),
                label,
                elapsed: Some(now - run.started),
                summary: summary.as_deref(),
                state,
                reduce_motion: app.reduce_motion,
                mark: app.working_mark,
            },
        );
    }

    let folder = app.selected_summary().map(|s| {
        std::path::Path::new(&s.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    });
    let protection = bukno_runtime::coordinator::codex_presets()
        .iter()
        .find(|p| Some(p.id.to_owned()) == app.preset())
        .map_or("", |p| p.protection);
    for (rect, dock, code_max) in placed {
        match dock {
            Dock::Decision(d, count) => {
                let provider = provider_name(app.provider());
                let mut answers = std::mem::take(&mut app.extra.answers);
                let action = decision::show(
                    ui,
                    &theme,
                    rect,
                    code_max,
                    &d,
                    provider,
                    folder.as_deref().unwrap_or(""),
                    protection,
                    &mut answers,
                );
                app.extra.answers = answers;
                if count > 1 {
                    ui.painter().text(
                        pos2(rect.right() - 16.0, rect.top() - 10.0),
                        Align2::RIGHT_CENTER,
                        format!("1 of {count} requests"),
                        theme.font(&theme.text.t_small),
                        c.text_tertiary,
                    );
                }
                match action {
                    Some(CardAction::Allow) => app.answer(&d, DecisionAnswer::Allow),
                    Some(CardAction::Decline) => app.answer(&d, DecisionAnswer::Decline),
                    Some(CardAction::Answers(answers)) => app.answer(&d, DecisionAnswer::Answers(answers)),
                    None => {}
                }
            }
            Dock::Notice { key, text, problem, actions } => {
                let clicked = notice_card(ui, &theme, rect, Id::new(("dock", key)), &text, problem, &actions);
                if let Some(i) = clicked {
                    dock_action(app, key, actions[i]);
                }
            }
        }
    }
    if let Some(notice) = app.notice.clone() {
        let color = if app.is_synthetic() || app.extra.notice_problem { c.negative } else { c.text_secondary };
        ui.painter().text(
            pos2(column.left(), composer_rect.top() - 20.0),
            Align2::LEFT_CENTER,
            notice,
            theme.font(&theme.text.t_small),
            color,
        );
    }
    if let Some(error) = app.extra.draft_error.clone() {
        ui.painter().text(
            pos2(column.right(), composer_rect.top() - 20.0),
            Align2::RIGHT_CENTER,
            format!("Draft not saved: {error}"),
            theme.font(&theme.text.t_small),
            c.negative,
        );
    }
    draw_composer(app, ui, composer_rect);
}

fn dock_action(app: &mut BuknoApp, key: &str, label: &str) {
    use bukno_runtime::UiCommand;
    let task = app.selected;
    match (key, label) {
        ("stop-slow", _) => {
            if let (Some(task), Some(run)) = (task, app.run.as_ref()) {
                app.send_command(UiCommand::ForceStop { task, run: run.run });
            }
        }
        ("wait", "Run anyway") => {
            if let Some((run, _)) = app.extra.wait.clone() {
                app.send_command(UiCommand::RunAnyway { run });
            }
        }
        ("wait", "Send now") => {
            if let Some((run, _)) = app.extra.wait.clone() {
                app.send_command(UiCommand::SendQueued { run });
            }
        }
        ("wait", "Remove") => {
            if let (Some(task), Some((run, _))) = (task, app.extra.wait.clone()) {
                app.send_command(UiCommand::Interrupt { task, run });
            }
        }
        ("unknown", "Send again") => {
            if let Some(unknown) = app.extra.unknown.clone() {
                let preset = app.preset();
                app.send_command(UiCommand::Resend { unknown: unknown.run, preset });
            }
        }
        ("unknown", "Dismiss") => {
            if let Some(unknown) = app.extra.unknown.take() {
                app.send_command(UiCommand::Dismiss { run: unknown.run });
            }
        }
        ("banner", _) => app.banner = None,
        _ => {}
    }
}

/// Closing with work active: keep working, or stop it and quit (section 16).
pub fn quit_dialog(app: &mut BuknoApp, ui: &mut Ui, full: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let flow = app.quit;
    egui::Area::new(Id::new("quit-dialog")).order(egui::Order::Foreground).fixed_pos(full.min).show(ui.ctx(), |ui| {
        ui.painter().rect_filled(full, 0.0, c.scrim);
        let card = Rect::from_center_size(full.center(), vec2(420.0, 150.0));
        theme.paint_shadow(ui.painter(), card, theme.radius.radius_lg, &theme.shadow.shadow_popover);
        ui.painter().rect_filled(card, theme.radius.radius_lg, c.surface_popover);
        let x = card.left() + 20.0;
        match flow {
            QuitFlow::Asking => {
                ui.painter().text(
                    pos2(x, card.top() + 28.0),
                    Align2::LEFT_CENTER,
                    "Codex is still working",
                    theme.font(&theme.text.t_heading),
                    c.text_primary,
                );
                ui.painter().text(
                    pos2(x, card.top() + 58.0),
                    Align2::LEFT_CENTER,
                    "Quitting stops that work. Your drafts are saved.",
                    theme.font(&theme.text.t_ui),
                    c.text_secondary,
                );
                let (stop, w) = super::setup::button(
                    ui,
                    &theme,
                    Id::new("quit-stop"),
                    pos2(x, card.bottom() - 52.0),
                    "Stop and quit",
                    true,
                );
                let (keep, _) = super::setup::button(
                    ui,
                    &theme,
                    Id::new("quit-keep"),
                    pos2(x + w + 8.0, card.bottom() - 52.0),
                    "Keep working",
                    false,
                );
                if stop.clicked() {
                    app.confirm_quit();
                } else if keep.clicked() {
                    app.quit = QuitFlow::None;
                }
            }
            _ => {
                ui.painter().text(
                    pos2(x, card.top() + 28.0),
                    Align2::LEFT_CENTER,
                    "Closing Bukno",
                    theme.font(&theme.text.t_heading),
                    c.text_primary,
                );
                ui.painter().text(
                    pos2(x, card.top() + 58.0),
                    Align2::LEFT_CENTER,
                    "Stopping work, closing Codex and saving…",
                    theme.font(&theme.text.t_ui),
                    c.text_secondary,
                );
            }
        }
    });
}

pub(crate) fn state_words(state: RunState) -> &'static str {
    match state {
        RunState::Preparing => "Sending",
        RunState::Starting => "Starting",
        RunState::Running => "Working",
        RunState::WaitingForApproval => "Needs approval",
        RunState::WaitingForInput => "Has a question",
        RunState::Cancelling => "Stopping",
        RunState::Completed => "Done",
        RunState::Failed => "Failed",
        RunState::Interrupted => "Stopped",
        RunState::OutcomeUnknown => "Outcome unknown",
    }
}
