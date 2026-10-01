//! BuildGrid: the proposed working mark (docs/design/system/components/BuildGrid).
//!
//! Nine blocks on a 3 by 3 grid, changed in hard steps of `step-working`.
//! Only the step boundaries need a frame, so the window repaints about six
//! times a second instead of on every animation frame.

use egui::{Color32, Painter, Pos2, Rect, vec2};

use super::orb::OrbState;
use crate::theme::Theme;

/// The thinking trail's path: a winding walk that turns back at each end.
const PATH: [usize; 16] = [0, 1, 2, 5, 4, 3, 6, 7, 8, 7, 6, 3, 4, 5, 2, 1];

/// Block levels for one step: 0 empty, 1 faint trace, 2 placed, 3 newest.
pub fn levels(state: OrbState, step: u64) -> [u8; 9] {
    let mut lv = [0u8; 9];
    match state {
        OrbState::Waiting => lv = [1, 0, 1, 0, 2, 0, 1, 0, 1],
        OrbState::Thinking => {
            for n in 0..3u64 {
                let k = PATH[((step + PATH.len() as u64 * 4 - n) % PATH.len() as u64) as usize];
                lv[k] = lv[k].max(3 - n as u8);
            }
        }
    }
    lv
}

pub fn paint(painter: &Painter, theme: &Theme, center: Pos2, size: f32, step: u64, accent: Color32, state: OrbState) {
    let c = &theme.color;
    let cell = size * 0.1875;
    let gap = size * 0.093_75;
    let origin = center - vec2(size, size) / 2.0;
    let inset = (size - cell * 3.0 - gap * 2.0) / 2.0;
    let newest = if state == OrbState::Waiting { c.text_secondary } else { accent };
    let fills = [c.surface_raised, c.text_disabled, c.text_secondary, newest];
    for (i, level) in levels(state, step).into_iter().enumerate() {
        let min = origin + vec2(inset + (i % 3) as f32 * (cell + gap), inset + (i / 3) as f32 * (cell + gap));
        painter.rect_filled(Rect::from_min_size(min, vec2(cell, cell)), cell * 0.28, fills[level as usize]);
    }
}
