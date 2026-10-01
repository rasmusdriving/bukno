//! StreakLabel: the proposed working treatment
//! (docs/design/system/components/StreakLabel). The activity words in
//! `text-tertiary` with a soft light passing through them once per
//! `loop-streak`. The light is applied per vertex of the laid-out text, so
//! the gradient runs smoothly across each letter.

use std::sync::Arc;

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Galley, Painter, Pos2, Shape, TextureId};

/// Frames a second for the streak. Soft and slow, so it stays smooth at a
/// lower rate than the orb needs.
pub const STREAK_FPS: f32 = 15.0;
/// Width of the light in points, whatever the length of the text.
const BAND_WIDTH: f32 = 120.0;

/// Paint `galley` at `origin` with the light at `phase` (0 to 1) of its pass.
pub fn paint(painter: &Painter, galley: &Arc<Galley>, origin: Pos2, phase: f32, rest: Color32, core: Color32) {
    let width = galley.size().x.max(1.0);
    // Same geometry as the CSS reference: a fixed-width band that starts
    // fully left of the words and ends fully right of them.
    let half = BAND_WIDTH / 2.0;
    let center = -half + phase * (width + BAND_WIDTH);
    let atlas = painter.ctx().fonts(|f| f.font_image_size());
    let uv_scale = egui::vec2(1.0 / atlas[0] as f32, 1.0 / atlas[1] as f32);
    let mut mesh = Mesh::with_texture(TextureId::default());
    for row in &galley.rows {
        let row_origin = origin + row.pos.to_vec2();
        let base = mesh.vertices.len() as u32;
        mesh.indices.extend(row.visuals.mesh.indices.iter().map(|i| i + base));
        mesh.vertices.extend(row.visuals.mesh.vertices.iter().map(|v| {
            let pos = row_origin + v.pos.to_vec2();
            // The CSS gradient runs at 100 degrees; lean the band slightly.
            let x = pos.x - origin.x - (pos.y - origin.y - galley.size().y / 2.0) * 0.176;
            let d = ((x - center).abs() / half).min(1.0);
            let t = (1.0 - d) * (1.0 - d) * (3.0 - 2.0 * (1.0 - d)); // smoothstep of closeness
            Vertex { pos, uv: (v.uv.to_vec2() * uv_scale).to_pos2(), color: lerp(rest, core, t) }
        }));
    }
    painter.add(Shape::mesh(mesh));
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

/// The light's core: `text-primary` with just over a quarter of the provider colour.
pub fn core_color(text_primary: Color32, provider: Color32) -> Color32 {
    lerp(text_primary, provider, 0.28)
}
