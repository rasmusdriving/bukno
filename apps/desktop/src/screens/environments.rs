//! T3 servers: paired environments with their connection status and models,
//! and the Add environment form (address plus a pasted pairing link).

use bukno_t3_client::{ConnectionStatus, PairingStatus};
use egui::{Align2, Id, Rect, ScrollArea, TextEdit, Ui, pos2, vec2};

use crate::app::BuknoApp;
use crate::sources::status_words;
use crate::theme::Theme;

use super::setup::button;

const COLUMN: f32 = 600.0;

fn text_at(
    ui: &Ui,
    theme: &Theme,
    text: &str,
    style: &crate::theme::TypeStyle,
    color: egui::Color32,
    at: egui::Pos2,
) -> f32 {
    let galley = ui.painter().layout_job(theme.job(text, style, color, COLUMN));
    let h = galley.size().y;
    ui.painter().galley(at, galley, color);
    h
}

pub fn show(app: &mut BuknoApp, ui: &mut Ui, full: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let left = full.center().x - COLUMN / 2.0;
    let top = full.top() + theme.size.size_titlebar;
    let area = Rect::from_min_max(pos2(left - 24.0, top), pos2(left + COLUMN + 24.0, full.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    ScrollArea::vertical().id_salt("environments").auto_shrink([false, false]).show(&mut child, |ui| {
        ui.set_width(area.width());
        let origin = ui.cursor().min;
        let left = origin.x + 24.0;
        let mut y = origin.y + 24.0;
        let painter = ui.painter().clone();
        painter.text(pos2(left, y), Align2::LEFT_TOP, "T3 servers", theme.font(&theme.text.t_display), c.text_primary);
        y += 50.0;
        y += text_at(
            ui,
            &theme,
            "Bukno shows the projects and chats from a T3 server and works in them through T3: new chats, messages, approvals, questions, Stop, queue and steer. T3 runs the agents and keeps the chats; Bukno keeps only your drafts. It signs in to read and operate chats, nothing else.",
            &theme.text.t_body,
            c.text_secondary,
            pos2(left, y),
        ) + 24.0;

        y = super::setup::local_card(app, ui, &theme, pos2(left, y), COLUMN) + 24.0;

        let Some(t3) = app.t3.as_ref() else { return };
        let environments = t3.view.environments.clone();
        let pairing = t3.view.pairing.clone();
        let store_error = t3.view.store_error.clone();
        let mut reconnect = None;
        let mut forget = None;
        for env in &environments {
            let (words, problem) = status_words(&env.status);
            painter.text(pos2(left, y), Align2::LEFT_TOP, &env.saved.label, theme.font(&theme.text.t_title), c.text_primary);
            painter.text(
                pos2(left + COLUMN, y + 6.0),
                Align2::RIGHT_TOP,
                &words,
                theme.font(&theme.text.t_ui),
                if problem { c.attention } else { c.text_secondary },
            );
            y += 32.0;
            let version = env.server_version.clone().unwrap_or_else(|| env.saved.server_version.clone());
            let details = format!(
                "{} · T3 {version} · {} projects, {} chats · {} connection{}",
                env.saved.address.trim_end_matches('/'),
                env.projects.len(),
                env.threads.len(),
                env.connections,
                if env.connections == 1 { "" } else { "s" }
            );
            y += text_at(ui, &theme, &details, &theme.text.t_small, c.text_tertiary, pos2(left, y)) + 6.0;
            if let ConnectionStatus::NeedsPairing { reason }
            | ConnectionStatus::Blocked { reason }
            | ConnectionStatus::Reconnecting { reason, .. } = &env.status
            {
                y += text_at(ui, &theme, reason, &theme.text.t_small, c.negative, pos2(left, y)) + 6.0;
            }
            if !env.can_operate() {
                y += text_at(
                    ui,
                    &theme,
                    "Paired when Bukno could only read. Paste a new pairing link below to send, answer and stop from Bukno.",
                    &theme.text.t_small,
                    c.attention,
                    pos2(left, y),
                ) + 6.0;
            }
            for provider in env.providers.iter().filter(|p| p.enabled) {
                // Several variants can share one name; T3 tells them apart, so name the slug too.
                let names: Vec<String> = provider
                    .models
                    .iter()
                    .map(|m| {
                        let shared = provider.models.iter().filter(|o| o.name == m.name).count() > 1;
                        if shared { format!("{} ({})", m.name, m.slug) } else { m.name.clone() }
                    })
                    .collect();
                let line = format!(
                    "{} {}: {}",
                    provider.name(),
                    provider.version.as_deref().unwrap_or(""),
                    if names.is_empty() { "no models".to_owned() } else { names.join(", ") }
                );
                y += text_at(ui, &theme, &line, &theme.text.t_small, c.text_secondary, pos2(left, y)) + 4.0;
            }
            y += 8.0;
            let (again, w) =
                button(ui, &theme, Id::new(("env-reconnect", &env.saved.environment_id)), pos2(left, y), "Reconnect", false);
            if again.clicked() {
                reconnect = Some(env.saved.environment_id.clone());
            }
            let (remove, _) = button(
                ui,
                &theme,
                Id::new(("env-forget", &env.saved.environment_id)),
                pos2(left + w + 8.0, y),
                "Remove",
                false,
            );
            if remove.clicked() {
                forget = Some(env.saved.environment_id.clone());
            }
            y += 56.0;
        }

        let (advanced, _) = button(ui, &theme, Id::new("env-advanced"), pos2(left, y), "Advanced connection", false);
        y += 48.0;
        let Some(t3) = app.t3.as_mut() else { return };
        if advanced.clicked() { t3.advanced_setup = !t3.advanced_setup; }
        let mut connect = false;
        if t3.advanced_setup {
            painter.text(pos2(left, y), Align2::LEFT_TOP, "Connect another server", theme.font(&theme.text.t_title), c.text_primary);
            y += 34.0;
            y += text_at(
                ui,
                &theme,
                "For a server on another computer, copy its pairing link from T3 Code settings. Paste it below. The address is optional when the link already contains it.",
                &theme.text.t_small,
                c.text_secondary,
                pos2(left, y),
            ) + 12.0;
            let field = |ui: &mut Ui, y: f32, label: &str, id: &str, value: &mut String, password: bool, hint: &str| {
                ui.painter().text(pos2(left, y), Align2::LEFT_TOP, label, theme.font(&theme.text.t_small), c.text_secondary);
                let rect = Rect::from_min_size(pos2(left, y + 20.0), vec2(COLUMN, 32.0));
                ui.put(
                    rect,
                    TextEdit::singleline(value)
                        .id(Id::new(id))
                        .password(password)
                        .hint_text(hint)
                        .font(theme.font(&theme.text.t_ui))
                        .margin(vec2(10.0, 8.0)),
                );
                y + 64.0
            };

            y = field(ui, y, "Address", "env-address", &mut t3.form.address, false, "Optional server address");
            y = field(ui, y, "Pairing link", "env-link", &mut t3.form.link, true, "Paste the link from T3");
            let busy = pairing == PairingStatus::Working;
            let ready = !t3.form.link.trim().is_empty() && !busy;

            ui.add_enabled_ui(ready, |ui| {
                let (go, _) = button(ui, &theme, Id::new("env-connect"), pos2(left, y), "Connect", true);
                connect = go.clicked();
            });
            y += 44.0;
            let (message, problem) = match &pairing {
                PairingStatus::Idle => (None, false),
                PairingStatus::Working => (Some("Checking the server and pairing…".to_owned()), false),
                PairingStatus::Failed(why) => (Some(why.clone()), true),
                PairingStatus::Paired { label, .. } => (Some(format!("Paired with {label}. Its chats are in the sidebar.")), false),
            };
            if let Some(message) = message {
                y += text_at(ui, &theme, &message, &theme.text.t_ui, if problem { c.negative } else { c.text_secondary }, pos2(left, y));
            }
        }
        let (done, _) = button(ui, &theme, Id::new("env-done"), pos2(left, y), "Done", false);
        y += 44.0;
        if let Some(error) = store_error {
            y += 8.0;
            y += text_at(ui, &theme, &error, &theme.text.t_small, c.negative, pos2(left, y));
        }
        ui.allocate_space(vec2(area.width(), (y - origin.y + 40.0).max(0.0)));

        if connect {
            t3.pair();
        }
        if let Some(id) = reconnect {
            t3.reconnect(&id);
        }
        if let Some(id) = forget {
            t3.forget(&id);
            if app.remote.as_ref().is_some_and(|r| r.environment == id) {
                app.new_chat(None);
            }
        }
        if done.clicked() {
            if let Some(t3) = app.t3.as_ref() {
                t3.clear_pairing_status();
            }
            app.show_environments = false;
        }
    });
}
