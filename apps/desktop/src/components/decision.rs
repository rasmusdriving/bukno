//! The ApprovalCard: an approval or question from the running agent, docked
//! above the composer (specification section 11, journey screen 2.2).
//!
//! Its keys act only while the card itself has focus, so the Enter that sent
//! a message can never approve a command that arrives a moment later.

use std::collections::HashMap;

use bukno_core::decision::{DecisionKind, DecisionState, DecisionView};
use egui::{Align2, Event, Id, Key, Rect, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

use super::icons::{self, Icon};
use super::{focus_ring, kbd};
use crate::theme::Theme;

const PAD: f32 = 16.0;
/// Tallest the command or diff box grows before it scrolls.
pub const CODE_MAX: f32 = 110.0;
/// Shortest it gets in a small window: two lines, still scrollable.
pub const CODE_MIN: f32 = 40.0;
const MAX_FILES: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardAction {
    Allow,
    Decline,
    Answers(Vec<(String, Vec<String>)>),
}

pub fn card_id(decision: &DecisionView) -> Id {
    Id::new(("decision-card", decision.id))
}

fn title(kind: &DecisionKind, provider: &str) -> String {
    match kind {
        DecisionKind::Command { .. } => format!("{provider} wants to run a command"),
        DecisionKind::FileChange { files, .. } if files.len() == 1 => format!("{provider} wants to change a file"),
        DecisionKind::FileChange { .. } => format!("{provider} wants to change files"),
        DecisionKind::Question { .. } => format!("{provider} has a question"),
    }
}

/// The text shown in the code box, if any.
fn body(kind: &DecisionKind) -> Option<String> {
    match kind {
        DecisionKind::Command { command, .. } => Some(command.clone()),
        DecisionKind::FileChange { files, .. } if !files.is_empty() => {
            let mut shown: Vec<String> = files.iter().take(MAX_FILES).cloned().collect();
            if files.len() > MAX_FILES {
                shown.push(format!("and {} more", files.len() - MAX_FILES));
            }
            Some(shown.join("\n"))
        }
        _ => None,
    }
}

fn reason(kind: &DecisionKind) -> Option<&str> {
    match kind {
        DecisionKind::Command { reason, .. } | DecisionKind::FileChange { reason, .. } => reason.as_deref(),
        DecisionKind::Question { .. } => None,
    }
}

/// The card's height when its command box is at most `code_max` tall.
pub fn height(ui: &Ui, theme: &Theme, decision: &DecisionView, width: f32, code_max: f32) -> f32 {
    let inner = width - 2.0 * PAD;
    let mut h = PAD + 22.0 + 12.0;
    if let Some(text) = body(&decision.kind) {
        let galley =
            ui.painter().layout_job(theme.job(text, &theme.text.t_code, theme.color.text_primary, inner - 24.0));
        h += galley.size().y.min(code_max) + 24.0 + 10.0;
    }
    if let DecisionKind::Question { questions } = &decision.kind {
        for q in questions {
            let galley =
                ui.painter().layout_job(theme.job(q.text.clone(), &theme.text.t_ui, theme.color.text_primary, inner));
            h += 18.0 + galley.size().y + 8.0 + if q.options.is_empty() || q.other { 70.0 } else { 40.0 };
        }
    }
    h += 20.0 + 10.0; // reason or protection line
    h + 32.0 + PAD
}

/// Draw the card in `rect` (use [`height`] with the same `code_max`).
/// `protection` says what enforces the action; `folder` is where it runs.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    code_max: f32,
    decision: &DecisionView,
    provider: &str,
    folder: &str,
    protection: &str,
    answers: &mut HashMap<String, String>,
) -> Option<CardAction> {
    let c = &theme.color;
    let id = card_id(decision);
    theme.paint_shadow(ui.painter(), rect, theme.radius.radius_lg, &theme.shadow.shadow_raised);
    ui.painter().rect_filled(rect, theme.radius.radius_lg, c.surface_composer);
    let header = title(&decision.kind, provider);
    // The card is focusable, so its keys act only on purpose.
    let response = ui.interact(rect, id, Sense::focusable_noninteractive());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, &header));
    let focused = response.has_focus();
    if focused {
        focus_ring(ui, theme, &response, theme.radius.radius_lg);
    }
    let mut action = None;
    let sending = decision.state == DecisionState::Sending;
    let inner = rect.width() - 2.0 * PAD;
    let x = rect.left() + PAD;
    let mut y = rect.top() + PAD;

    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(x + 8.0, y + 11.0), vec2(16.0, 16.0)),
        Icon::Alert,
        15.0,
        c.text_primary,
    );
    ui.painter().text(
        pos2(x + 26.0, y + 11.0),
        Align2::LEFT_CENTER,
        &header,
        theme.font(&theme.text.t_ui_strong),
        c.text_primary,
    );
    if !folder.is_empty() {
        ui.painter().text(
            pos2(rect.right() - PAD, y + 11.0),
            Align2::RIGHT_CENTER,
            format!("in {folder}"),
            theme.font(&theme.text.t_small),
            c.text_tertiary,
        );
    }
    y += 22.0 + 12.0;

    if let Some(text) = body(&decision.kind) {
        let galley = ui.painter().layout_job(theme.job(text, &theme.text.t_code, c.text_primary, inner - 24.0));
        let full = galley.size().y;
        let h = full.min(code_max) + 24.0;
        let code = Rect::from_min_size(pos2(x, y), vec2(inner, h));
        ui.painter().rect_filled(code, theme.radius.radius_md, c.surface_code);
        let clip = code.shrink(12.0);
        if full <= clip.height() + 0.5 {
            ui.painter().with_clip_rect(clip).galley(clip.min, galley, c.text_primary);
        } else {
            // Longer than the box: it scrolls, so the whole command can be read before answering.
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(clip));
            egui::ScrollArea::vertical().id_salt(id.with("code")).auto_shrink([false, false]).show(&mut child, |ui| {
                ui.add(egui::Label::new(galley).selectable(false));
            });
        }
        y += h + 10.0;
    }

    if let DecisionKind::Question { questions } = &decision.kind {
        for q in questions {
            ui.painter().text(
                pos2(x, y),
                Align2::LEFT_TOP,
                &q.header,
                theme.font(&theme.text.t_small),
                c.text_tertiary,
            );
            y += 18.0;
            let galley = ui.painter().layout_job(theme.job(q.text.clone(), &theme.text.t_ui, c.text_primary, inner));
            let gh = galley.size().y;
            ui.painter().galley(pos2(x, y), galley, c.text_primary);
            y += gh + 8.0;
            let mut bx = x;
            for option in &q.options {
                let chosen = answers.get(&q.id) == Some(option);
                let galley = ui.painter().layout_no_wrap(option.clone(), theme.font(&theme.text.t_ui), c.text_primary);
                let w = galley.size().x + 20.0;
                let b = Rect::from_min_size(pos2(bx, y), vec2(w, 30.0));
                let r = ui.interact(b, id.with(("option", &q.id, option)), Sense::click());
                r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, !sending, chosen, option));
                let fill = if chosen {
                    c.surface_selected
                } else if r.hovered() {
                    c.surface_hover
                } else {
                    c.surface_raised
                };
                ui.painter().rect_filled(b, theme.radius.radius_md, fill);
                ui.painter().galley(b.center() - galley.size() / 2.0, galley, c.text_primary);
                focus_ring(ui, theme, &r, theme.radius.radius_md);
                if r.clicked() && !sending {
                    answers.insert(q.id.clone(), option.clone());
                }
                bx += w + 6.0;
            }
            if !q.options.is_empty() {
                y += 30.0 + 10.0;
            }
            if q.options.is_empty() || q.other {
                let field = Rect::from_min_size(pos2(x, y), vec2(inner, 60.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field));
                let text = answers.entry(q.id.clone()).or_default();
                child.add_sized(
                    field.size(),
                    egui::TextEdit::multiline(text).id(id.with(("text", &q.id))).hint_text("Type an answer"),
                );
                y += 70.0;
            }
        }
    }

    let note = reason(&decision.kind).map(str::to_owned).unwrap_or_else(|| protection.to_owned());
    let mut job = theme.job(note, &theme.text.t_small, c.text_secondary, inner);
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(x, y), galley, c.text_secondary);
    y += 20.0 + 10.0;

    let bar = y;
    if sending {
        ui.painter().text(
            pos2(x, bar + 16.0),
            Align2::LEFT_CENTER,
            "Sending decision…",
            theme.font(&theme.text.t_ui),
            c.text_secondary,
        );
        return None;
    }
    let question = matches!(decision.kind, DecisionKind::Question { .. });
    let primary_text = if question { "Send answer" } else { "Allow once" };
    let primary = super::raised_primary(ui, theme, id.with("allow"), pos2(x, bar), primary_text, Some("↵"));
    let decline_text = if question { "Skip" } else { "Deny" };
    let decline =
        super::flat_text_button(ui, theme, id.with("deny"), pos2(rect.right() - PAD, bar), decline_text, Some("Esc"));
    let mut allow = primary.clicked();
    let mut deny = decline.clicked();
    if focused {
        ui.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift => {
                    allow = true;
                    false
                }
                Event::Key { key: Key::Escape, pressed: true, .. } => {
                    deny = true;
                    false
                }
                _ => true,
            })
        });
    }
    if allow {
        action = Some(match &decision.kind {
            DecisionKind::Question { questions } => CardAction::Answers(
                questions
                    .iter()
                    .map(|q| {
                        (
                            q.id.clone(),
                            answers.get(&q.id).filter(|a| !a.trim().is_empty()).cloned().into_iter().collect(),
                        )
                    })
                    .collect(),
            ),
            _ => CardAction::Allow,
        });
    } else if deny {
        action = Some(CardAction::Decline);
    }
    let _ = kbd;
    action
}
