//! Icons on the design's 16-point grid with a 1.5-point round stroke, drawn
//! with the renderer's own primitives in the current colour.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    NewChat,
    Search,
    Plus,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    Sidebar,
    Folder,
    Branch,
    Bolt,
    Stop,
    ArrowUp,
    Dots,
    Copy,
    Close,
    /// Filled circle with an exclamation mark: needs you.
    Alert,
    Check,
    Refresh,
    /// Placeholder Codex mark from the design system. Replace before release.
    Hexagon,
    /// Placeholder Claude mark from the design system. Replace before release.
    Diamond,
}

/// Paint `icon` centred in `rect`, scaled from the 16-point grid to `size`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, size: f32, color: Color32) {
    let s = size / 16.0;
    let o = rect.center() - vec2(8.0, 8.0) * s;
    let p = |x: f32, y: f32| -> Pos2 { o + vec2(x, y) * s };
    let stroke = Stroke::new(1.5 * s.max(0.75), color);
    let line = |pts: &[Pos2]| {
        painter.add(Shape::line(pts.to_vec(), stroke));
    };
    let closed = |pts: Vec<Pos2>| {
        painter.add(Shape::closed_line(pts, stroke));
    };
    match icon {
        Icon::NewChat => {
            line(&[
                p(8.0, 2.75),
                p(3.25, 2.75),
                p(2.5, 3.5),
                p(2.5, 12.75),
                p(3.25, 13.5),
                p(12.5, 13.5),
                p(13.25, 12.75),
                p(13.25, 8.0),
            ]);
            line(&[p(11.75, 1.9), p(14.1, 4.25), p(8.5, 9.85), p(5.9, 10.4), p(6.45, 7.8), p(11.75, 1.9)]);
        }
        Icon::Search => {
            painter.circle_stroke(p(7.0, 7.0), 4.75 * s, stroke);
            line(&[p(10.5, 10.5), p(13.75, 13.75)]);
        }
        Icon::Plus => {
            line(&[p(8.0, 3.0), p(8.0, 13.0)]);
            line(&[p(3.0, 8.0), p(13.0, 8.0)]);
        }
        Icon::ChevronDown => line(&[p(4.5, 6.25), p(8.0, 9.75), p(11.5, 6.25)]),
        Icon::ChevronUp => line(&[p(4.5, 9.75), p(8.0, 6.25), p(11.5, 9.75)]),
        Icon::ChevronRight => line(&[p(6.25, 4.5), p(9.75, 8.0), p(6.25, 11.5)]),
        Icon::Sidebar => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.25, 3.0), p(13.75, 13.0)),
                2.0 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(&[p(6.25, 3.0), p(6.25, 13.0)]);
        }
        Icon::Folder => {
            closed(vec![p(2.25, 4.0), p(6.0, 4.0), p(7.5, 5.5), p(13.75, 5.5), p(13.75, 12.5), p(2.25, 12.5)])
        }
        Icon::Branch => {
            painter.circle_stroke(p(5.0, 3.75), 1.6 * s, stroke);
            painter.circle_stroke(p(5.0, 12.25), 1.6 * s, stroke);
            painter.circle_stroke(p(11.0, 5.25), 1.6 * s, stroke);
            line(&[p(5.0, 5.4), p(5.0, 10.6)]);
            line(&[p(11.0, 6.9), p(11.0, 7.5), p(5.2, 10.4)]);
        }
        Icon::Bolt => {
            closed(vec![p(9.0, 1.75), p(3.75, 9.0), p(7.75, 9.0), p(7.0, 14.25), p(12.25, 7.0), p(8.25, 7.0)])
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_center_size(rect.center(), vec2(8.0, 8.0) * s), 1.5 * s, color);
        }
        Icon::ArrowUp => {
            line(&[p(8.0, 13.0), p(8.0, 3.5)]);
            line(&[p(4.0, 7.5), p(8.0, 3.5), p(12.0, 7.5)]);
        }
        Icon::Dots => {
            for x in [3.5, 8.0, 12.5] {
                painter.circle_filled(p(x, 8.0), 1.1 * s, color);
            }
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(p(5.5, 5.5), p(13.5, 13.5)),
                1.75 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(&[p(10.5, 3.25), p(10.5, 2.5), p(3.25, 2.5), p(2.5, 3.25), p(2.5, 10.5), p(3.25, 10.5)]);
        }
        Icon::Close => {
            line(&[p(4.0, 4.0), p(12.0, 12.0)]);
            line(&[p(12.0, 4.0), p(4.0, 12.0)]);
        }
        Icon::Alert => {
            painter.circle_filled(p(8.0, 8.0), 6.25 * s, color);
            let ink = Color32::from_rgb(0x23, 0x22, 0x20);
            painter.line_segment([p(8.0, 4.6), p(8.0, 8.9)], Stroke::new(1.6 * s, ink));
            painter.circle_filled(p(8.0, 11.2), 0.95 * s, ink);
        }
        Icon::Check => line(&[p(3.5, 8.25), p(6.75, 11.5), p(12.5, 4.75)]),
        Icon::Refresh => {
            let arc: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = -0.35 + i as f32 / 20.0 * 5.2;
                    p(8.0 + 5.0 * a.cos(), 8.0 + 5.0 * a.sin())
                })
                .collect();
            line(&arc);
            line(&[p(13.0, 2.75), p(13.0, 6.25), p(9.5, 6.25)]);
        }
        Icon::Hexagon => {
            let pts = (0..6)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_3 * i as f32 - std::f32::consts::FRAC_PI_2;
                    p(8.0 + 6.0 * a.cos(), 8.0 + 6.0 * a.sin())
                })
                .collect();
            closed(pts);
            painter.circle_filled(p(8.0, 8.0), 1.6 * s, color);
        }
        Icon::Diamond => {
            closed(vec![p(8.0, 1.75), p(14.25, 8.0), p(8.0, 14.25), p(1.75, 8.0)]);
            painter.circle_filled(p(8.0, 8.0), 1.6 * s, color);
        }
    }
}
