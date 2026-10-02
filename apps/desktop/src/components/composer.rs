//! The Composer: a rounded input over a compact toolbar.
//!
//! Enter sends, except while an input method is composing text or when
//! Shift is held (Shift+Enter inserts a newline). Cmd+Enter (Ctrl+Enter on
//! Windows) also sends.

use std::sync::Arc;

use bukno_core::message::Provider;
use egui::text::LayoutJob;
use egui::{
    Align2, Event, Galley, Id, ImeEvent, Key, Rect, Response, Sense, TextBuffer, Ui, UiBuilder, WidgetInfo, WidgetType,
    pos2, vec2,
};

use super::icons::{self, Icon};
use super::{focus_ring, icon_button, provider_color};
use crate::theme::Theme;

pub const COMPOSER_ID: &str = "bukno-composer";
const PAD_TOP: f32 = 14.0;
const PAD_RIGHT: f32 = 10.0;
const PAD_BOTTOM: f32 = 10.0;
const PAD_LEFT: f32 = 16.0;
const INPUT_LINE: f32 = 22.0;
const INPUT_MIN: f32 = 44.0;
const INPUT_MAX: f32 = 220.0;
const BAR_GAP: f32 = 6.0;

pub fn composer_id() -> Id {
    Id::new(COMPOSER_ID)
}

#[derive(Default)]
pub struct ComposerState {
    pub text: String,
    /// Increases on every edit; a send captures it.
    pub revision: u64,
    /// True while an input method shows uncommitted text.
    pub composing: bool,
}

pub struct ComposerProps<'a> {
    pub provider: Provider,
    pub placeholder: &'a str,
    pub running: bool,
    pub model: &'a str,
    pub effort: &'a str,
    /// 1 to 5 on the provider's effort ramp.
    pub effort_level: u8,
    pub permission: &'a str,
    /// Why Send is unavailable right now, shown beside the composer. The
    /// draft is never cleared while this is set.
    pub send_blocked: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerAction {
    Send,
    Stop,
}

/// The composer's height for its current text, never more than
/// `max_height` (the text scrolls inside it beyond that).
pub fn height_for(ui: &Ui, theme: &Theme, state: &ComposerState, width: f32, max_height: f32) -> f32 {
    let wrap = width - PAD_LEFT - PAD_RIGHT - 6.0;
    let galley = ui.fonts_mut(|f| f.layout_job(input_job(theme, &state.text, wrap)));
    let chrome = PAD_TOP + BAR_GAP + theme.size.size_control + PAD_BOTTOM;
    let input_max = (max_height - chrome).clamp(INPUT_MIN, INPUT_MAX);
    let input = galley.size().y.clamp(INPUT_MIN, input_max);
    (chrome + input).max(theme.size.size_composer_min)
}

fn input_job(theme: &Theme, text: &str, wrap: f32) -> LayoutJob {
    let mut format = theme.format(&theme.text.t_body, theme.color.text_primary);
    format.line_height = Some(INPUT_LINE);
    let mut job = LayoutJob::single_section(text.to_owned(), format);
    job.wrap.max_width = wrap;
    job
}

/// Draw the composer in `rect` (use [`height_for`] for its height).
pub fn show(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    state: &mut ComposerState,
    props: &ComposerProps<'_>,
) -> Option<ComposerAction> {
    let c = &theme.color;
    let id = composer_id();
    let mut action = None;
    theme.paint_shadow(ui.painter(), rect, theme.radius.radius_xl, &theme.shadow.shadow_composer);
    ui.painter().rect_filled(rect, theme.radius.radius_xl, c.surface_composer);

    // Composition belongs to the focused field. Losing focus ends it, so a
    // cancellation the input method sends afterwards cannot leave it stuck.
    let focused = ui.memory(|m| m.has_focus(id));
    if !focused {
        state.composing = false;
    }
    // Enter handling happens before the text field sees the events.
    if focused {
        let mut send = false;
        ui.input_mut(|i| {
            let mut ime_activity = false;
            for event in &i.events {
                if let Event::Ime(ime) = event {
                    ime_activity = true;
                    match ime {
                        ImeEvent::Preedit { text, .. } => state.composing = !text.is_empty(),
                        ImeEvent::Commit(_) => state.composing = false,
                        _ => {}
                    }
                }
            }
            // Enter during composition belongs to the input method.
            let composing = state.composing || ime_activity;
            i.events.retain(|event| match event {
                Event::Key { key: Key::Enter, pressed, modifiers, .. } if !modifiers.shift && !composing => {
                    if *pressed {
                        send = true;
                    }
                    false
                }
                _ => true,
            });
        });
        if send && !state.text.trim().is_empty() && props.send_blocked.is_none() {
            action = Some(ComposerAction::Send);
        }
    }
    if let Some(reason) = props.send_blocked
        && !state.text.trim().is_empty()
    {
        ui.painter().text(
            pos2(rect.left() + PAD_LEFT, rect.top() - 14.0),
            Align2::LEFT_CENTER,
            reason,
            theme.font(&theme.text.t_small),
            c.text_tertiary,
        );
    }

    let input_rect = Rect::from_min_max(
        pos2(rect.left() + PAD_LEFT, rect.top() + PAD_TOP),
        pos2(rect.right() - PAD_RIGHT, rect.bottom() - PAD_BOTTOM - theme.size.size_control - BAR_GAP),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(input_rect));
    let theme_for_layout = theme.clone();
    let mut layouter = move |ui: &Ui, text: &dyn TextBuffer, wrap: f32| -> Arc<Galley> {
        ui.fonts_mut(|f| f.layout_job(input_job(&theme_for_layout, text.as_str(), wrap)))
    };
    let edit = egui::ScrollArea::vertical()
        .id_salt("composer-scroll")
        .max_height(input_rect.height())
        .auto_shrink([false, true])
        .show(&mut child, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut state.text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(input_rect.width() - 6.0)
                    .desired_rows(2)
                    .hint_text(
                        egui::RichText::new(props.placeholder)
                            .color(c.text_tertiary)
                            .font(theme.font(&theme.text.t_body)),
                    )
                    .layouter(&mut layouter),
            )
        })
        .inner;
    if edit.changed() {
        state.revision += 1;
    }
    // The placeholder names the recipient; screen readers get the same name.
    ui.ctx().accesskit_node_builder(id, |node| node.set_label(props.placeholder));

    // Toolbar.
    let bar = Rect::from_min_max(
        pos2(rect.left() + PAD_LEFT - 6.0, rect.bottom() - PAD_BOTTOM - theme.size.size_control),
        pos2(rect.right() - PAD_RIGHT, rect.bottom() - PAD_BOTTOM),
    );
    let add = Rect::from_min_size(bar.left_top(), vec2(32.0, 32.0));
    ui.add_enabled_ui(false, |ui| {
        icon_button(ui, theme, id.with("add"), add, Icon::Plus, "Add files, available in a later pass");
    });
    ui.painter().text(
        pos2(add.right() + 8.0, bar.center().y),
        Align2::LEFT_CENTER,
        props.permission,
        theme.font(&theme.text.t_ui),
        c.text_secondary,
    );

    let primary = Rect::from_min_size(pos2(bar.right() - 32.0, bar.top()), vec2(32.0, 32.0));
    let has_text = !state.text.trim().is_empty();
    let can_send = has_text && props.send_blocked.is_none();
    let model_right = if props.running && !has_text {
        let stop = Rect::from_min_max(pos2(bar.right() - 78.0, bar.top()), bar.right_bottom());
        if stop_button(ui, theme, stop, id.with("stop")).clicked() {
            action = Some(ComposerAction::Stop);
        }
        stop.left() - 4.0
    } else {
        if send_button(ui, theme, primary, id.with("send"), can_send).clicked() && can_send {
            action = Some(ComposerAction::Send);
        }
        let mut left = primary.left() - 4.0;
        if props.running {
            let stop = Rect::from_min_size(pos2(left - 32.0, bar.top()), vec2(32.0, 32.0));
            if icon_button(ui, theme, id.with("stop-small"), stop, Icon::Stop, "Stop").clicked() {
                action = Some(ComposerAction::Stop);
            }
            left = stop.left() - 4.0;
        }
        left
    };
    model_control(ui, theme, bar, model_right, props);
    action
}

fn send_button(ui: &mut Ui, theme: &Theme, rect: Rect, id: Id, enabled: bool) -> Response {
    let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, "Send"));
    let c = &theme.color;
    let (fill, ink) = if enabled { (c.action, c.on_action) } else { (c.surface_raised, c.text_disabled) };
    ui.painter().circle_filled(rect.center(), rect.width() / 2.0, fill);
    icons::paint(ui.painter(), rect, Icon::ArrowUp, 16.0, ink);
    focus_ring(ui, theme, &response, rect.width() / 2.0);
    response
}

fn stop_button(ui: &mut Ui, theme: &Theme, rect: Rect, id: Id) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Stop"));
    let c = &theme.color;
    theme.paint_shadow(ui.painter(), rect, theme.radius.radius_md, &theme.shadow.shadow_raised);
    let fill = if response.hovered() { c.surface_raised.gamma_multiply(1.12) } else { c.surface_raised };
    ui.painter().rect_filled(rect, theme.radius.radius_md, fill);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)),
        Icon::Stop,
        14.0,
        c.text_primary,
    );
    ui.painter().text(
        pos2(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Stop",
        theme.font(&theme.text.t_ui_strong),
        c.text_primary,
    );
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    response
}

/// The closed ModelControl: lightning, model, quieter effort. Display only
/// until the picker lands; its accessible name says what is selected.
fn model_control(ui: &mut Ui, theme: &Theme, bar: Rect, right: f32, props: &ComposerProps<'_>) {
    let c = &theme.color;
    let painter = ui.painter();
    let effort = painter.layout_job(theme.job(props.effort, &theme.text.t_small, c.text_tertiary, f32::INFINITY));
    let model = painter.layout_job(theme.job(props.model, &theme.text.t_ui_strong, c.text_primary, f32::INFINITY));
    let width = 8.0 + 14.0 + 6.0 + model.size().x + 6.0 + effort.size().x + 8.0;
    let rect = Rect::from_min_max(pos2(right - width, bar.top()), pos2(right, bar.bottom()));
    let response = ui.interact(rect, composer_id().with("model"), Sense::hover());
    let label = format!("Model {}, effort {}", props.model, props.effort);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &label));
    let ramp = effort_color(theme, props.provider, props.effort_level);
    let mut x = rect.left() + 8.0;
    icons::paint(
        painter,
        Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0)),
        Icon::Bolt,
        14.0,
        ramp,
    );
    x += 14.0 + 6.0;
    let model_w = model.size().x;
    painter.galley(pos2(x, rect.center().y - model.size().y / 2.0), model, c.text_primary);
    x += model_w + 6.0;
    painter.galley(pos2(x, rect.center().y - effort.size().y / 2.0 + 1.0), effort, c.text_tertiary);
    let _ = provider_color;
}

pub fn effort_color(theme: &Theme, provider: Provider, level: u8) -> egui::Color32 {
    let c = &theme.color;
    let ramp = match provider {
        Provider::Codex => [c.codex_effort_1, c.codex_effort_2, c.codex_effort_3, c.codex_effort_4, c.codex_effort_5],
        Provider::Claude => {
            [c.claude_effort_1, c.claude_effort_2, c.claude_effort_3, c.claude_effort_4, c.claude_effort_5]
        }
    };
    ramp[(level.clamp(1, 5) - 1) as usize]
}
