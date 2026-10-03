//! The left sidebar: New chat, Search, Projects, Chats, usage and profile.
//!
//! In synthetic scenario mode the project and chat names are fixed sample
//! rows; only the scenario's own chat has content. Otherwise the rows are
//! the user's projects and chats as the coordinator publishes them.

use bukno_core::event::{ChatActivity, ChatSummary};
use bukno_core::message::Provider;
use egui::{Id, Rect, Ui, pos2, vec2};

use crate::app::{BuknoApp, View};
use crate::components::icons::Icon;
use crate::components::rows::{self, Row, Trailing, Usage};
use crate::components::{command_key, composer::composer_id, icon_button};

/// Below this conversation width the sidebar collapses (the right panel,
/// when it exists, collapses first).
pub const MIN_CONVERSATION: f32 = 560.0;

pub fn sidebar_rect(app: &BuknoApp, full: Rect) -> Option<Rect> {
    let width = app.theme.size.size_sidebar;
    (app.sidebar_open && full.width() - width >= MIN_CONVERSATION)
        .then(|| Rect::from_min_size(full.min, vec2(width, full.height())))
}

pub fn sidebar(app: &mut BuknoApp, ui: &mut Ui, rect: Rect) {
    if app.is_synthetic() {
        sidebar_synthetic(app, ui, rect);
    } else {
        sidebar_real(app, ui, rect);
    }
}

/// The fixed sample rows of synthetic scenario mode.
fn sidebar_synthetic(app: &mut BuknoApp, ui: &mut Ui, rect: Rect) {
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

    // Usage and profile are pinned to the bottom; the lists scroll in the
    // space above them, so short windows never overlap rows and footer.
    let bottom = rect.bottom() - theme.size.size_inset_bottom;
    let profile = Rect::from_min_max(pos2(left, bottom - 40.0), pos2(rect.right() - inset, bottom));
    let meters_top = profile.top() - 7.0 - 2.0 * 35.0;
    let list = Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), (meters_top - 8.0).max(y)));
    app.sidebar_list = Some(list);
    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
    list_ui.set_clip_rect(list);
    egui::ScrollArea::vertical().id_salt("nav-list").auto_shrink([false, false]).show(&mut list_ui, |ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        // Reserve the next slot of the list and return it, aligned to the rows.
        let next = |ui: &mut Ui, height: f32| {
            let (slot, _) = ui.allocate_exact_size(vec2(list.width(), height), egui::Sense::hover());
            Rect::from_min_size(pos2(left, slot.top()), vec2(width, height))
        };
        let label = next(ui, 27.0);
        rows::section_label(ui, &theme, Rect::from_min_size(label.min, vec2(width, 28.0)), "Projects");
        let projects = [("Bukno", 0usize), ("Fieldnotes", 1), ("Studio", 2)];
        for (name, i) in projects {
            let id = Id::new(("nav-project", i));
            let row = next(ui, row_h + 2.0);
            if rows::project_row(
                ui,
                &theme,
                Rect::from_min_size(row.min, vec2(width, row_h)),
                id,
                name,
                app.projects_open[i],
            )
            .clicked()
            {
                app.projects_open[i] = !app.projects_open[i];
            }
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
                    let row = next(ui, row_h + 2.0);
                    let response = rows::row(
                        ui,
                        &theme,
                        Rect::from_min_size(row.min, vec2(width, row_h)),
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
                }
            }
        }
        next(ui, theme.space.space_6 - 10.0);
        let label = next(ui, 27.0);
        rows::section_label(ui, &theme, Rect::from_min_size(label.min, vec2(width, 28.0)), "Chats");
        for (j, (title, provider)) in
            [("A quick idea", Provider::Claude), ("Packing list for Lisbon", Provider::Codex)].into_iter().enumerate()
        {
            let row = next(ui, row_h + 2.0);
            let response = rows::row(
                ui,
                &theme,
                Rect::from_min_size(row.min, vec2(width, row_h)),
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
        }
    });

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

fn chat_trailing(chat: &ChatSummary, selected: bool) -> Trailing<'static> {
    match chat.activity {
        ChatActivity::Working | ChatActivity::Queued => Trailing::Working(chat.provider),
        ChatActivity::NeedsYou | ChatActivity::Unknown => Trailing::Alert,
        ChatActivity::Idle => {
            let _ = selected;
            Trailing::Mark(chat.provider)
        }
    }
}

fn activity_words(chat: &ChatSummary) -> &'static str {
    match chat.activity {
        ChatActivity::Idle => "",
        ChatActivity::Queued => ", queued",
        ChatActivity::Working => ", working",
        ChatActivity::NeedsYou => ", needs you",
        ChatActivity::Unknown => ", outcome unknown",
    }
}

/// The user's projects and chats.
fn sidebar_real(app: &mut BuknoApp, ui: &mut Ui, rect: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    ui.painter().rect_filled(rect, 0.0, c.surface_sidebar);
    let row_h = theme.size.size_row;
    let inset = 8.0;
    let left = rect.left() + inset;
    let width = rect.width() - 2.0 * inset;
    let row_rect = |y: f32| Rect::from_min_size(pos2(left, y), vec2(width, row_h));
    let cmd = command_key();

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
        app.new_chat(None);
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

    let bottom = rect.bottom() - theme.size.size_inset_bottom;
    let profile = Rect::from_min_max(pos2(left, bottom - 40.0), pos2(rect.right() - inset, bottom));
    let meters_top = profile.top() - 7.0 - 2.0 * 35.0;
    let list = Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), (meters_top - 8.0).max(y)));
    app.sidebar_list = Some(list);
    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
    list_ui.set_clip_rect(list);
    let selected_task = (app.view == View::Chat).then_some(app.selected).flatten();
    let projects = app.projects.clone();
    let chats = app.chats.clone();
    let mut select = None;
    let mut new_in = None;
    let mut add_project = false;
    egui::ScrollArea::vertical().id_salt("nav-list").auto_shrink([false, false]).show(&mut list_ui, |ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let next = |ui: &mut Ui, height: f32| {
            let (slot, _) = ui.allocate_exact_size(vec2(list.width(), height), egui::Sense::hover());
            Rect::from_min_size(pos2(left, slot.top()), vec2(width, height))
        };
        let label = next(ui, 27.0);
        rows::section_label(ui, &theme, Rect::from_min_size(label.min, vec2(width, 28.0)), "Projects");
        let add = Rect::from_center_size(pos2(left + width - 12.0, label.center().y), vec2(24.0, 24.0));
        if icon_button(ui, &theme, Id::new("nav-add-project"), add, Icon::Plus, "Add a project").clicked() {
            add_project = true;
        }
        if projects.is_empty() {
            let hint = next(ui, row_h);
            ui.painter().text(
                pos2(hint.left() + 12.0, hint.center().y),
                egui::Align2::LEFT_CENTER,
                "Add a folder to work in",
                theme.font(&theme.text.t_small),
                c.text_tertiary,
            );
        }
        for project in &projects {
            let open = app.open_projects.get(&project.id).copied().unwrap_or(true);
            let row = next(ui, row_h + 2.0);
            let header = Rect::from_min_size(row.min, vec2(width, row_h));
            let name = if project.available { project.name.clone() } else { format!("{} (unavailable)", project.name) };
            if rows::project_row(ui, &theme, header, Id::new(("nav-project", project.id)), &name, open).clicked() {
                app.open_projects.insert(project.id, !open);
            }
            if header.contains(ui.ctx().pointer_hover_pos().unwrap_or_default()) || !open {
                let plus = Rect::from_center_size(pos2(header.right() - 14.0, header.center().y), vec2(24.0, 24.0));
                let label = format!("New chat in {}", project.name);
                if icon_button(ui, &theme, Id::new(("nav-project-new", project.id)), plus, Icon::Plus, &label).clicked()
                {
                    new_in = Some(project.id);
                }
            }
            if !open {
                continue;
            }
            for chat in chats.iter().filter(|c| c.project == Some(project.id)) {
                let row = next(ui, row_h + 2.0);
                let selected = selected_task == Some(chat.task);
                let response = rows::row(
                    ui,
                    &theme,
                    Rect::from_min_size(row.min, vec2(width, row_h)),
                    Row {
                        id: Id::new(("nav-chat", chat.task)),
                        title: &chat.title,
                        lead: None,
                        trailing: chat_trailing(chat, selected),
                        selected,
                        child: true,
                        label: Some(format!(
                            "{}, {} chat{}",
                            chat.title,
                            crate::components::provider_name(chat.provider),
                            activity_words(chat)
                        )),
                    },
                );
                if response.clicked() {
                    select = Some(chat.task);
                }
            }
        }
        next(ui, theme.space.space_6 - 10.0);
        let label = next(ui, 27.0);
        rows::section_label(ui, &theme, Rect::from_min_size(label.min, vec2(width, 28.0)), "Chats");
        for chat in chats.iter().filter(|c| c.project.is_none()) {
            let row = next(ui, row_h + 2.0);
            let selected = selected_task == Some(chat.task);
            let response = rows::row(
                ui,
                &theme,
                Rect::from_min_size(row.min, vec2(width, row_h)),
                Row {
                    id: Id::new(("nav-chat", chat.task)),
                    title: &chat.title,
                    lead: None,
                    trailing: chat_trailing(chat, selected),
                    selected,
                    child: false,
                    label: Some(format!(
                        "{}, {} chat{}",
                        chat.title,
                        crate::components::provider_name(chat.provider),
                        activity_words(chat)
                    )),
                },
            );
            if response.clicked() {
                select = Some(chat.task);
            }
        }
    });
    if let Some(task) = select {
        app.select_chat(task);
    }
    if let Some(project) = new_in {
        app.new_chat(Some(project));
        ui.ctx().memory_mut(|m| m.request_focus(composer_id()));
    }
    if add_project && let Some(folder) = crate::screens::setup::pick_folder("Add a project folder") {
        app.send_command(bukno_runtime::UiCommand::AddProject { path: folder });
    }

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
    if rows::profile_row(ui, &theme, profile, Id::new("nav-profile"), "B", "Engines and folders").clicked() {
        app.show_setup = true;
    }
}
