//! Native equivalents of the design system's components. Each one only draws
//! itself; screens decide where it goes.

pub mod build_grid;
pub mod composer;
pub mod decision;
pub mod icons;
pub mod orb;
pub mod rows;
pub mod streak;

use bukno_core::message::Provider;
use egui::{Align2, Color32, Id, Rect, Response, Sense, Ui, WidgetInfo, WidgetType, vec2};

use crate::theme::Theme;
use icons::Icon;

pub fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Codex => "Codex",
        Provider::Claude => "Claude",
    }
}

pub fn provider_color(theme: &Theme, provider: Provider) -> Color32 {
    match provider {
        Provider::Codex => theme.color.codex,
        Provider::Claude => theme.color.claude,
    }
}

pub fn provider_mark(provider: Provider) -> Icon {
    match provider {
        Provider::Codex => Icon::Hexagon,
        Provider::Claude => Icon::Diamond,
    }
}

#[derive(Clone, Copy, Default)]
struct KeyboardMode(bool);

/// Focus rings show after keyboard navigation and hide after a pointer press,
/// so a clicked button does not keep a ring.
pub fn update_keyboard_mode(ctx: &egui::Context) {
    let (keys, pointer) = ctx.input(|i| {
        let keys = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Tab
                        | egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight,
                    pressed: true,
                    ..
                }
            )
        });
        (keys, i.pointer.any_pressed())
    });
    if keys || pointer {
        ctx.data_mut(|d| d.insert_temp(Id::NULL.with("keyboard-mode"), KeyboardMode(keys && !pointer)));
    }
}

pub fn keyboard_mode(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<KeyboardMode>(Id::NULL.with("keyboard-mode"))).is_some_and(|m| m.0)
}

/// Draw the focus ring when this response has keyboard focus.
pub fn focus_ring(ui: &Ui, theme: &Theme, response: &Response, radius: f32) {
    if response.has_focus() && keyboard_mode(ui.ctx()) {
        theme.paint_focus_ring(ui.painter(), response.rect, radius);
    }
}

/// A flat 28 or 32 point icon button with an accessible name.
pub fn icon_button(ui: &mut Ui, theme: &Theme, id: Id, rect: Rect, icon: Icon, label: &str) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let radius = if rect.height() <= 28.0 { theme.radius.radius_sm } else { theme.radius.radius_md };
    let c = &theme.color;
    let hovered = response.hovered() && enabled;
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius, c.surface_selected);
    } else if hovered {
        ui.painter().rect_filled(rect, radius, c.surface_hover);
    }
    let color = if !enabled {
        c.text_disabled
    } else if hovered {
        c.text_primary
    } else {
        c.text_secondary
    };
    icons::paint(ui.painter(), rect, icon, 16.0, color);
    focus_ring(ui, theme, &response, radius);
    response.on_hover_text(label)
}

/// A raised secondary button sitting on the composer or over the transcript.
pub fn raised_button(ui: &mut Ui, theme: &Theme, rect: Rect, text: &str, label: &str) -> Response {
    let id = Id::new(("raised-button", label));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let c = &theme.color;
    theme.paint_shadow(ui.painter(), rect, theme.radius.radius_md, &theme.shadow.shadow_raised);
    let fill = if response.is_pointer_button_down_on() {
        c.surface_selected
    } else if response.hovered() {
        c.surface_raised.gamma_multiply(1.12)
    } else {
        c.surface_raised
    };
    ui.painter().rect_filled(rect, theme.radius.radius_md, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme.font(&theme.text.t_ui_strong), c.text_primary);
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    response
}

/// Keyboard hint chips, right-aligned at `right`. Returns their total width.
pub fn kbd(ui: &Ui, theme: &Theme, right: egui::Pos2, keys: &[&str]) -> f32 {
    let font = egui::FontId::new(11.0, egui::FontFamily::Name("geist-500".into()));
    let mut x = right.x;
    for key in keys.iter().rev() {
        let galley = ui.painter().layout_no_wrap((*key).to_owned(), font.clone(), theme.color.text_secondary);
        let w = (galley.size().x + 8.0).max(18.0);
        let rect = Rect::from_min_size(egui::pos2(x - w, right.y - 9.0), vec2(w, 18.0));
        ui.painter().rect_filled(rect, theme.radius.radius_xs, Color32::from_white_alpha(0x12));
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, theme.color.text_secondary);
        x -= w + 2.0;
    }
    right.x - x
}

/// The platform's command key label.
pub fn command_key() -> &'static str {
    if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" }
}

/// The primary action on a card: filled, with an optional key hint.
pub fn raised_primary(ui: &mut Ui, theme: &Theme, id: Id, at: egui::Pos2, text: &str, key: Option<&str>) -> Response {
    let c = &theme.color;
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme.font(&theme.text.t_ui_strong), c.on_action);
    let hint = key.map_or(0.0, |_| 26.0);
    let rect = Rect::from_min_size(at, vec2(galley.size().x + 24.0 + hint, 32.0));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
    let fill = if response.hovered() { c.action.gamma_multiply(0.92) } else { c.action };
    ui.painter().rect_filled(rect, theme.radius.radius_md, fill);
    ui.painter().galley(egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0), galley, c.on_action);
    if let Some(key) = key {
        let chip = Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), vec2(20.0, 18.0));
        ui.painter().rect_filled(chip, theme.radius.radius_xs, Color32::from_black_alpha(0x18));
        ui.painter().text(
            chip.center(),
            Align2::CENTER_CENTER,
            key,
            egui::FontId::new(11.0, egui::FontFamily::Name("geist-500".into())),
            c.on_action,
        );
    }
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    response
}

/// A quiet text button ending at `right.x`, with an optional key hint.
pub fn flat_text_button(
    ui: &mut Ui,
    theme: &Theme,
    id: Id,
    right: egui::Pos2,
    text: &str,
    key: Option<&str>,
) -> Response {
    let c = &theme.color;
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme.font(&theme.text.t_ui), c.text_secondary);
    let hint = key.map_or(0.0, |_| 30.0);
    let width = galley.size().x + 20.0 + hint;
    let rect = Rect::from_min_size(egui::pos2(right.x - width, right.y), vec2(width, 32.0));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
    if response.hovered() {
        ui.painter().rect_filled(rect, theme.radius.radius_md, c.surface_hover);
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        c.text_secondary,
    );
    if let Some(key) = key {
        kbd(ui, theme, egui::pos2(rect.right() - 8.0, rect.center().y), &[key]);
    }
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    response
}
