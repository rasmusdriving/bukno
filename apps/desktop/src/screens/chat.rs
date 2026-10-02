//! The chat canvas: titlebar, chat header, transcript, working indicator
//! and composer. Also the empty new-chat screen.

use bukno_core::run::RunState;
use egui::{Align2, Id, Rect, Sense, Ui, ViewportCommand, pos2, vec2};

use crate::app::{BuknoApp, View};
use crate::components::composer::{self, ComposerAction, ComposerProps};
use crate::components::icons::{self, Icon};
use crate::components::orb::{self, OrbState, Working};
use crate::components::{icon_button, provider_color, provider_name};
use crate::transcript::column_rect;

/// Height reserved after the last message for the working indicator.
const WORKING_HEIGHT: f32 = 56.0;

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
        View::NewChat => {
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

pub fn show(app: &mut BuknoApp, ui: &mut Ui, canvas: Rect) {
    let body = Rect::from_min_max(pos2(canvas.left(), canvas.top() + app.theme.size.size_titlebar), canvas.max);
    match app.view {
        View::NewChat => new_chat(app, ui, body),
        View::Chat => chat(app, ui, body),
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

fn draw_composer(app: &mut BuknoApp, ui: &mut Ui, rect: Rect) {
    let (placeholder, running) = composer_props(app);
    let theme = app.theme.clone();
    let props = ComposerProps {
        provider: app.provider(),
        placeholder: &placeholder,
        running,
        model: "Synthetic model",
        effort: "Medium",
        effort_level: 3,
        permission: "Synthetic engine",
        send_blocked: app.send_blocked(),
    };
    match composer::show(ui, &theme, rect, &mut app.composer, &props) {
        Some(ComposerAction::Send) => app.submit(),
        Some(ComposerAction::Stop) => app.stop(),
        None => {}
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
    draw_composer(app, ui, rect);
    let meta_y = rect.bottom() + 36.0;
    let painter = ui.painter();
    icons::paint(
        painter,
        Rect::from_center_size(pos2(column.left() + 21.0, meta_y), vec2(14.0, 14.0)),
        Icon::Folder,
        14.0,
        c.text_secondary,
    );
    painter.text(
        pos2(column.left() + 34.0, meta_y),
        Align2::LEFT_CENTER,
        "No project",
        theme.font(&theme.text.t_ui),
        c.text_secondary,
    );
    painter.text(
        pos2(column.right() - 6.0, meta_y),
        Align2::RIGHT_CENTER,
        "Saved in its own workspace folder",
        theme.font(&theme.text.t_small),
        c.text_tertiary,
    );
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

    // Composer at the bottom, then the transcript fills the space between.
    // The transcript keeps at least 120 points; the composer scrolls inside beyond that.
    let composer_max = body.height() - 36.0 - 120.0 - theme.size.size_inset_bottom;
    let composer_h = composer::height_for(ui, &theme, &app.composer, column.width(), composer_max);
    let composer_rect = Rect::from_min_size(
        pos2(column.left(), body.bottom() - theme.size.size_inset_bottom - composer_h),
        vec2(column.width(), composer_h),
    );
    let transcript_rect =
        Rect::from_min_max(pos2(body.left(), header_top + 28.0), pos2(body.right(), composer_rect.top() - 12.0));

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
        let summary = if run.has_output { Some("Writing a reply") } else { None };
        orb::working_indicator(
            &mut child,
            &theme,
            rect,
            &Working {
                accent: provider_color(&theme, app.provider()),
                label,
                elapsed: Some(now - run.started),
                summary,
                state,
                reduce_motion: app.reduce_motion,
                mark: app.working_mark,
            },
        );
    }
    if let Some(notice) = app.notice.clone() {
        ui.painter().text(
            pos2(column.left(), composer_rect.top() - 20.0),
            Align2::LEFT_CENTER,
            notice,
            theme.font(&theme.text.t_small),
            c.negative,
        );
    }
    draw_composer(app, ui, composer_rect);
}

fn state_words(state: RunState) -> &'static str {
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
