//! ThinkingOrb and WorkingIndicator: the one moving element at the end of
//! the transcript while a run is active.
//!
//! The orb is a dotted sphere drawn with circles. It animates through
//! scheduled repaints capped at [`ORB_FPS`]; with reduced motion, or while
//! the run waits for the user, it draws one still frame and asks for none.

use std::sync::OnceLock;
use std::time::Duration;

use egui::{Color32, Painter, Pos2, Rect, Ui, pos2, vec2};

use crate::theme::Theme;

/// Frame-rate cap for the orb. Section 13 starts at 20 frames a second.
pub const ORB_FPS: f32 = 20.0;
const TURN_SECONDS: f32 = 20.0;
const DOTS: usize = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbState {
    Thinking,
    Waiting,
}

fn sphere() -> &'static [[f32; 3]] {
    static POINTS: OnceLock<Vec<[f32; 3]>> = OnceLock::new();
    POINTS.get_or_init(|| {
        // Evenly spread points (Fibonacci sphere).
        let golden = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
        (0..DOTS)
            .map(|i| {
                let y = 1.0 - 2.0 * (i as f32 + 0.5) / DOTS as f32;
                let r = (1.0 - y * y).sqrt();
                let a = golden * i as f32;
                [r * a.cos(), y, r * a.sin()]
            })
            .collect()
    })
}

/// Paint the orb at time `t` seconds.
pub fn paint(painter: &Painter, theme: &Theme, center: Pos2, size: f32, t: f32, accent: Color32, state: OrbState) {
    let radius = size / 2.0 * (1.0 + 0.02 * (t * std::f32::consts::TAU / 3.2).sin());
    let angle = t * std::f32::consts::TAU / TURN_SECONDS;
    let (sin, cos) = angle.sin_cos();
    let band = (t * std::f32::consts::TAU / 4.0).sin() * 0.55;
    let ink = theme.color.text_secondary;
    let mut dots: Vec<(f32, Pos2, f32, Color32)> = sphere()
        .iter()
        .map(|[x, y, z]| {
            let rx = x * cos + z * sin;
            let rz = -x * sin + z * cos;
            let depth = (rz + 1.0) / 2.0;
            let mut color = ink.gamma_multiply(0.25 + 0.75 * depth);
            if state == OrbState::Thinking {
                let near = (1.0 - (y - band).abs() / 0.3).max(0.0);
                if near > 0.0 {
                    color = lerp(color, accent.gamma_multiply(0.4 + 0.6 * depth), near);
                }
            } else {
                color = color.gamma_multiply(0.7);
            }
            let r = radius * (0.04 + 0.05 * depth);
            (rz, pos2(center.x + rx * radius * 0.86, center.y - y * radius * 0.86), r, color)
        })
        .collect();
    // Far dots first, so near dots sit on top.
    dots.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, pos, r, color) in dots {
        painter.circle_filled(pos, r, color);
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

pub struct Working<'a> {
    pub accent: Color32,
    pub label: &'a str,
    pub elapsed: Option<f64>,
    pub summary: Option<&'a str>,
    pub state: OrbState,
    pub reduce_motion: bool,
}

/// The WorkingIndicator row: orb, label, elapsed time and the latest
/// activity summary. Schedules its own capped repaints while animating.
pub fn working_indicator(ui: &mut Ui, theme: &Theme, rect: Rect, w: &Working<'_>) {
    let painter = ui.painter_at(rect.expand(4.0));
    let now = ui.input(|i| i.time) as f32;
    let animate = w.state == OrbState::Thinking && !w.reduce_motion && is_shown(ui);
    let t = if animate { now } else { 0.0 };
    let orb_center = pos2(rect.left() + 16.0, rect.top() + 18.0);
    paint(&painter, theme, orb_center, 32.0, t, w.accent, w.state);

    let c = &theme.color;
    let x = rect.left() + 32.0 + theme.space.space_3;
    let label_color = if w.state == OrbState::Waiting { c.text_secondary } else { c.text_primary };
    let label = painter.layout_job(theme.job(w.label, &theme.text.t_ui_strong, label_color, f32::INFINITY));
    let label_w = label.size().x;
    painter.galley(pos2(x, rect.top() + 6.0), label, label_color);
    if let Some(elapsed) = w.elapsed {
        let time =
            painter.layout_job(theme.job(format_elapsed(elapsed), &theme.text.t_small, c.text_tertiary, f32::INFINITY));
        painter.galley(pos2(x + label_w + 8.0, rect.top() + 7.0), time, c.text_tertiary);
    }
    if let Some(summary) = w.summary {
        let galley = painter.layout_job(theme.job(summary, &theme.text.t_small, c.text_secondary, rect.width() - 48.0));
        painter.galley(pos2(x, rect.top() + 28.0), galley, c.text_secondary);
    }

    // egui shortens every scheduled repaint by its predicted frame time
    // (about 16 ms), which turned a 50 ms request into roughly 30 fps.
    // Add it back so the cap holds.
    let predicted = Duration::from_secs_f32(ui.input(|i| i.predicted_dt).clamp(0.0, 0.1));
    if animate {
        ui.ctx().request_repaint_after(Duration::from_secs_f32(1.0 / ORB_FPS) + predicted);
    } else if w.elapsed.is_some() && w.state == OrbState::Thinking {
        // The still variant only needs the clock to tick.
        ui.ctx().request_repaint_after(Duration::from_secs(1) + predicted);
    }
    let _ = vec2;
}

/// Hidden and minimised windows never schedule animation frames.
pub fn is_shown(ui: &Ui) -> bool {
    ui.input(|i| {
        let v = i.viewport();
        !v.minimized.unwrap_or(false) && !v.occluded.unwrap_or(false)
    })
}

pub fn format_elapsed(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s < 60 { format!("{s}s") } else { format!("{}m {}s", s / 60, s % 60) }
}
