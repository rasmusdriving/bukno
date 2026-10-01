//! Sidebar rows: SectionLabel, ProjectRow, ChatRow, UsageMeter and ProfileRow.

use bukno_core::message::Provider;
use egui::{Align2, Id, Rect, Response, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

use super::icons::{self, Icon};
use super::{focus_ring, kbd, provider_mark};
use crate::theme::Theme;

pub enum Trailing<'a> {
    None,
    Keys(&'a [&'a str]),
    Mark(Provider),
    /// The working ring for a chat whose run is active elsewhere.
    Working(Provider),
}

pub struct Row<'a> {
    pub id: Id,
    pub title: &'a str,
    pub lead: Option<Icon>,
    pub trailing: Trailing<'a>,
    pub selected: bool,
    /// Indented under a project.
    pub child: bool,
    /// Accessible name when it differs from the title.
    pub label: Option<String>,
}

/// One 32-point navigation row. Selection is a fill only; focus adds the ring.
pub fn row(ui: &mut Ui, theme: &Theme, rect: Rect, row: Row<'_>) -> Response {
    let response = ui.interact(rect, row.id, Sense::click());
    let label = row.label.clone().unwrap_or_else(|| row.title.to_owned());
    response.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, row.selected, &label));
    let c = &theme.color;
    let painter = ui.painter();
    let radius = theme.radius.radius_md;
    if row.selected || response.is_pointer_button_down_on() {
        painter.rect_filled(rect, radius, c.surface_selected);
    } else if response.hovered() {
        painter.rect_filled(rect, radius, c.surface_hover);
    }

    let mut x = rect.left() + if row.child { 32.0 } else { 12.0 };
    if let Some(icon) = row.lead {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0)),
            icon,
            16.0,
            c.text_secondary,
        );
        x += 16.0 + theme.space.space_2;
    }
    let right = rect.right() - 10.0;
    let trailing_w = match row.trailing {
        Trailing::None => 0.0,
        Trailing::Keys(keys) => kbd(ui, theme, pos2(right, rect.center().y), keys),
        Trailing::Mark(provider) => {
            let color = if row.selected { c.text_secondary } else { c.text_tertiary };
            icons::paint(
                painter,
                Rect::from_center_size(pos2(right - 8.0, rect.center().y), vec2(16.0, 16.0)),
                provider_mark(provider),
                14.0,
                color,
            );
            16.0
        }
        Trailing::Working(provider) => {
            let center = pos2(right - 8.0, rect.center().y);
            painter.circle_stroke(center, 5.5, egui::Stroke::new(1.5, c.surface_raised));
            let accent = super::provider_color(theme, provider);
            let arc: Vec<_> = (0..=16)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 16.0 * std::f32::consts::PI * 1.2;
                    center + vec2(a.cos(), a.sin()) * 5.5
                })
                .collect();
            painter.add(egui::Shape::line(arc, egui::Stroke::new(1.5, accent)));
            16.0
        }
    };
    let max_w = (right - trailing_w - 8.0 - x).max(0.0);
    let mut job = theme.job(row.title, &theme.text.t_ui, c.text_primary, max_w);
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, c.text_primary);
    focus_ring(ui, theme, &response, radius);
    response
}

/// A collapsible project header.
pub fn project_row(ui: &mut Ui, theme: &Theme, rect: Rect, id: Id, name: &str, open: bool) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(WidgetType::CollapsingHeader, true, name);
        info.selected = Some(open);
        info
    });
    let c = &theme.color;
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, theme.radius.radius_md, c.surface_hover);
    }
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    icons::paint(
        painter,
        Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)),
        chevron,
        14.0,
        c.text_tertiary,
    );
    painter.text(
        pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme.font(&theme.text.t_ui),
        c.text_primary,
    );
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    response
}

/// A section label such as "Projects", with an optional trailing action.
pub fn section_label(ui: &Ui, theme: &Theme, rect: Rect, text: &str) {
    let c = &theme.color;
    let mut job = theme.job(text, &theme.text.t_caption, c.text_tertiary, f32::INFINITY);
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0), galley, c.text_tertiary);
}

pub enum Usage {
    Unavailable,
    /// Percent remaining, as the engine reported it.
    Left(f32),
}

/// Cached usage for one provider window. Unavailable shows the word, never zero.
pub fn usage_meter(ui: &Ui, theme: &Theme, rect: Rect, provider: Provider, window: &str, usage: Usage) {
    let c = &theme.color;
    let painter = ui.painter();
    let name = format!("{} · {window}", super::provider_name(provider));
    painter.text(
        pos2(rect.left(), rect.top() + 8.0),
        Align2::LEFT_CENTER,
        name,
        theme.font(&theme.text.t_caption),
        c.text_secondary,
    );
    match usage {
        Usage::Unavailable => {
            painter.text(
                pos2(rect.right(), rect.top() + 8.0),
                Align2::RIGHT_CENTER,
                "Unavailable",
                theme.font(&theme.text.t_caption),
                c.text_tertiary,
            );
        }
        Usage::Left(left) => {
            painter.text(
                pos2(rect.right(), rect.top() + 8.0),
                Align2::RIGHT_CENTER,
                format!("{left:.0}% left"),
                theme.font(&theme.text.t_caption),
                c.text_secondary,
            );
            let track = Rect::from_min_size(pos2(rect.left(), rect.top() + 22.0), vec2(rect.width(), 3.0));
            painter.rect_filled(track, 2.0, c.surface_raised);
            let mut fill = track;
            fill.set_width(track.width() * (left / 100.0).clamp(0.0, 1.0));
            painter.rect_filled(fill, 2.0, super::provider_color(theme, provider));
        }
    }
}

/// The bottom-left profile row: avatar, name and an upward chevron.
pub fn profile_row(ui: &mut Ui, theme: &Theme, rect: Rect, id: Id, initial: &str, name: &str) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Profile, {name}")));
    let c = &theme.color;
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, theme.radius.radius_md, c.surface_hover);
    }
    let avatar = pos2(rect.left() + 6.0 + theme.size.size_avatar / 2.0, rect.center().y);
    painter.circle_filled(avatar, theme.size.size_avatar / 2.0, c.surface_raised);
    painter.text(
        avatar,
        Align2::CENTER_CENTER,
        initial,
        egui::FontId::new(13.0, egui::FontFamily::Name("geist-600".into())),
        c.text_primary,
    );
    painter.text(
        pos2(avatar.x + theme.size.size_avatar / 2.0 + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme.font(&theme.text.t_ui),
        c.text_primary,
    );
    icons::paint(
        painter,
        Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(16.0, 16.0)),
        Icon::ChevronUp,
        14.0,
        c.text_tertiary,
    );
    focus_ring(ui, theme, &response, theme.radius.radius_md);
    response
}
