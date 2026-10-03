//! A chat that lives on a T3 server, read only: the transcript as T3 has it,
//! live while it runs there, with older history loaded on request. There is
//! no composer; replies, approvals and Stop happen in T3 in this version.

use bukno_t3_client::ConnectionStatus;
use egui::{Align2, Id, Rect, Ui, pos2, vec2};

use crate::app::BuknoApp;
use crate::components::orb::{self, OrbState, Working};
use crate::components::{flat_text_button, provider_color};
use crate::sources::{provider_of, status_words};
use crate::transcript::column_rect;

use super::chat::{WORKING_HEIGHT, notice_card, notice_height};

/// What the docked note under the transcript says, and whether it is a problem.
fn note(app: &BuknoApp) -> (String, bool) {
    let Some(t3) = app.t3.as_ref() else { return ("T3 is not available in this mode.".into(), true) };
    let Some(chat) = app.remote.as_ref() else { return (String::new(), false) };
    let env = t3.environment(&chat.environment);
    let label = env.map_or("T3", |e| e.saved.label.as_str());
    let thread = t3.open_thread();
    if let Some(error) = thread.and_then(|t| t.error.clone()) {
        return (error, true);
    }
    if thread.is_some_and(|t| t.removed) {
        return (format!("This chat was deleted in T3 on {label}. What was loaded stays here until you leave."), true);
    }
    match env.map(|e| &e.status) {
        Some(ConnectionStatus::NeedsPairing { reason } | ConnectionStatus::Blocked { reason }) => {
            return (format!("{reason} Showing what was last received."), true);
        }
        Some(ConnectionStatus::Reconnecting { reason, .. }) => {
            return (format!("{reason} Reconnecting; showing what was last received."), true);
        }
        _ => {}
    }
    if t3.open_summary().is_some_and(|s| s.pending_runtime_request.is_some()) {
        return (format!("T3 is waiting for an answer in this chat. Answer it in T3 on {label}."), false);
    }
    (
        format!(
            "Read only. This chat lives in T3 on {label}. Reply, approve or stop it in T3; Bukno can send in its next version."
        ),
        false,
    )
}

pub fn chat(app: &mut BuknoApp, ui: &mut Ui, body: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let column = column_rect(body, &theme);
    let header_top = body.top() + 20.0;

    let (provider, working, loaded, current, has_more, loading_history, history_error, status) = {
        let t3 = app.t3.as_ref();
        let summary = t3.and_then(|t| t.open_summary());
        let thread = t3.and_then(|t| t.open_thread());
        let env = t3.and_then(|t| app.remote.as_ref().and_then(|r| t.environment(&r.environment)));
        (
            provider_of(summary.map_or("", |s| s.model_selection.instance())),
            summary.is_some_and(|s| s.status != "idle" && s.activity_run_status.is_some())
                || thread.is_some_and(|t| t.working),
            thread.is_some_and(|t| t.loaded),
            thread.is_some_and(|t| t.current),
            thread.is_some_and(|t| t.has_more_history),
            thread.is_some_and(|t| t.loading_history),
            thread.and_then(|t| t.history_error.clone()),
            env.map(|e| e.status.clone()),
        )
    };

    // Header: title, then where the chat lives.
    let painter = ui.painter();
    let mut title = theme.job(app.chat_title(), &theme.text.t_title, c.text_primary, column.width() - 220.0);
    title.wrap.max_rows = 1;
    title.wrap.overflow_character = Some('…');
    let title = painter.layout_job(title);
    let title_w = title.size().x;
    painter.galley(pos2(column.left(), header_top), title, c.text_primary);
    let state = if !loaded {
        "Loading…".to_owned()
    } else if !current {
        status.as_ref().map_or_else(|| "Catching up…".to_owned(), |s| status_words(s).0)
    } else if working {
        "Working in T3".to_owned()
    } else {
        "Read only".to_owned()
    };
    painter.text(
        pos2(column.left() + title_w + 16.0, header_top + 14.0),
        Align2::LEFT_CENTER,
        state,
        theme.font(&theme.text.t_small),
        c.text_tertiary,
    );
    if has_more || loading_history || history_error.is_some() {
        let text = if loading_history { "Loading older messages…" } else { "Load older messages" };
        let response = ui
            .add_enabled_ui(!loading_history, |ui| {
                flat_text_button(
                    ui,
                    &theme,
                    Id::new("remote-load-older"),
                    pos2(column.right(), header_top - 2.0),
                    text,
                    None,
                )
            })
            .inner;
        if response.clicked()
            && let Some(t3) = app.t3.as_ref()
        {
            t3.load_older();
        }
    }

    // The note docked at the bottom, where the composer would be.
    let (text, problem) = note(app);
    let note_h = notice_height(ui, &theme, column.width(), &text, 0);
    let note_rect = Rect::from_min_size(
        pos2(column.left(), body.bottom() - theme.size.size_inset_bottom - note_h),
        vec2(column.width(), note_h),
    );
    let mut transcript_bottom = note_rect.top() - 10.0;
    if let Some(error) = history_error {
        ui.painter().text(
            pos2(column.left(), transcript_bottom - 10.0),
            Align2::LEFT_CENTER,
            error,
            theme.font(&theme.text.t_small),
            c.negative,
        );
        transcript_bottom -= 24.0;
    }

    let transcript_top = header_top + 28.0;
    let transcript_rect = Rect::from_min_max(
        pos2(body.left(), transcript_top),
        pos2(body.right(), transcript_bottom.max(transcript_top)),
    );
    if !loaded {
        ui.painter().text(
            transcript_rect.center(),
            Align2::CENTER_CENTER,
            "Loading the chat from T3…",
            theme.font(&theme.text.t_ui),
            c.text_tertiary,
        );
    } else {
        let trailing = if working { WORKING_HEIGHT } else { 0.0 };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(transcript_rect));
        let output = app.transcript.show(&mut child, &app.doc, &theme, trailing);
        app.doc.frame_drawn();
        if let (Some(rect), true) = (output.trailing, working) {
            orb::working_indicator(
                &mut child,
                &theme,
                rect,
                &Working {
                    accent: provider_color(&theme, provider),
                    label: "Working in T3",
                    elapsed: None,
                    summary: None,
                    state: OrbState::Thinking,
                    reduce_motion: app.reduce_motion,
                    mark: app.working_mark,
                },
            );
        }
    }
    notice_card(ui, &theme, note_rect, Id::new("remote-note"), &text, problem, &[]);
}
