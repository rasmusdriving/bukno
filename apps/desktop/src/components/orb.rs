//! WorkingIndicator: the one moving element at the end of the transcript
//! while a run is active. The StreakLabel is the default treatment; the
//! ThinkingOrb and BuildGrid are kept for comparison.
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

/// The cap in use. `BUKNO_ORB_FPS` overrides it for measuring lower rates.
pub fn orb_fps() -> f32 {
    static FPS: OnceLock<f32> = OnceLock::new();
    *FPS.get_or_init(|| {
        std::env::var("BUKNO_ORB_FPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|f| (1.0..=60.0).contains(f))
            .unwrap_or(ORB_FPS)
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// The earlier ThinkingOrb, kept for comparison.
    Orb,
    /// The stepped BuildGrid proposal, kept for comparison.
    Grid,
    /// The StreakLabel: the activity words carry the light. The default.
    Streak,
}

impl Mark {
    /// The StreakLabel is the approved treatment (decision 35). The others
    /// stay selectable for comparison with `BUKNO_WORKING_MARK=orb` or `grid`.
    pub fn from_env() -> Self {
        match std::env::var("BUKNO_WORKING_MARK").ok().as_deref() {
            Some("grid") => Self::Grid,
            Some("orb") => Self::Orb,
            _ => Self::Streak,
        }
    }
}

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
    pub mark: Mark,
}

/// The WorkingIndicator row: orb, label, elapsed time and the latest
/// activity summary. Schedules its own capped repaints while animating.
pub fn working_indicator(ui: &mut Ui, theme: &Theme, rect: Rect, w: &Working<'_>) {
    let painter = ui.painter_at(rect.expand(4.0));
    let now = ui.input(|i| i.time) as f32;
    let animate = w.state == OrbState::Thinking && !w.reduce_motion && is_shown(ui);
    let t = if animate { now } else { 0.0 };
    let orb_center = pos2(rect.left() + 16.0, rect.top() + 18.0);
    let mark = w.mark;
    let grid = mark == Mark::Grid;
    let streak = mark == Mark::Streak;
    let step_seconds = f64::from(theme.duration.step_working);
    let step = if animate { (ui.input(|i| i.time) / step_seconds) as u64 } else { 2 };
    match mark {
        Mark::Grid => super::build_grid::paint(&painter, theme, orb_center, 32.0, step, w.accent, w.state),
        Mark::Orb => paint(&painter, theme, orb_center, 32.0, t, w.accent, w.state),
        Mark::Streak => {}
    }

    let c = &theme.color;
    let x = if streak { rect.left() } else { rect.left() + 32.0 + theme.space.space_3 };
    let label_color = if w.state == OrbState::Waiting { c.text_secondary } else { c.text_primary };
    let label = painter.layout_job(theme.job(w.label, &theme.text.t_ui_strong, label_color, f32::INFINITY));
    let label_w = label.size().x;
    if streak && animate {
        let pass = f64::from(theme.duration.loop_streak);
        let phase = (ui.input(|i| i.time) / pass).fract() as f32;
        let core = super::streak::core_color(c.text_primary, w.accent);
        super::streak::paint(&painter, &label, pos2(x, rect.top() + 6.0), phase, c.text_tertiary, core);
    } else {
        painter.galley(pos2(x, rect.top() + 6.0), label, label_color);
    }
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
    if animate && grid {
        // Wake exactly at the next step boundary; nothing changes in between.
        let next = (step + 1) as f64 * step_seconds - ui.input(|i| i.time);
        ui.ctx().request_repaint_after(Duration::from_secs_f64(next.max(0.0)) + predicted);
    } else if animate {
        let fps =
            if streak && std::env::var_os("BUKNO_ORB_FPS").is_none() { super::streak::STREAK_FPS } else { orb_fps() };
        ui.ctx().request_repaint_after(Duration::from_secs_f32(1.0 / fps) + predicted);
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
