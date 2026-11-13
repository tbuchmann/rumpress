//! Highlight / pen mode: freehand strokes over the current slide.
//!
//! Strokes are stored in normalized 0..1 slide coordinates, so they render
//! identically on the presenter's slide widget and on the presentation
//! (beamer) viewport regardless of each viewport's slide size.

use egui::{Color32, Pos2, Rect, Shape, pos2};

/// Stroke color for highlight annotations.
pub const STROKE_COLOR: Color32 = Color32::RED;

/// Stroke width in points for highlight annotations.
pub const STROKE_WIDTH: f32 = 3.0;

/// Freehand annotations for the current slide: a list of strokes, each a
/// list of points in normalized 0..1 slide coordinates.
#[derive(Default)]
pub struct Strokes {
    pub strokes: Vec<Vec<Pos2>>,
    /// Whether the pen was down at the last [`Strokes::push_point`]; a
    /// false→true transition starts a new stroke.
    pen_down: bool,
}

impl Strokes {
    /// Adds a point. While `drawing` is held true the points extend the
    /// current stroke; a `drawing` transition from false to true starts a
    /// new one. Calls with `drawing == false` only record the pen-up.
    pub fn push_point(&mut self, pos: Pos2, drawing: bool) {
        if drawing {
            if !self.pen_down || self.strokes.is_empty() {
                self.strokes.push(Vec::new());
            }
            self.strokes
                .last_mut()
                .expect("a stroke was just pushed")
                .push(pos);
        }
        self.pen_down = drawing;
    }

    /// Drops all strokes.
    pub fn clear(&mut self) {
        self.strokes.clear();
    }

    /// `true` when there is nothing to draw.
    #[allow(dead_code)] // part of the brief's public surface; used by tests
    pub fn is_empty(&self) -> bool {
        self.strokes.iter().all(Vec::is_empty)
    }
}

/// Draws `strokes` over `rect` (the on-screen slide rectangle), mapping
/// normalized coordinates into it.
pub fn draw(painter: &egui::Painter, rect: Rect, strokes: &Strokes) {
    for stroke in &strokes.strokes {
        let points: Vec<Pos2> = stroke
            .iter()
            .map(|p| {
                pos2(
                    rect.left() + p.x * rect.width(),
                    rect.top() + p.y * rect.height(),
                )
            })
            .collect();
        painter.add(Shape::line(
            points,
            egui::Stroke::new(STROKE_WIDTH, STROKE_COLOR),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_point_starts_new_stroke_then_extends() {
        let mut s = Strokes::default();
        s.push_point(egui::pos2(0.1, 0.1), true);
        s.push_point(egui::pos2(0.2, 0.1), true);
        assert_eq!(s.strokes.len(), 1);
        assert_eq!(s.strokes[0].len(), 2);
        s.push_point(egui::pos2(0.3, 0.3), false); // pen up
        s.push_point(egui::pos2(0.4, 0.4), true); // new stroke
        assert_eq!(s.strokes.len(), 2);
        s.clear();
        assert!(s.strokes.is_empty());
    }

    #[test]
    fn is_empty_ignores_empty_stroke_shells() {
        let mut s = Strokes::default();
        assert!(s.is_empty());
        s.push_point(egui::pos2(0.5, 0.5), false); // pen up: no stroke created
        assert!(s.is_empty());
        s.push_point(egui::pos2(0.5, 0.5), true);
        assert!(!s.is_empty());
    }

    #[test]
    fn draw_maps_normalized_points_into_the_rect() {
        let mut s = Strokes::default();
        s.push_point(pos2(0.0, 0.0), true);
        s.push_point(pos2(1.0, 1.0), true);
        let rect = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        let painter = egui::Painter::new(
            egui::Context::default(),
            egui::LayerId::new(egui::Order::Background, egui::Id::new("test")),
            rect,
        );
        // Just exercise the mapping path; shape internals are egui's business.
        draw(&painter, rect, &s);
    }
}
