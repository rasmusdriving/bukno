//! A chat that lives on a T3 server: the transcript as T3 has it, live while
//! it runs, and a composer that sends through T3. Approvals and questions,
//! queued messages and commands T3 has not confirmed dock above the composer,
//! as they do in a local chat. Also the new-chat screen for a T3 project.

use bukno_core::decision::{DecisionKind, DecisionState, DecisionView, Question};
use bukno_core::ids::{DecisionId, RunId, TaskId};
use bukno_core::run::RunState;
use bukno_t3_client::ConnectionStatus;
use bukno_t3_client::command::{Delivery, ModelChoice, Outgoing, RUNTIME_MODES, runtime_mode_label};
use bukno_t3_client::hub::{OutboxEntry, OutboxState};
use bukno_t3_client::model::Question as T3Question;
use bukno_t3_client::thread::{PendingKind, PendingRequest};
use egui::{Align2, Id, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Map, Value};

use crate::app::{BuknoApp, Menu};
use crate::components::composer::{self, ComposerAction, ComposerProps};
use crate::components::decision::{self, CardAction};
use crate::components::icons::{self, Icon};
use crate::components::orb::{self, OrbState, Working};
use crate::components::{flat_text_button, provider_color, provider_name};
use crate::sources::{provider_of, stable_id, status_words, unwrap_shell};
use crate::transcript::column_rect;

use super::chat::{
    DECISION_TRANSCRIPT_MIN, QUESTION_CARD_MIN, WORKING_HEIGHT, notice_card, notice_height, state_words,
};

/// What docks above the composer, most urgent first.
enum Dock {
    Request(PendingRequest, usize),
    Notice { key: String, text: String, problem: bool, actions: Vec<&'static str> },
}

/// A message's text, shortened for a one-line notice.
fn quote(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut out: String = line.chars().take(70).collect();
    if line.chars().count() > 70 || text.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        out.push('…');
    }
    format!("“{out}”")
}

fn docks(app: &BuknoApp) -> Vec<Dock> {
    let mut out = Vec::new();
    let (Some(t3), Some(chat)) = (app.t3.as_ref(), app.remote.as_ref()) else { return out };
    let Some(env) = t3.environment(&chat.environment) else { return out };
    let thread = t3.open_thread();
    let provider = provider_name(provider_of(t3.open_summary().map_or("", |s| s.model_selection.instance())));

    if let Some(thread) = thread
        && let Some(first) = thread.pending.first()
    {
        if first.capability == "not_resumable" {
            out.push(Dock::Notice {
                key: format!("stale:{}", first.request_id),
                text: format!(
                    "{provider} asked for an answer, but the session that asked has ended in T3, so it cannot be answered any more."
                ),
                problem: true,
                actions: vec![],
            });
        } else {
            out.push(Dock::Request(first.clone(), thread.pending.len()));
        }
    }

    if !env.can_operate() {
        out.push(Dock::Notice {
            key: "pair".into(),
            text:
                "This server was paired when Bukno could only read. Pair it again to send, answer and stop from Bukno."
                    .into(),
            problem: true,
            actions: vec!["Pair again"],
        });
    }

    for entry in env.outbox.iter().filter(|e| e.request.thread_id() == chat.thread) {
        let mine = app.remote_pending.iter().any(|p| p.command_id == entry.request.command_id);
        match &entry.state {
            OutboxState::Unconfirmed => out.push(Dock::Notice {
                key: format!("unconfirmed:{}", entry.request.command_id),
                text: format!(
                    "Not confirmed: {} may not have reached T3, because the connection dropped before it answered. Sending again is safe; T3 runs it at most once.",
                    entry.request.describe().to_lowercase()
                ),
                problem: true,
                actions: vec!["Send again", "Dismiss"],
            }),
            // The composer's own refusals show under the composer instead.
            OutboxState::Rejected(reason) if !mine => out.push(Dock::Notice {
                key: format!("rejected:{}", entry.request.command_id),
                text: format!("{} was not done. {reason}", entry.request.describe()),
                problem: true,
                actions: vec!["Dismiss"],
            }),
            _ => {}
        }
    }

    if let Some(thread) = thread {
        let held: Vec<_> = thread.queued.iter().filter(|q| q.held).collect();
        if !held.is_empty() {
            let text = match held.len() {
                1 => format!("Waiting after Stop: {}. Resume sends it.", quote(&held[0].text)),
                n => {
                    format!("{n} messages wait after Stop, starting with {}. Resume sends them.", quote(&held[0].text))
                }
            };
            out.push(Dock::Notice { key: "held".into(), text, problem: false, actions: vec!["Resume", "Remove"] });
        }
        for queued in thread.queued.iter().filter(|q| !q.held).take(3) {
            let actions = if thread.active_run.is_some() { vec!["Steer now", "Remove"] } else { vec!["Remove"] };
            out.push(Dock::Notice {
                key: format!("queued:{}", queued.run_id),
                text: format!("Queued for the next turn: {}", quote(&queued.text)),
                problem: false,
                actions,
            });
        }
    }

    match &env.status {
        ConnectionStatus::NeedsPairing { reason } | ConnectionStatus::Blocked { reason } => out.push(Dock::Notice {
            key: "connection".into(),
            text: format!("{reason} Showing what was last received."),
            problem: true,
            actions: vec![],
        }),
        ConnectionStatus::Reconnecting { reason, .. } => out.push(Dock::Notice {
            key: "connection".into(),
            text: format!("{reason} Reconnecting; showing what was last received."),
            problem: true,
            actions: vec![],
        }),
        _ => {}
    }
    if thread.is_some_and(|t| t.removed) {
        out.push(Dock::Notice {
            key: "removed".into(),
            text: format!(
                "This chat was deleted in T3 on {}. What was loaded stays here until you leave.",
                env.saved.label
            ),
            problem: true,
            actions: vec![],
        });
    }
    if let Some(error) = thread.and_then(|t| t.error.clone()) {
        out.push(Dock::Notice { key: "error".into(), text: error, problem: true, actions: vec![] });
    }
    out
}

/// Questions as the shared card shows them. Options are shown by label;
/// [`answers_for`] maps a chosen label back to the value T3 expects.
fn card_questions(questions: &[T3Question]) -> Vec<Question> {
    questions
        .iter()
        .map(|q| Question {
            id: q.id.clone(),
            header: q.header.clone(),
            text: q.question.clone(),
            options: q.options.iter().map(|o| o.label.clone()).collect(),
            other: q.options.is_empty() || q.allow_custom_answer != Some(false),
        })
        .collect()
}

/// The answers T3 expects: the option's value (its label when it has none),
/// a list for multi-select questions, or free text.
fn answers_for(questions: &[T3Question], given: &[(String, Vec<String>)]) -> Map<String, Value> {
    let mut out = Map::new();
    for q in questions {
        let chosen = given.iter().find(|(id, _)| *id == q.id).and_then(|(_, a)| a.first()).cloned().unwrap_or_default();
        let value = q.options.iter().find(|o| o.label == chosen).map_or(chosen.clone(), |o| o.answer().to_owned());
        let answer = if q.multi_select == Some(true) {
            Value::Array(if value.is_empty() { vec![] } else { vec![Value::String(value)] })
        } else {
            Value::String(value)
        };
        out.insert(q.id.clone(), answer);
    }
    out
}

fn decision_view(request: &PendingRequest, task: TaskId, sending: bool) -> DecisionView {
    let kind = match &request.kind {
        PendingKind::Question { questions, .. } => DecisionKind::Question { questions: card_questions(questions) },
        PendingKind::Approval { request_kind, prompt } => match request_kind.as_str() {
            "command" => DecisionKind::Command {
                command: prompt.as_deref().map_or_else(|| "A command (T3 did not say which)".into(), unwrap_shell),
                cwd: None,
                reason: None,
            },
            "file-change" => DecisionKind::FileChange { files: prompt.iter().cloned().collect(), reason: None },
            "file-read" => DecisionKind::Access { what: "read files".into(), detail: prompt.clone() },
            "mcp-elicitation" => DecisionKind::Access { what: "use a connected app".into(), detail: prompt.clone() },
            _ => DecisionKind::Access { what: "go ahead".into(), detail: prompt.clone() },
        },
    };
    DecisionView {
        id: DecisionId(stable_id(&request.request_id)),
        task,
        run: RunId(0),
        generation: 0,
        kind,
        state: if sending { DecisionState::Sending } else { DecisionState::Pending },
    }
}

/// Whether a command tied to a card is still on its way.
fn in_flight(app: &BuknoApp, key: &str) -> bool {
    let (Some(t3), Some(chat)) = (app.t3.as_ref(), app.remote.as_ref()) else { return false };
    app.remote_answers
        .get(key)
        .and_then(|id| t3.command(&chat.environment, id))
        .is_some_and(|e: &OutboxEntry| matches!(e.state, OutboxState::Sending))
}

/// Model and effort words for the composer: (model, effort, level).
fn model_words(app: &BuknoApp) -> (String, String, u8) {
    let t3 = app.t3.as_ref();
    let (selection, env) = match app.view {
        crate::app::View::RemoteNew => {
            let new = app.remote_new.as_ref();
            let env = t3.zip(new).and_then(|(t, n)| t.environment(&n.environment));
            (new.and_then(|n| n.model.clone()).map(|m| (m, None)), env)
        }
        _ => {
            let summary = t3.and_then(|t| t.open_summary());
            let env = t3.zip(app.remote.as_ref()).and_then(|(t, c)| t.environment(&c.environment));
            let selection = summary.map(|s| {
                let choice = ModelChoice {
                    instance_id: s.model_selection.instance().to_owned(),
                    model: s.model_selection.model.clone(),
                };
                (choice, s.model_selection.effort().map(str::to_owned))
            });
            (selection, env)
        }
    };
    let Some((choice, effort)) = selection else { return ("No model".into(), String::new(), 1) };
    let name = env
        .and_then(|e| e.providers.iter().find(|p| p.instance_id == choice.instance_id))
        .and_then(|p| p.models.iter().find(|m| m.slug == choice.model))
        .map_or_else(|| choice.model.clone(), |m| m.name.clone());
    let effort = effort.unwrap_or_else(|| "Default".into());
    let level = match effort.to_lowercase().as_str() {
        "minimal" | "low" => 1,
        "medium" | "default" => 3,
        "high" => 4,
        _ => 5,
    };
    let effort = effort[..1].to_uppercase() + &effort[1..];
    (name, effort, level)
}

fn runtime_mode(app: &BuknoApp) -> String {
    match app.view {
        crate::app::View::RemoteNew => app.remote_new.as_ref().map(|n| n.runtime_mode.clone()).unwrap_or_default(),
        _ => app.t3.as_ref().and_then(|t| t.open_summary()).and_then(|s| s.runtime_mode.clone()).unwrap_or_default(),
    }
}

fn provider(app: &BuknoApp) -> bukno_core::message::Provider {
    match app.view {
        crate::app::View::RemoteNew => {
            provider_of(app.remote_new.as_ref().and_then(|n| n.model.as_ref()).map_or("", |m| m.instance_id.as_str()))
        }
        _ => provider_of(app.t3.as_ref().and_then(|t| t.open_summary()).map_or("", |s| s.model_selection.instance())),
    }
}

fn draw_composer(app: &mut BuknoApp, ui: &mut Ui, rect: Rect, running: bool) {
    let theme = app.theme.clone();
    let (model, effort, effort_level) = model_words(app);
    let mode = runtime_mode(app);
    let permission = runtime_mode_label(&mode).to_owned();
    let provider = provider(app);
    let name = provider_name(provider);
    let placeholder = if running {
        format!("Queue a message for {name}, or Steer to add it to this turn")
    } else if app.view == crate::app::View::RemoteNew {
        format!("Ask {name} to do something")
    } else {
        format!("Follow up with {name}")
    };
    let blocked = app.remote_send_blocked();
    let props = ComposerProps {
        provider,
        placeholder: &placeholder,
        running,
        model: &model,
        effort: &effort,
        effort_level,
        permission: &permission,
        permission_menu: blocked.is_none_or(|b| !b.starts_with("Pair")),
        send_blocked: blocked,
        steer: true,
    };
    match composer::show(ui, &theme, rect, &mut app.composer, &props) {
        Some(ComposerAction::Send) => app.remote_submit(if running { Delivery::Queue } else { Delivery::Auto }),
        Some(ComposerAction::Steer) => app.remote_submit(Delivery::Steer),
        Some(ComposerAction::Stop) => app.remote_stop(),
        Some(ComposerAction::Permission) => {
            app.menu = if app.menu == Some(Menu::Permission) { None } else { Some(Menu::Permission) };
        }
        None => {}
    }
    if app.menu == Some(Menu::Permission) {
        mode_menu(app, ui, rect, &mode);
    }
}

/// T3's runtime modes. For an existing chat the change applies from its next turn.
fn mode_menu(app: &mut BuknoApp, ui: &mut Ui, composer: Rect, current: &str) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let width = 380.0;
    let item_h = 56.0;
    let height = RUNTIME_MODES.len() as f32 * item_h + 40.0;
    let rect =
        Rect::from_min_size(pos2(composer.left() + 40.0, composer.bottom() - 52.0 - height), vec2(width, height));
    let mut chosen = None;
    egui::Area::new(Id::new("t3-mode-menu")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ui.ctx(), |ui| {
        theme.paint_shadow(ui.painter(), rect, theme.radius.radius_lg, &theme.shadow.shadow_popover);
        ui.painter().rect_filled(rect, theme.radius.radius_lg, c.surface_popover);
        let mut y = rect.top() + 8.0;
        for (mode, label, description) in RUNTIME_MODES {
            let item = Rect::from_min_size(pos2(rect.left() + 6.0, y), vec2(width - 12.0, item_h - 4.0));
            let response = ui.interact(item, Id::new(("t3-mode", mode)), Sense::click());
            let selected = mode == current;
            response
                .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label));
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
                label,
                theme.font(&theme.text.t_ui_strong),
                c.text_primary,
            );
            ui.painter().text(
                pos2(item.left() + 10.0, item.top() + 34.0),
                Align2::LEFT_CENTER,
                description,
                theme.font(&theme.text.t_small),
                c.text_secondary,
            );
            crate::components::focus_ring(ui, &theme, &response, theme.radius.radius_md);
            if response.clicked() {
                chosen = Some(mode);
            }
            y += item_h;
        }
        ui.painter().text(
            pos2(rect.left() + 16.0, y + 12.0),
            Align2::LEFT_CENTER,
            "T3 and the provider enforce this, not Bukno.",
            theme.font(&theme.text.t_small),
            c.text_tertiary,
        );
    });
    if let Some(mode) = chosen {
        app.menu = None;
        if mode != current {
            match app.view {
                crate::app::View::RemoteNew => {
                    if let Some(new) = app.remote_new.as_mut() {
                        new.runtime_mode = mode.to_owned();
                    }
                }
                _ => {
                    if let Some(chat) = app.remote.clone() {
                        app.remote_command(
                            None,
                            Outgoing::SetRuntimeMode { thread_id: chat.thread, runtime_mode: mode.to_owned() },
                        );
                    }
                }
            }
        }
    } else if ui.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
        && !ui.ctx().pointer_interact_pos().is_some_and(|p| rect.contains(p))
    {
        app.menu = None;
    }
}

/// The open chat's run state in words, for the header.
fn header_state(app: &BuknoApp) -> Option<(&'static str, RunState)> {
    let t3 = app.t3.as_ref()?;
    let thread = t3.open_thread()?;
    let run = thread.active_run.as_ref()?;
    let status = thread.run_status.get(run).map(String::as_str).unwrap_or("running");
    let stopping =
        app.t3.as_ref().zip(app.remote.as_ref()).and_then(|(t, c)| t.environment(&c.environment)).is_some_and(|e| {
            e.outbox.iter().any(|o| {
                matches!(&o.request.outgoing, Outgoing::Interrupt { run_id, .. } if run_id == run)
                    && matches!(o.state, OutboxState::Sending | OutboxState::Accepted { .. })
            })
        });
    let state = if stopping {
        RunState::Cancelling
    } else if !thread.pending.is_empty() {
        match thread.pending[0].kind {
            PendingKind::Question { .. } => RunState::WaitingForInput,
            PendingKind::Approval { .. } => RunState::WaitingForApproval,
        }
    } else {
        match status {
            "preparing" => RunState::Preparing,
            "starting" => RunState::Starting,
            _ => RunState::Running,
        }
    };
    Some((state_words(state), state))
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
            thread.is_some_and(|t| t.active_run.is_some()),
            thread.is_some_and(|t| t.loaded),
            thread.is_some_and(|t| t.current),
            thread.is_some_and(|t| t.has_more_history),
            thread.is_some_and(|t| t.loading_history),
            thread.and_then(|t| t.history_error.clone()),
            env.map(|e| e.status.clone()),
        )
    };
    let run_state = header_state(app);

    // Header: title, then the run state or where the chat lives.
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
    } else if let Some((words, _)) = run_state {
        format!("{words} in T3")
    } else {
        "In T3".to_owned()
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

    // Composer at the bottom, then docked cards, then the transcript between.
    let docks = docks(app);
    let task = TaskId(stable_id(&format!("t3-thread:{}", app.remote.as_ref().map_or("", |r| r.thread.as_str()))));
    let decision_h = match docks.first() {
        Some(Dock::Request(r, _)) => {
            Some(decision::height(ui, &theme, &decision_view(r, task, false), column.width(), decision::CODE_MIN))
        }
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
    let note_line =
        app.notice.is_some() || (app.remote_send_blocked().is_some() && !app.composer.text.trim().is_empty());
    let mut dock_bottom = composer_rect.top() - if note_line && !docks.is_empty() { 28.0 } else { 10.0 };
    let mut placed = Vec::new();
    for dock in docks {
        let (h, min, code_max) = match &dock {
            Dock::Request(r, _) => {
                let view = decision_view(r, task, false);
                let room = dock_bottom - (header_top + 28.0 + DECISION_TRANSCRIPT_MIN);
                let full = decision::height(ui, &theme, &view, column.width(), decision::CODE_MAX);
                let code_max = (decision::CODE_MAX - (full - room).max(0.0)).max(decision::CODE_MIN);
                let mut h = decision::height(ui, &theme, &view, column.width(), code_max);
                if matches!(view.kind, DecisionKind::Question { .. }) {
                    h = h.min(room.max(QUESTION_CARD_MIN));
                }
                (h, DECISION_TRANSCRIPT_MIN, code_max)
            }
            Dock::Notice { text, actions, .. } => {
                (notice_height(ui, &theme, column.width(), text, actions.len()), 120.0, 0.0)
            }
        };
        let request = matches!(dock, Dock::Request(..));
        if dock_bottom - h < header_top + 28.0 + min && !request {
            break;
        }
        let rect = Rect::from_min_size(pos2(column.left(), dock_bottom - h), vec2(column.width(), h));
        dock_bottom = rect.top() - 8.0;
        placed.push((rect, dock, code_max));
    }

    let transcript_top = header_top + 28.0;
    let mut transcript_bottom = dock_bottom - 2.0;
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
            let (label, state) = run_state.unwrap_or(("Working", RunState::Running));
            orb::working_indicator(
                &mut child,
                &theme,
                rect,
                &Working {
                    accent: provider_color(&theme, provider),
                    label,
                    elapsed: None,
                    summary: None,
                    state: match state {
                        RunState::WaitingForApproval | RunState::WaitingForInput => OrbState::Waiting,
                        _ => OrbState::Thinking,
                    },
                    reduce_motion: app.reduce_motion,
                    mark: app.working_mark,
                },
            );
        }
    }

    let project = app
        .t3
        .as_ref()
        .and_then(|t| {
            let summary = t.open_summary()?;
            let env = t.environment(&app.remote.as_ref()?.environment)?;
            env.projects.iter().find(|p| p.id == summary.project_id).map(|p| p.title.clone())
        })
        .unwrap_or_default();
    let protection = format!("T3 enforces “{}” for this chat.", runtime_mode_label(&runtime_mode(app)));
    for (rect, dock, code_max) in placed {
        match dock {
            Dock::Request(request, count) => {
                let key = format!("request:{}", request.request_id);
                let view = decision_view(&request, task, in_flight(app, &key));
                let mut answers = std::mem::take(&mut app.extra.answers);
                let action = decision::show(
                    ui,
                    &theme,
                    rect,
                    code_max,
                    &view,
                    provider_name(provider),
                    &project,
                    &protection,
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
                let Some(chat) = app.remote.clone() else { continue };
                let thread_id = chat.thread;
                let request_id = request.request_id.clone();
                let outgoing = match (action, &request.kind) {
                    (Some(CardAction::Allow), _) => {
                        Some(Outgoing::Approve { thread_id, request_id, decision: "accept" })
                    }
                    (Some(CardAction::Decline), PendingKind::Approval { .. }) => {
                        Some(Outgoing::Approve { thread_id, request_id, decision: "decline" })
                    }
                    (Some(CardAction::Decline), PendingKind::Question { .. }) => {
                        Some(Outgoing::Dismiss { thread_id, request_id })
                    }
                    (Some(CardAction::Answers(given)), PendingKind::Question { questions, .. }) => {
                        Some(Outgoing::Answer { thread_id, request_id, answers: answers_for(questions, &given) })
                    }
                    _ => None,
                };
                if let Some(outgoing) = outgoing {
                    app.remote_command(Some(key), outgoing);
                }
            }
            Dock::Notice { key, text, problem, actions } => {
                let clicked = notice_card(ui, &theme, rect, Id::new(("remote-dock", &key)), &text, problem, &actions);
                if let Some(i) = clicked {
                    dock_action(app, &key, actions[i]);
                }
            }
        }
    }

    if let Some(notice) = app.notice.clone() {
        let color = if app.extra.notice_problem { c.negative } else { c.text_secondary };
        ui.painter().text(
            pos2(column.left(), composer_rect.top() - 20.0),
            Align2::LEFT_CENTER,
            notice,
            theme.font(&theme.text.t_small),
            color,
        );
    }
    draw_composer(app, ui, composer_rect, working);
}

fn dock_action(app: &mut BuknoApp, key: &str, label: &str) {
    let Some(chat) = app.remote.clone() else { return };
    let thread_id = chat.thread.clone();
    let (kind, id) = key.split_once(':').unwrap_or((key, ""));
    let thread = app.t3.as_ref().and_then(|t| t.open_thread()).cloned();
    match (kind, label) {
        ("pair", _) => app.show_environments = true,
        ("unconfirmed", "Send again") => {
            if let Some(t3) = app.t3.as_ref() {
                t3.retry(&chat.environment, id);
            }
        }
        ("unconfirmed" | "rejected", "Dismiss") => {
            if let Some(t3) = app.t3.as_ref() {
                t3.dismiss_command(&chat.environment, id);
            }
            app.remote_pending.retain(|p| p.command_id != id);
        }
        ("held", "Resume") => app.remote_command(None, Outgoing::ResumeQueue { thread_id }),
        ("held", "Remove") => {
            for queued in thread.iter().flat_map(|t| t.queued.iter()).filter(|q| q.held) {
                app.remote_command(
                    None,
                    Outgoing::CancelQueued { thread_id: thread_id.clone(), run_id: queued.run_id.clone() },
                );
            }
        }
        ("queued", "Steer now") => {
            if let Some(target) = thread.as_ref().and_then(|t| t.active_run.clone()) {
                app.remote_command(
                    None,
                    Outgoing::PromoteQueued { thread_id, queued_run_id: id.to_owned(), target_run_id: target },
                );
            }
        }
        ("queued", "Remove") => {
            app.remote_command(None, Outgoing::CancelQueued { thread_id, run_id: id.to_owned() });
        }
        _ => {}
    }
}

/// A new chat in a T3 project: the composer, the project, and the model.
pub fn new_chat(app: &mut BuknoApp, ui: &mut Ui, body: Rect) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let column = column_rect(body, &theme);
    let height = composer::height_for(ui, &theme, &app.composer, column.width(), body.height() - 260.0);
    let top = body.top() + ((body.height() - height - 120.0) / 2.0 - 40.0).clamp(16.0, 128.0);
    ui.painter().text(
        pos2(column.center().x, top + 17.0),
        Align2::CENTER_CENTER,
        "What should we work on?",
        theme.font(&theme.text.t_display),
        c.text_primary,
    );
    let rect = Rect::from_min_size(pos2(column.left(), top + 58.0), vec2(column.width(), height));
    if let Some(notice) = app.notice.clone() {
        ui.painter().text(
            pos2(column.left(), rect.top() - 14.0),
            Align2::LEFT_CENTER,
            notice,
            theme.font(&theme.text.t_small),
            c.negative,
        );
    }
    draw_composer(app, ui, rect, false);

    // Where it runs, and with which model.
    let meta_y = rect.bottom() + 36.0;
    let Some(new) = app.remote_new.clone() else { return };
    let env = app.t3.as_ref().and_then(|t| t.environment(&new.environment)).cloned();
    let project = env
        .as_ref()
        .and_then(|e| e.projects.iter().find(|p| p.id == new.project).cloned())
        .map_or_else(|| "Project".to_owned(), |p| p.title);
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
        format!("{project} on {}", env.as_ref().map_or("T3", |e| e.saved.label.as_str())),
        theme.font(&theme.text.t_ui),
        c.text_secondary,
    );

    let (model, _, _) = model_words(app);
    let provider = provider(app);
    let label = format!("{} · {model}", provider_name(provider));
    let galley = painter.layout_no_wrap(label.clone(), theme.font(&theme.text.t_ui), c.text_secondary);
    let pick = Rect::from_min_size(
        pos2(column.right() - galley.size().x - 40.0, meta_y - 14.0),
        vec2(galley.size().x + 34.0, 28.0),
    );
    let response = ui.interact(pick, Id::new("t3-new-model"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Model: {label}")));
    if response.hovered() {
        ui.painter().rect_filled(pick, theme.radius.radius_md, c.surface_hover);
    }
    ui.painter().galley(pos2(pick.left() + 8.0, meta_y - galley.size().y / 2.0), galley, c.text_secondary);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(pick.right() - 12.0, meta_y), vec2(12.0, 12.0)),
        Icon::ChevronDown,
        11.0,
        c.text_tertiary,
    );
    crate::components::focus_ring(ui, &theme, &response, theme.radius.radius_md);
    if response.clicked() {
        app.menu = if app.menu == Some(Menu::NewChatProject) { None } else { Some(Menu::NewChatProject) };
    }
    if app.menu == Some(Menu::NewChatProject)
        && let Some(env) = env
    {
        model_menu(app, ui, pick, &env.providers);
    }
}

/// Every model of every ready provider on the server, grouped by provider.
fn model_menu(app: &mut BuknoApp, ui: &mut Ui, anchor: Rect, providers: &[bukno_t3_client::model::Provider]) {
    let theme = app.theme.clone();
    let c = &theme.color;
    let mut options: Vec<(String, ModelChoice)> = Vec::new();
    for p in providers.iter().filter(|p| p.enabled && p.installed) {
        for m in &p.models {
            // Several variants can share a name; the slug tells them apart.
            let shared = p.models.iter().filter(|o| o.name == m.name).count() > 1;
            let name = if shared { format!("{} ({})", m.name, m.slug) } else { m.name.clone() };
            options.push((
                format!("{} · {name}", p.name()),
                ModelChoice { instance_id: p.instance_id.clone(), model: m.slug.clone() },
            ));
        }
    }
    let row_h = 30.0;
    let height = (options.len() as f32 * row_h + 12.0).min(420.0);
    let rect = Rect::from_min_size(pos2(anchor.right() - 320.0, anchor.bottom() + 4.0), vec2(320.0, height));
    let current = app.remote_new.as_ref().and_then(|n| n.model.clone());
    let mut chosen = None;
    egui::Area::new(Id::new("t3-model-menu")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ui.ctx(), |ui| {
        theme.paint_shadow(ui.painter(), rect, theme.radius.radius_lg, &theme.shadow.shadow_popover);
        ui.painter().rect_filled(rect, theme.radius.radius_lg, c.surface_popover);
        let inner = rect.shrink2(vec2(6.0, 6.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        egui::ScrollArea::vertical().id_salt("t3-model-list").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            for (i, (label, choice)) in options.iter().enumerate() {
                let (item, response) = ui.allocate_exact_size(vec2(inner.width(), row_h), Sense::click());
                let selected = current.as_ref() == Some(choice);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
                });
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
                    label,
                    theme.font(&theme.text.t_ui),
                    c.text_primary,
                );
                if response.clicked() {
                    chosen = Some(i);
                }
            }
        });
    });
    if let Some(i) = chosen {
        if let Some(new) = app.remote_new.as_mut() {
            new.model = Some(options[i].1.clone());
        }
        app.menu = None;
    } else if ui.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
        && !ui.ctx().pointer_interact_pos().is_some_and(|p| rect.contains(p) || anchor.contains(p))
    {
        app.menu = None;
    }
}
