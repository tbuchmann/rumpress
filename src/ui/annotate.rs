//! Annotation tools over the current slide: laser pointer, pen, highlighter
//! (freehand marker and filled area), shapes, and an eraser.
//!
//! Everything is stored in normalized 0..1 slide coordinates, so it renders
//! identically on the presenter's slide widget and on the beamer viewport
//! regardless of each viewport's slide size. Annotations are kept per PDF
//! page, so they are still there when you come back to a page.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// An annotation tool the presenter can select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Laser,
    Pen,
    Highlighter,
    /// Filled translucent rectangle: marks a region of the slide.
    Area,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Eraser,
}

impl Tool {
    /// All tools, in toolbar order.
    pub const ALL: [Tool; 9] = [
        Tool::Laser,
        Tool::Pen,
        Tool::Highlighter,
        Tool::Area,
        Tool::Line,
        Tool::Arrow,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Eraser,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Laser => "Laser",
            Tool::Pen => "Pen",
            Tool::Highlighter => "Marker",
            Tool::Area => "Area",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Rect => "Box",
            Tool::Ellipse => "Oval",
            Tool::Eraser => "Eraser",
        }
    }

    /// The key that selects this tool (shown in tooltips and help).
    pub fn key_hint(self) -> char {
        match self {
            Tool::Laser => 'L',
            Tool::Pen => 'P',
            Tool::Highlighter => 'H',
            Tool::Area => 'F',
            Tool::Line => 'N',
            Tool::Arrow => 'A',
            Tool::Rect => 'R',
            Tool::Ellipse => 'O',
            Tool::Eraser => 'E',
        }
    }

    fn index(self) -> usize {
        Tool::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// Colors this tool can cycle through (empty for the eraser).
    pub fn palette(self) -> &'static [Color32] {
        match self {
            Tool::Laser => &LASER_COLORS,
            Tool::Highlighter | Tool::Area => &MARKER_COLORS,
            Tool::Eraser => &[],
            _ => &PEN_COLORS,
        }
    }
}

const PEN_COLORS: [Color32; 5] = [
    Color32::from_rgb(230, 30, 30),
    Color32::from_rgb(43, 108, 255),
    Color32::from_rgb(30, 160, 70),
    Color32::from_rgb(255, 140, 0),
    Color32::from_rgb(20, 20, 20),
];
const MARKER_COLORS: [Color32; 4] = [
    Color32::from_rgb(255, 235, 0),
    Color32::from_rgb(80, 230, 120),
    Color32::from_rgb(255, 105, 180),
    Color32::from_rgb(255, 170, 60),
];
const LASER_COLORS: [Color32; 2] = [
    Color32::from_rgb(255, 30, 30),
    Color32::from_rgb(0, 220, 70),
];

/// Stroke widths (points) the width key cycles through.
pub const WIDTHS: [f32; 3] = [2.0, 4.0, 8.0];
/// Labels for [`WIDTHS`], for the toolbar.
pub const WIDTH_LABELS: [&str; 3] = ["S", "M", "L"];

/// The highlighter marker is this much wider than a pen of the same setting.
const MARKER_WIDTH_FACTOR: f32 = 4.0;
/// Alpha of marker strokes and filled areas.
const MARKER_ALPHA: u8 = 110;

/// The current color/width choices: one color per tool, one shared width.
#[derive(Debug, Clone, Default)]
pub struct Style {
    color_idx: [usize; Tool::ALL.len()],
    width_idx: usize,
}

impl Style {
    pub fn color(&self, tool: Tool) -> Color32 {
        let palette = tool.palette();
        if palette.is_empty() {
            return Color32::TRANSPARENT;
        }
        palette[self.color_idx[tool.index()] % palette.len()]
    }

    pub fn color_index(&self, tool: Tool) -> usize {
        let len = tool.palette().len();
        if len == 0 {
            0
        } else {
            self.color_idx[tool.index()] % len
        }
    }

    pub fn set_color_index(&mut self, tool: Tool, idx: usize) {
        self.color_idx[tool.index()] = idx;
    }

    pub fn cycle_color(&mut self, tool: Tool) {
        let len = tool.palette().len();
        if len > 0 {
            let i = &mut self.color_idx[tool.index()];
            *i = (*i + 1) % len;
        }
    }

    pub fn width(&self) -> f32 {
        WIDTHS[self.width_idx % WIDTHS.len()]
    }

    pub fn width_index(&self) -> usize {
        self.width_idx % WIDTHS.len()
    }

    pub fn set_width_index(&mut self, idx: usize) {
        self.width_idx = idx % WIDTHS.len();
    }

    pub fn cycle_width(&mut self) {
        self.width_idx = (self.width_idx + 1) % WIDTHS.len();
    }
}

/// The geometry of one annotation, in normalized slide coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Freehand(Vec<Pos2>),
    Line(Pos2, Pos2),
    Arrow(Pos2, Pos2),
    Rect(Pos2, Pos2),
    Ellipse(Pos2, Pos2),
    /// Filled rectangle.
    Area(Pos2, Pos2),
}

/// One finished (or in-progress) annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub kind: Kind,
    pub color: Color32,
    pub width: f32,
    /// Drawn as a wide translucent marker stroke.
    pub marker: bool,
}

/// Scales x by the slide aspect ratio, so distances and angles computed on
/// the result are the same as on screen (normalized space is stretched).
fn corrected(p: Pos2, aspect: f32) -> Pos2 {
    pos2(p.x * aspect, p.y)
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

impl Kind {
    /// The outline of this shape as polylines in normalized coordinates.
    /// `aspect` is slide width / height, used to build undistorted
    /// arrowheads and ellipses.
    fn polylines(&self, aspect: f32) -> Vec<Vec<Pos2>> {
        let aspect = aspect.max(0.1);
        match self {
            Kind::Freehand(points) => vec![points.clone()],
            Kind::Line(a, b) => vec![vec![*a, *b]],
            Kind::Arrow(a, b) => {
                let mut out = vec![vec![*a, *b]];
                let (ca, cb) = (corrected(*a, aspect), corrected(*b, aspect));
                let d = cb - ca;
                let len = d.length();
                if len > 1e-4 {
                    let dir = d / len;
                    // Head length: 4% of the slide height, at most half the shaft.
                    let head = 0.04_f32.min(len * 0.5);
                    let ang = 25.0_f32.to_radians();
                    for s in [ang, -ang] {
                        let (sin, cos) = s.sin_cos();
                        let rot = vec2(dir.x * cos - dir.y * sin, dir.x * sin + dir.y * cos);
                        let tip = cb - rot * head;
                        out.push(vec![*b, pos2(tip.x / aspect, tip.y)]);
                    }
                }
                out
            }
            Kind::Rect(a, b) | Kind::Area(a, b) => {
                let r = Rect::from_two_pos(*a, *b);
                vec![vec![
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ]]
            }
            Kind::Ellipse(a, b) => {
                let r = Rect::from_two_pos(*a, *b);
                let (c, rx, ry) = (r.center(), r.width() / 2.0, r.height() / 2.0);
                const STEPS: usize = 48;
                let pts = (0..=STEPS)
                    .map(|i| {
                        let t = i as f32 / STEPS as f32 * std::f32::consts::TAU;
                        pos2(c.x + rx * t.cos(), c.y + ry * t.sin())
                    })
                    .collect();
                vec![pts]
            }
        }
    }

    /// Whether `p` (normalized) is within `radius` (in slide-height units) of
    /// this shape, for the eraser. Filled areas also count when `p` is inside.
    fn hit(&self, p: Pos2, radius: f32, aspect: f32) -> bool {
        if let Kind::Area(a, b) = self
            && Rect::from_two_pos(*a, *b).contains(p)
        {
            return true;
        }
        let pc = corrected(p, aspect);
        self.polylines(aspect)
            .iter()
            .any(|line| match line.as_slice() {
                [] => false,
                [only] => (corrected(*only, aspect) - pc).length() <= radius,
                _ => line.windows(2).any(|w| {
                    dist_to_segment(pc, corrected(w[0], aspect), corrected(w[1], aspect)) <= radius
                }),
            })
    }
}

impl Annotation {
    /// A click without a drag (or a one-point scribble) is not worth keeping.
    fn is_meaningful(&self) -> bool {
        const MIN_SIZE: f32 = 0.005;
        match &self.kind {
            Kind::Freehand(points) => points.len() >= 2,
            Kind::Line(a, b)
            | Kind::Arrow(a, b)
            | Kind::Rect(a, b)
            | Kind::Ellipse(a, b)
            | Kind::Area(a, b) => (*b - *a).length() > MIN_SIZE,
        }
    }
}

/// All annotations, per PDF page, plus the shape currently being drawn.
#[derive(Default)]
pub struct Annotations {
    pages: HashMap<u16, Vec<Annotation>>,
    draft: Option<Annotation>,
}

impl Annotations {
    pub fn on_page(&self, page: u16) -> &[Annotation] {
        self.pages.get(&page).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The shape being dragged out right now, if any.
    pub fn draft(&self) -> Option<&Annotation> {
        self.draft.as_ref()
    }

    /// Starts drawing with `tool` at `pos`. Tools that don't draw (laser,
    /// eraser) do nothing.
    pub fn begin(&mut self, tool: Tool, style: &Style, pos: Pos2) {
        let kind = match tool {
            Tool::Pen | Tool::Highlighter => Kind::Freehand(vec![pos]),
            Tool::Line => Kind::Line(pos, pos),
            Tool::Arrow => Kind::Arrow(pos, pos),
            Tool::Rect => Kind::Rect(pos, pos),
            Tool::Ellipse => Kind::Ellipse(pos, pos),
            Tool::Area => Kind::Area(pos, pos),
            Tool::Laser | Tool::Eraser => return,
        };
        self.draft = Some(Annotation {
            kind,
            color: style.color(tool),
            width: style.width(),
            marker: matches!(tool, Tool::Highlighter | Tool::Area),
        });
    }

    /// Extends the shape being drawn to `pos`.
    pub fn update(&mut self, pos: Pos2) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        match &mut draft.kind {
            Kind::Freehand(points) => {
                if points.last() != Some(&pos) {
                    points.push(pos);
                }
            }
            Kind::Line(_, b)
            | Kind::Arrow(_, b)
            | Kind::Rect(_, b)
            | Kind::Ellipse(_, b)
            | Kind::Area(_, b) => *b = pos,
        }
    }

    /// Ends the drag: stores the shape on `page` if it is meaningful.
    /// Returns whether something was stored.
    pub fn finish(&mut self, page: u16) -> bool {
        let Some(draft) = self.draft.take() else {
            return false;
        };
        if draft.is_meaningful() {
            self.pages.entry(page).or_default().push(draft);
            true
        } else {
            false
        }
    }

    /// Abandons the shape being drawn (e.g. on a page change or tool switch).
    pub fn cancel(&mut self) {
        self.draft = None;
    }

    /// Removes the most recent annotation on `page`.
    pub fn undo(&mut self, page: u16) -> bool {
        self.pages
            .get_mut(&page)
            .is_some_and(|items| items.pop().is_some())
    }

    /// Removes every annotation on `page`.
    pub fn clear(&mut self, page: u16) {
        self.pages.remove(&page);
    }

    /// Removes every annotation within `radius` of `pos` on `page`.
    /// Returns whether anything was removed.
    pub fn erase_at(&mut self, page: u16, pos: Pos2, radius: f32, aspect: f32) -> bool {
        let Some(items) = self.pages.get_mut(&page) else {
            return false;
        };
        let before = items.len();
        items.retain(|a| !a.kind.hit(pos, radius, aspect));
        items.len() != before
    }

    /// `true` when `page` has no annotations.
    pub fn is_empty(&self, page: u16) -> bool {
        self.on_page(page).is_empty()
    }
}

/// How long the laser's trail takes to fade out.
pub const LASER_TRAIL: Duration = Duration::from_millis(450);

/// The virtual laser pointer: a dot at the pointer position with a short
/// fading trail. Positions are normalized slide coordinates.
#[derive(Default)]
pub struct Laser {
    head: Option<Pos2>,
    trail: Vec<(Pos2, Instant)>,
}

impl Laser {
    /// The pointer is at `pos` (inside the slide).
    pub fn point(&mut self, pos: Pos2, now: Instant) {
        self.head = Some(pos);
        if self.trail.last().map(|(p, _)| *p) != Some(pos) {
            self.trail.push((pos, now));
        }
        self.prune(now);
    }

    /// The pointer left the slide: the dot disappears, the trail fades.
    pub fn leave(&mut self) {
        self.head = None;
    }

    /// Drops trail points older than [`LASER_TRAIL`].
    pub fn prune(&mut self, now: Instant) {
        self.trail
            .retain(|(_, t)| now.saturating_duration_since(*t) < LASER_TRAIL);
    }

    /// `true` while something is visible, so the UI should keep repainting.
    pub fn is_animating(&self) -> bool {
        !self.trail.is_empty()
    }

    pub fn head(&self) -> Option<Pos2> {
        self.head
    }

    /// Hides the laser immediately.
    pub fn reset(&mut self) {
        self.head = None;
        self.trail.clear();
    }
}

fn to_screen(rect: Rect, p: Pos2) -> Pos2 {
    pos2(
        rect.left() + p.x * rect.width(),
        rect.top() + p.y * rect.height(),
    )
}

fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// Draws the annotations of one page, and the in-progress shape, over `rect`
/// (the on-screen slide rectangle).
pub fn draw_page(
    painter: &egui::Painter,
    rect: Rect,
    items: &[Annotation],
    draft: Option<&Annotation>,
) {
    for a in items.iter().chain(draft) {
        draw_one(painter, rect, a);
    }
}

fn draw_one(painter: &egui::Painter, rect: Rect, a: &Annotation) {
    let aspect = rect.width() / rect.height().max(1.0);
    if let Kind::Area(p, q) = &a.kind {
        let r = Rect::from_two_pos(to_screen(rect, *p), to_screen(rect, *q));
        painter.rect_filled(r, 0.0, with_alpha(a.color, MARKER_ALPHA));
        return;
    }
    let (color, width) = if a.marker {
        (
            with_alpha(a.color, MARKER_ALPHA),
            a.width * MARKER_WIDTH_FACTOR,
        )
    } else {
        (a.color, a.width)
    };
    for line in a.kind.polylines(aspect) {
        match line.as_slice() {
            [] => {}
            [only] => {
                painter.circle_filled(to_screen(rect, *only), width / 2.0, color);
            }
            _ => {
                let points = line.iter().map(|p| to_screen(rect, *p)).collect();
                painter.add(Shape::line(points, Stroke::new(width, color)));
            }
        }
    }
}

/// Longest the laser tail may get, in dot radii (regardless of speed).
const MAX_TAIL_RADII: f32 = 9.0;

/// Limits the tail to `max_len` pixels and fades it along that length.
///
/// `points` are screen positions, oldest first, each with its age (0..1).
/// The result has the same shape, but each value is now the combined fade:
/// the larger of the age and the distance from the head as a fraction of
/// `max_len`. Points beyond `max_len` are cut off, with the cut placed
/// exactly on the segment crossing it (fade 1, i.e. invisible), so the tail
/// ends smoothly instead of popping.
fn tail_points(points: &[(Pos2, f32)], max_len: f32) -> Vec<(Pos2, f32)> {
    let max_len = max_len.max(1e-3);
    let mut out: Vec<(Pos2, f32)> = Vec::new(); // built newest first
    let mut dist = 0.0;
    let mut prev: Option<Pos2> = None;
    for (p, age) in points.iter().rev() {
        if let Some(q) = prev {
            let seg = (q - *p).length();
            if dist + seg >= max_len {
                let t = ((max_len - dist) / seg.max(1e-6)).clamp(0.0, 1.0);
                out.push((q + (*p - q) * t, 1.0));
                break;
            }
            dist += seg;
        }
        let fade = age.max(dist / max_len).clamp(0.0, 1.0);
        out.push((*p, fade));
        prev = Some(*p);
        if fade >= 1.0 {
            break;
        }
    }
    out.reverse();
    out
}

/// Builds the laser's tail as one tapered, fading ribbon: a triangle strip
/// whose width and opacity shrink with the age of each point. Being a single
/// mesh with per-vertex colors, it has no overlap artifacts (unlike a row of
/// translucent dots). `points` are screen positions, oldest first, each with
/// its fade in 0..1 (0 = fresh, 1 = gone; see [`tail_points`]). Returns `None` when there is nothing to draw.
fn trail_strip(points: &[(Pos2, f32)], radius: f32, color: Color32) -> Option<egui::Mesh> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    let mut mesh = egui::Mesh::default();
    let mut tangent = vec2(1.0, 0.0);
    for (i, (p, age)) in points.iter().enumerate() {
        let before = points[i.saturating_sub(1)].0;
        let after = points[(i + 1).min(n - 1)].0;
        let d = after - before;
        if d.length() > 1e-3 {
            tangent = d.normalized();
        }
        let normal = vec2(-tangent.y, tangent.x);
        let fresh = (1.0 - age).clamp(0.0, 1.0);
        let half_width = radius * 0.8 * fresh;
        let alpha = (170.0 * fresh * fresh) as u8;
        let c = with_alpha(color, alpha);
        mesh.colored_vertex(*p + normal * half_width, c);
        mesh.colored_vertex(*p - normal * half_width, c);
    }
    for i in 0..(n as u32 - 1) {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(b, d, c);
    }
    Some(mesh)
}

/// Draws the laser dot and its trail over `rect`. `width` is the style's
/// stroke width, which scales the dot size.
pub fn draw_laser(
    painter: &egui::Painter,
    rect: Rect,
    laser: &Laser,
    color: Color32,
    width: f32,
    now: Instant,
) {
    let radius = (rect.width() * 0.0016 * width).clamp(3.0, 20.0);

    // The tail: points oldest -> newest (the live head last), with their age
    // as a 0..1 fraction of the fade time.
    let fade = LASER_TRAIL.as_secs_f32();
    let mut points: Vec<(Pos2, f32)> = laser
        .trail
        .iter()
        .map(|(p, t)| {
            let age = now.saturating_duration_since(*t).as_secs_f32() / fade;
            (to_screen(rect, *p), age)
        })
        .filter(|(_, age)| *age < 1.0)
        .collect();
    if let Some(head) = laser.head() {
        let head = to_screen(rect, head);
        if points.last().map(|(p, _)| *p) != Some(head) {
            points.push((head, 0.0));
        }
    }
    // Fade by distance as well as by age, so a fast move does not stretch
    // the tail across the slide: it never gets longer than MAX_TAIL_RADII
    // dot radii.
    let points = tail_points(&points, radius * MAX_TAIL_RADII);
    if let Some(mesh) = trail_strip(&points, radius, color) {
        painter.add(Shape::mesh(mesh));
    }

    if let Some(head) = laser.head() {
        let c = to_screen(rect, head);
        painter.circle_filled(c, radius * 1.8, with_alpha(color, 60));
        painter.circle_filled(c, radius, color);
        painter.circle_filled(c, radius * 0.4, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style::default()
    }

    #[test]
    fn pen_drag_creates_one_freehand_annotation() {
        let mut a = Annotations::default();
        a.begin(Tool::Pen, &style(), pos2(0.1, 0.1));
        a.update(pos2(0.2, 0.1));
        a.update(pos2(0.3, 0.2));
        assert!(a.draft().is_some());
        assert!(a.finish(3));
        assert!(a.draft().is_none());
        assert_eq!(a.on_page(3).len(), 1);
        assert!(a.on_page(4).is_empty(), "annotations are per page");
        assert!(!a.on_page(3)[0].marker);
    }

    #[test]
    fn a_click_without_drag_is_discarded() {
        let mut a = Annotations::default();
        a.begin(Tool::Pen, &style(), pos2(0.5, 0.5));
        assert!(!a.finish(0), "single-point scribble kept");
        a.begin(Tool::Rect, &style(), pos2(0.5, 0.5));
        assert!(!a.finish(0), "zero-size shape kept");
        assert!(a.is_empty(0));
    }

    #[test]
    fn shapes_follow_the_drag_end_point() {
        let mut a = Annotations::default();
        a.begin(Tool::Arrow, &style(), pos2(0.1, 0.1));
        a.update(pos2(0.5, 0.4));
        a.update(pos2(0.6, 0.7));
        assert_eq!(
            a.draft().unwrap().kind,
            Kind::Arrow(pos2(0.1, 0.1), pos2(0.6, 0.7))
        );
        assert!(a.finish(0));
    }

    #[test]
    fn highlighter_and_area_are_markers() {
        let mut a = Annotations::default();
        a.begin(Tool::Highlighter, &style(), pos2(0.1, 0.1));
        assert!(a.draft().unwrap().marker);
        a.cancel();
        a.begin(Tool::Area, &style(), pos2(0.1, 0.1));
        assert!(a.draft().unwrap().marker);
        a.cancel();
        a.begin(Tool::Rect, &style(), pos2(0.1, 0.1));
        assert!(!a.draft().unwrap().marker);
    }

    #[test]
    fn laser_and_eraser_do_not_start_a_drawing() {
        let mut a = Annotations::default();
        a.begin(Tool::Laser, &style(), pos2(0.1, 0.1));
        assert!(a.draft().is_none());
        a.begin(Tool::Eraser, &style(), pos2(0.1, 0.1));
        assert!(a.draft().is_none());
    }

    #[test]
    fn undo_removes_the_latest_and_clear_removes_all() {
        let mut a = Annotations::default();
        for x in [0.1, 0.3, 0.5] {
            a.begin(Tool::Line, &style(), pos2(x, 0.1));
            a.update(pos2(x, 0.4));
            a.finish(1);
        }
        assert_eq!(a.on_page(1).len(), 3);
        assert!(a.undo(1));
        assert_eq!(a.on_page(1).len(), 2);
        assert_eq!(
            a.on_page(1)[1].kind,
            Kind::Line(pos2(0.3, 0.1), pos2(0.3, 0.4))
        );
        a.clear(1);
        assert!(a.is_empty(1));
        assert!(!a.undo(1), "nothing left to undo");
    }

    #[test]
    fn eraser_removes_only_nearby_annotations() {
        let mut a = Annotations::default();
        a.begin(Tool::Line, &style(), pos2(0.1, 0.5));
        a.update(pos2(0.4, 0.5));
        a.finish(0);
        a.begin(Tool::Line, &style(), pos2(0.6, 0.5));
        a.update(pos2(0.9, 0.5));
        a.finish(0);
        // Far from both lines: nothing happens.
        assert!(!a.erase_at(0, pos2(0.5, 0.1), 0.03, 16.0 / 9.0));
        assert_eq!(a.on_page(0).len(), 2);
        // On the left line.
        assert!(a.erase_at(0, pos2(0.25, 0.51), 0.03, 16.0 / 9.0));
        assert_eq!(a.on_page(0).len(), 1);
        assert_eq!(
            a.on_page(0)[0].kind,
            Kind::Line(pos2(0.6, 0.5), pos2(0.9, 0.5))
        );
    }

    #[test]
    fn eraser_hits_box_outline_but_not_its_empty_inside() {
        let mut a = Annotations::default();
        a.begin(Tool::Rect, &style(), pos2(0.2, 0.2));
        a.update(pos2(0.8, 0.8));
        a.finish(0);
        assert!(
            !a.erase_at(0, pos2(0.5, 0.5), 0.02, 16.0 / 9.0),
            "hollow inside"
        );
        assert!(a.erase_at(0, pos2(0.5, 0.2), 0.02, 16.0 / 9.0), "top edge");
    }

    #[test]
    fn eraser_hits_filled_area_anywhere_inside() {
        let mut a = Annotations::default();
        a.begin(Tool::Area, &style(), pos2(0.2, 0.2));
        a.update(pos2(0.8, 0.8));
        a.finish(0);
        assert!(a.erase_at(0, pos2(0.5, 0.5), 0.02, 16.0 / 9.0));
        assert!(a.is_empty(0));
    }

    #[test]
    fn eraser_hits_ellipse_on_its_outline() {
        let mut a = Annotations::default();
        a.begin(Tool::Ellipse, &style(), pos2(0.2, 0.2));
        a.update(pos2(0.8, 0.6));
        a.finish(0);
        assert!(!a.erase_at(0, pos2(0.5, 0.4), 0.02, 16.0 / 9.0), "centre");
        assert!(
            a.erase_at(0, pos2(0.8, 0.4), 0.02, 16.0 / 9.0),
            "right extreme"
        );
    }

    #[test]
    fn arrow_has_a_shaft_and_two_head_strokes() {
        let k = Kind::Arrow(pos2(0.1, 0.5), pos2(0.6, 0.5));
        let lines = k.polylines(16.0 / 9.0);
        assert_eq!(lines.len(), 3);
        // Both head strokes start at the tip.
        assert_eq!(lines[1][0], pos2(0.6, 0.5));
        assert_eq!(lines[2][0], pos2(0.6, 0.5));
        // ... and point back towards the shaft (smaller x), on opposite sides.
        assert!(lines[1][1].x < 0.6 && lines[2][1].x < 0.6);
        assert!(
            lines[1][1].y < 0.5 && lines[2][1].y > 0.5
                || lines[1][1].y > 0.5 && lines[2][1].y < 0.5
        );
    }

    #[test]
    fn degenerate_arrow_does_not_panic() {
        let k = Kind::Arrow(pos2(0.3, 0.3), pos2(0.3, 0.3));
        assert_eq!(k.polylines(1.0).len(), 1);
    }

    #[test]
    fn style_cycles_colors_per_tool_and_wraps() {
        let mut s = style();
        let first = s.color(Tool::Pen);
        s.cycle_color(Tool::Pen);
        assert_ne!(s.color(Tool::Pen), first);
        // Other tools keep their own choice.
        assert_eq!(s.color(Tool::Highlighter), MARKER_COLORS[0]);
        for _ in 0..PEN_COLORS.len() - 1 {
            s.cycle_color(Tool::Pen);
        }
        assert_eq!(s.color(Tool::Pen), first, "wraps around");
        // The eraser has no colors; cycling is a harmless no-op.
        s.cycle_color(Tool::Eraser);
    }

    #[test]
    fn style_cycles_widths_and_wraps() {
        let mut s = style();
        assert_eq!(s.width(), WIDTHS[0]);
        s.cycle_width();
        assert_eq!(s.width(), WIDTHS[1]);
        s.cycle_width();
        s.cycle_width();
        assert_eq!(s.width(), WIDTHS[0]);
        s.set_width_index(2);
        assert_eq!(s.width_index(), 2);
    }

    #[test]
    fn new_annotations_use_the_selected_style() {
        let mut s = style();
        s.cycle_color(Tool::Pen);
        s.cycle_width();
        let mut a = Annotations::default();
        a.begin(Tool::Pen, &s, pos2(0.1, 0.1));
        let d = a.draft().unwrap();
        assert_eq!(d.color, PEN_COLORS[1]);
        assert_eq!(d.width, WIDTHS[1]);
    }

    #[test]
    fn laser_trail_fades_but_head_follows_the_pointer() {
        let mut l = Laser::default();
        let t0 = Instant::now();
        assert!(!l.is_animating());
        l.point(pos2(0.2, 0.2), t0);
        l.point(pos2(0.3, 0.3), t0 + Duration::from_millis(100));
        assert_eq!(l.head(), Some(pos2(0.3, 0.3)));
        assert!(l.is_animating());
        // Pointer rests: no new points, head stays.
        l.prune(t0 + Duration::from_millis(300));
        assert_eq!(l.head(), Some(pos2(0.3, 0.3)));
        // Old trail points expire.
        l.prune(t0 + Duration::from_millis(520));
        assert_eq!(l.trail.len(), 1);
        // Leaving hides the dot immediately; the trail then fades out.
        l.leave();
        assert_eq!(l.head(), None);
        l.prune(t0 + Duration::from_secs(2));
        assert!(!l.is_animating());
    }

    #[test]
    fn laser_ignores_repeated_identical_positions() {
        let mut l = Laser::default();
        let t0 = Instant::now();
        for i in 0..10 {
            l.point(pos2(0.5, 0.5), t0 + Duration::from_millis(i));
        }
        assert_eq!(l.trail.len(), 1);
    }

    #[test]
    fn tool_list_is_consistent() {
        for (i, t) in Tool::ALL.iter().enumerate() {
            assert_eq!(t.index(), i);
        }
        // Key hints are unique.
        let mut keys: Vec<char> = Tool::ALL.iter().map(|t| t.key_hint()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Tool::ALL.len());
    }

    #[test]
    fn trail_strip_is_one_tapered_fading_ribbon() {
        let pts = [
            (pos2(0.0, 0.0), 0.9),
            (pos2(10.0, 0.0), 0.5),
            (pos2(20.0, 5.0), 0.0),
        ];
        let mesh = trail_strip(&pts, 6.0, Color32::RED).unwrap();
        assert_eq!(mesh.vertices.len(), 6, "two vertices per point");
        assert_eq!(mesh.indices.len(), 6 * 2, "two triangles per segment");
        assert!(
            mesh.indices
                .iter()
                .all(|i| (*i as usize) < mesh.vertices.len())
        );
        // The tail end is thinner and more transparent than the head end.
        let width = |i: usize| (mesh.vertices[2 * i].pos - mesh.vertices[2 * i + 1].pos).length();
        assert!(width(0) < width(1) && width(1) < width(2));
        assert!(mesh.vertices[0].color.a() < mesh.vertices[4].color.a());
    }

    #[test]
    fn tail_length_is_capped_no_matter_how_fast_the_mouse_moves() {
        // A very fast move: points 100 px apart, all brand new.
        let pts = [
            (pos2(0.0, 0.0), 0.1),
            (pos2(100.0, 0.0), 0.05),
            (pos2(200.0, 0.0), 0.02),
            (pos2(300.0, 0.0), 0.0),
        ];
        let tail = tail_points(&pts, 50.0);
        let len: f32 = tail.windows(2).map(|w| (w[1].0 - w[0].0).length()).sum();
        assert!((len - 50.0).abs() < 0.01, "tail is {len} px, expected 50");
        // It ends invisible, at exactly the cap, and the head is fully fresh.
        assert_eq!(tail[0].1, 1.0);
        assert_eq!(tail[0].0, pos2(250.0, 0.0));
        assert_eq!(tail.last().unwrap().0, pos2(300.0, 0.0));
        assert_eq!(tail.last().unwrap().1, 0.0);
    }

    #[test]
    fn slow_movement_keeps_its_short_time_based_tail() {
        let pts = [
            (pos2(0.0, 0.0), 0.8),
            (pos2(2.0, 0.0), 0.4),
            (pos2(4.0, 0.0), 0.0),
        ];
        let tail = tail_points(&pts, 50.0);
        assert_eq!(tail.len(), 3, "nothing was cut");
        // Fade is driven by age here (distance fractions are tiny).
        assert!((tail[0].1 - 0.8).abs() < 1e-6);
        assert!((tail[1].1 - 0.4).abs() < 1e-6);
    }

    #[test]
    fn tail_fade_never_decreases_towards_the_tail_end() {
        let pts: Vec<(Pos2, f32)> = (0..20)
            .map(|i| (pos2(i as f32 * 7.0, 0.0), (19 - i) as f32 * 0.04))
            .collect();
        let tail = tail_points(&pts, 40.0);
        assert!(tail.windows(2).all(|w| w[0].1 >= w[1].1), "{tail:?}");
    }

    #[test]
    fn tail_points_handles_empty_single_and_stationary_input() {
        assert!(tail_points(&[], 10.0).is_empty());
        assert_eq!(tail_points(&[(pos2(1.0, 1.0), 0.0)], 10.0).len(), 1);
        // The pointer rests: many identical points, no division by zero.
        let rest = [
            (pos2(5.0, 5.0), 0.6),
            (pos2(5.0, 5.0), 0.3),
            (pos2(5.0, 5.0), 0.0),
        ];
        let tail = tail_points(&rest, 10.0);
        assert!(tail.iter().all(|(p, f)| p.x.is_finite() && f.is_finite()));
    }

    #[test]
    fn trail_strip_needs_two_points_and_survives_duplicates() {
        assert!(trail_strip(&[], 5.0, Color32::RED).is_none());
        assert!(trail_strip(&[(pos2(1.0, 1.0), 0.0)], 5.0, Color32::RED).is_none());
        let same = [(pos2(3.0, 3.0), 0.5), (pos2(3.0, 3.0), 0.0)];
        let mesh = trail_strip(&same, 5.0, Color32::RED).unwrap();
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.pos.x.is_finite() && v.pos.y.is_finite())
        );
    }

    #[test]
    fn draw_exercises_every_shape_without_panicking() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(160.0, 90.0));
        let painter = egui::Painter::new(
            egui::Context::default(),
            egui::LayerId::new(egui::Order::Background, egui::Id::new("test")),
            rect,
        );
        let mut a = Annotations::default();
        for tool in [
            Tool::Pen,
            Tool::Highlighter,
            Tool::Area,
            Tool::Line,
            Tool::Arrow,
            Tool::Rect,
            Tool::Ellipse,
        ] {
            a.begin(tool, &style(), pos2(0.1, 0.1));
            a.update(pos2(0.7, 0.6));
            a.finish(0);
        }
        a.begin(Tool::Pen, &style(), pos2(0.5, 0.5)); // an unfinished draft too
        draw_page(&painter, rect, a.on_page(0), a.draft());
        let mut l = Laser::default();
        l.point(pos2(0.5, 0.5), Instant::now());
        draw_laser(&painter, rect, &l, Color32::RED, 4.0, Instant::now());
    }
}
