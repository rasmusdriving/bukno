//! The left sidebar: New chat, Search, Projects, Chats, usage and profile.
//!
//! In synthetic scenario mode the project and chat names are fixed sample
//! rows; only the scenario's own chat has content.

use bukno_core::message::Provider;
use egui::{Id, Rect, Ui, pos2, vec2};

use crate::app::{BuknoApp, View};
use crate::components::icons::Icon;
use crate::components::rows::{self, Row, Trailing, Usage};
use crate::components::{command_key, composer::composer_id};

/// Below this conversation width the sidebar collapses (the right panel,
/// when it exists, collapses first).
pub const MIN_CONVERSATION: f32 = 560.0;

pub fn sidebar_rect(app: &BuknoApp, full: Rect) -> Option<Rect> {
    let width = app.theme.size.size_sidebar;
    (app.sidebar_open && full.width() - width >= MIN_CONVERSATION)
        .then(|| Rect::from_min_size(full.min, vec2(width, full.height())))
}

pub fn sidebar(app: &mut BuknoApp, ui: &mut Ui, rect: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    ui.painter().rect_filled(rect, 0.0, c.surface_sidebar);
    let row_h = theme.size.size_row;
    let inset = 8.0;
    let left = rect.left() + inset;
    let width = rect.width() - 2.0 * inset;
    let row_rect = |y: f32| Rect::from_min_size(pos2(left, y), vec2(width, row_h));
    let cmd = command_key();

    // Below the 44-point titlebar.
    let mut y = rect.top() + theme.size.size_titlebar + 4.0;
    let new_chat = rows::row(
        ui,
        &theme,
        row_rect(y),
        Row {
            id: Id::new("nav-new-chat"),
            title: "New chat",
            lead: Some(Icon::NewChat),
            trailing: Trailing::Keys(&[cmd, "N"]),
            selected: app.view == View::NewChat,
            child: false,
            label: Some("New chat".into()),
        },
    );
    if new_chat.clicked() {
        app.view = View::NewChat;
        ui.ctx().memory_mut(|m| m.request_focus(composer_id()));
    }
    y += row_h + 2.0;
    ui.add_enabled_ui(false, |ui| {
        rows::row(
            ui,
            &theme,
            row_rect(y),
            Row {
                id: Id::new("nav-search"),
                title: "Search",
                lead: Some(Icon::Search),
                trailing: Trailing::Keys(&[cmd, "K"]),
                selected: false,
                child: false,
                label: Some("Search chats, available in a later pass".into()),
            },
        );
    });
    y += row_h + theme.space.space_6 - 8.0;

    rows::section_label(ui, &theme, Rect::from_min_size(pos2(left, y), vec2(width, 28.0)), "Projects");
    y += 27.0;
    let projects = [("Bukno", 0usize), ("Fieldnotes", 1), ("Studio", 2)];
    for (name, i) in projects {
        let id = Id::new(("nav-project", i));
        if rows::project_row(ui, &theme, row_rect(y), id, name, app.projects_open[i]).clicked() {
            app.projects_open[i] = !app.projects_open[i];
        }
        y += row_h + 2.0;
        if i == 0 && app.projects_open[0] {
            let working = app.run.is_some() && app.view != View::Chat;
            let chats: [(&str, Provider, bool); 3] = [
                (app.scenario.title, Provider::Codex, true),
                ("Windows startup", Provider::Claude, false),
                ("Provider setup", Provider::Codex, false),
            ];
            for (j, (title, provider, is_scenario)) in chats.into_iter().enumerate() {
                let selected = is_scenario && app.view == View::Chat;
                let trailing = if is_scenario && (working || (app.run.is_some() && selected)) {
                    Trailing::Working(provider)
                } else {
                    Trailing::Mark(provider)
                };
                let response = rows::row(
                    ui,
                    &theme,
                    row_rect(y),
                    Row {
                        id: Id::new(("nav-chat", j)),
                        title,
                        lead: None,
                        trailing,
                        selected,
                        child: true,
                        label: Some(format!("{title}, {} chat", crate::components::provider_name(provider))),
                    },
                );
                if response.clicked() {
                    app.view = if is_scenario { View::Chat } else { View::NewChat };
                }
                y += row_h + 2.0;
            }
        }
    }
    y += theme.space.space_6 - 10.0;
    rows::section_label(ui, &theme, Rect::from_min_size(pos2(left, y), vec2(width, 28.0)), "Chats");
    y += 27.0;
    for (j, (title, provider)) in
        [("A quick idea", Provider::Claude), ("Packing list for Lisbon", Provider::Codex)].into_iter().enumerate()
    {
        let response = rows::row(
            ui,
            &theme,
            row_rect(y),
            Row {
                id: Id::new(("nav-loose-chat", j)),
                title,
                lead: None,
                trailing: Trailing::Mark(provider),
                selected: false,
                child: false,
                label: Some(format!("{title}, {} chat", crate::components::provider_name(provider))),
            },
        );
        if response.clicked() {
            app.view = View::NewChat;
        }
        y += row_h + 2.0;
    }

    // Usage and profile, pinned to the bottom.
    let bottom = rect.bottom() - theme.size.size_inset_bottom;
    let profile = Rect::from_min_max(pos2(left, bottom - 40.0), pos2(rect.right() - inset, bottom));
    let meters_left = rect.left() + 20.0;
    let meters_w = rect.width() - 40.0;
    for (k, provider) in [Provider::Codex, Provider::Claude].into_iter().enumerate() {
        let top = profile.top() - 7.0 - (2 - k) as f32 * 35.0;
        rows::usage_meter(
            ui,
            &theme,
            Rect::from_min_size(pos2(meters_left, top), vec2(meters_w, 24.0)),
            provider,
            "5h",
            Usage::Unavailable,
        );
    }
    rows::profile_row(ui, &theme, profile, Id::new("nav-profile"), "S", "Synthetic scenario");
}
