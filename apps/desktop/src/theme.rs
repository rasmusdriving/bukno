//! The approved design tokens as one native theme.
//!
//! `build.rs` turns docs/design/system/tokens.json into typed values; this
//! module adds the fonts and the few derived styles egui needs. Widgets take
//! every colour, size and type style from here, never from literals.

use std::sync::Arc;

use egui::epaint::{CornerRadius, Shape};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Rect, Vec2};

mod generated {
    use egui::Color32;

    #[derive(Clone, Copy, Debug)]
    pub struct TypeStyle {
        pub size: f32,
        pub line_height: f32,
        pub weight: u16,
        /// In points, converted from the token's em value.
        pub letter_spacing: f32,
        pub mono: bool,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct ShadowLayer {
        pub inset: bool,
        pub x: f32,
        pub y: f32,
        pub blur: f32,
        pub spread: f32,
        pub color: Color32,
    }

    include!(concat!(env!("OUT_DIR"), "/tokens.rs"));
}

pub use generated::{Colors, Durations, Easings, Radii, ShadowLayer, Shadows, Sizes, Spacing, TypeStyle, TypeStyles};

const GEIST_400: &[u8] = include_bytes!("../../../assets/fonts/Geist-Regular.ttf");
const GEIST_500: &[u8] = include_bytes!("../../../assets/fonts/Geist-Medium.ttf");
const GEIST_600: &[u8] = include_bytes!("../../../assets/fonts/Geist-SemiBold.ttf");
const GEIST_MONO_400: &[u8] = include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf");

#[derive(Clone, Debug)]
pub struct Theme {
    pub color: Colors,
    pub text: TypeStyles,
    pub space: Spacing,
    pub radius: Radii,
    pub size: Sizes,
    pub shadow: Shadows,
    pub duration: Durations,
    pub easing: Easings,
    /// Text selection fill. Not in tokens.json yet; derived from `focus`.
    /// Recorded as a design gap in the Pass 0 toolkit decision.
    pub selection: Color32,
}

impl Theme {
    pub fn load() -> Self {
        let color = generated::colors();
        let selection = color.focus.gamma_multiply(0.28);
        Self {
            text: generated::type_styles(),
            space: generated::spacing(),
            radius: generated::radius(),
            size: generated::size(),
            shadow: generated::shadows(),
            duration: generated::durations(),
            easing: generated::easings(),
            color,
            selection,
        }
    }

    /// Install fonts and map tokens onto egui's own style.
    pub fn install(&self, ctx: &egui::Context) {
        ctx.set_fonts(font_definitions());
        ctx.set_theme(egui::Theme::Dark);
        let c = &self.color;
        ctx.style_mut_of(egui::Theme::Dark, |style| {
            use egui::{FontFamily as F, TextStyle};
            style.text_styles = [
                (TextStyle::Heading, self.font(&self.text.t_title)),
                (TextStyle::Body, self.font(&self.text.t_body)),
                (TextStyle::Button, self.font(&self.text.t_ui_strong)),
                (TextStyle::Small, self.font(&self.text.t_small)),
                (TextStyle::Monospace, FontId::new(self.text.t_code.size, F::Monospace)),
            ]
            .into();
            let v = &mut style.visuals;
            v.dark_mode = true;
            v.panel_fill = c.surface_canvas;
            v.window_fill = c.surface_popover;
            v.extreme_bg_color = c.surface_composer;
            v.faint_bg_color = c.surface_hover;
            v.code_bg_color = c.surface_code;
            v.hyperlink_color = c.text_primary;
            v.selection.bg_fill = self.selection;
            v.selection.stroke.color = c.text_primary;
            v.text_cursor.stroke = egui::Stroke::new(1.5, c.text_primary);
            v.widgets.noninteractive.fg_stroke.color = c.text_primary;
            v.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
            v.widgets.inactive.fg_stroke.color = c.text_secondary;
            v.widgets.hovered.fg_stroke.color = c.text_primary;
            v.widgets.active.fg_stroke.color = c.text_primary;
            for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                w.bg_stroke = egui::Stroke::NONE;
                w.corner_radius = CornerRadius::same(self.radius.radius_md as u8);
            }
            v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
            v.widgets.hovered.weak_bg_fill = c.surface_hover;
            v.widgets.active.weak_bg_fill = c.surface_selected;
            v.window_stroke = egui::Stroke::NONE;
            v.window_corner_radius = CornerRadius::same(self.radius.radius_lg as u8);
            style.spacing.item_spacing = Vec2::new(self.space.space_2, self.space.space_2);
            style.spacing.button_padding = Vec2::new(self.space.space_3, 6.0);
            style.interaction.selectable_labels = false;
        });
    }

    pub fn font(&self, style: &TypeStyle) -> FontId {
        FontId::new(style.size, family(style))
    }

    pub fn format(&self, style: &TypeStyle, color: Color32) -> TextFormat {
        TextFormat {
            font_id: self.font(style),
            extra_letter_spacing: style.letter_spacing,
            line_height: Some(style.line_height),
            color,
            ..Default::default()
        }
    }

    /// A single-style text layout job, wrapped at `wrap` points.
    pub fn job(&self, text: impl Into<String>, style: &TypeStyle, color: Color32, wrap: f32) -> LayoutJob {
        let mut job = LayoutJob::single_section(text.into(), self.format(style, color));
        job.wrap.max_width = wrap;
        job
    }

    pub fn ease_standard(&self, t: f32) -> f32 {
        cubic_bezier(self.easing.ease_standard, t)
    }

    /// Paint layered drop shadows behind a rounded rectangle. Inset layers are
    /// approximated by a one-point highlight along the top edge.
    pub fn paint_shadow(&self, painter: &Painter, rect: Rect, radius: f32, layers: &[ShadowLayer]) {
        for layer in layers.iter().rev() {
            if layer.inset {
                let top = Rect::from_min_max(
                    rect.min + Vec2::new(radius * 0.6, layer.y.max(0.0)),
                    egui::pos2(rect.max.x - radius * 0.6, rect.min.y + layer.y.max(1.0)),
                );
                painter.rect_filled(top, 0.0, layer.color);
                continue;
            }
            let shadow = egui::epaint::Shadow {
                offset: [layer.x as i8, layer.y as i8],
                blur: layer.blur as u8,
                spread: layer.spread.max(0.0) as u8,
                color: layer.color,
            };
            // Negative spread shrinks the shape before blurring.
            let shape_rect = rect.shrink((-layer.spread).max(0.0));
            painter.add(Shape::from(shadow.as_shape(shape_rect, CornerRadius::same(radius as u8))));
        }
    }

    /// A vertical fade from `color` at the top edge to transparent at the
    /// bottom (or the reverse), used where content scrolls under a header.
    pub fn paint_fade(&self, painter: &Painter, rect: Rect, color: Color32, opaque_top: bool) {
        let (top, bottom) = if opaque_top { (color, Color32::TRANSPARENT) } else { (Color32::TRANSPARENT, color) };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(rect.left_top(), top);
        mesh.colored_vertex(rect.right_top(), top);
        mesh.colored_vertex(rect.left_bottom(), bottom);
        mesh.colored_vertex(rect.right_bottom(), bottom);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        painter.add(Shape::mesh(mesh));
    }

    /// The keyboard focus ring: a canvas-coloured gap, then the focus colour,
    /// following the element's radius.
    pub fn paint_focus_ring(&self, painter: &Painter, rect: Rect, radius: f32) {
        let gap = self.shadow.focus_ring.first().map_or(2.0, |l| l.spread);
        let ring = self.shadow.focus_ring.last().map_or(4.0, |l| l.spread) - gap;
        painter.rect_stroke(
            rect.expand(gap + ring / 2.0),
            CornerRadius::same((radius + gap + ring / 2.0) as u8),
            egui::Stroke::new(ring, self.color.focus),
            egui::StrokeKind::Middle,
        );
    }
}

fn family(style: &TypeStyle) -> FontFamily {
    if style.mono {
        return FontFamily::Monospace;
    }
    match style.weight {
        400 => FontFamily::Proportional,
        500 => FontFamily::Name("geist-500".into()),
        _ => FontFamily::Name("geist-600".into()),
    }
}

/// Geist for interface text and Geist Mono for code, with egui's bundled
/// fonts kept only as fallbacks for symbols Geist lacks.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallback_sans = fonts.families[&FontFamily::Proportional].clone();
    let fallback_mono = fonts.families[&FontFamily::Monospace].clone();
    for (name, bytes) in [
        ("geist-400", GEIST_400),
        ("geist-500", GEIST_500),
        ("geist-600", GEIST_600),
        ("geist-mono-400", GEIST_MONO_400),
    ] {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let with_fallback = |first: &str, rest: &[String]| {
        let mut list = vec![first.to_owned()];
        list.extend(rest.iter().cloned());
        list
    };
    fonts.families.insert(FontFamily::Proportional, with_fallback("geist-400", &fallback_sans));
    fonts.families.insert(FontFamily::Name("geist-500".into()), with_fallback("geist-500", &fallback_sans));
    fonts.families.insert(FontFamily::Name("geist-600".into()), with_fallback("geist-600", &fallback_sans));
    fonts.families.insert(FontFamily::Monospace, with_fallback("geist-mono-400", &fallback_mono));
    fonts
}

/// CSS-style cubic-bezier easing, solved for x by bisection.
pub fn cubic_bezier([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let bez = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}
