//! Font-independent interface controls. Geometry stays crisp at every UI scale.
use eframe::egui::{self, Widget};
#[derive(Clone, Copy)]
pub enum Icon {
    Effects,
    Materials,
    Templates,
    Media,
    Inspector,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Play,
    Pause,
    Loop,
    Left,
    Right,
    Up,
    Down,
    PreviousFrame,
    NextFrame,
    Reset,
    Anchor,
    More,
    Info,
}
pub struct Button {
    icon: Icon,
    label: String,
    selected: bool,
    tooltip: bool,
    caption: bool,
}
impl Button {
    pub fn new(icon: Icon, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            selected: false,
            tooltip: true,
            caption: false,
        }
    }
    pub fn caption(mut self) -> Self {
        self.caption = true;
        self
    }
    /// Keeps the label for accessibility but leaves the hover tooltip to the caller.
    pub fn without_tooltip(mut self) -> Self {
        self.tooltip = false;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}
impl Widget for Button {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            if self.caption {
                egui::vec2(54., 48.)
            } else {
                egui::vec2(22., 22.)
            },
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.label)
        });
        let visuals = ui.style().interact_selectable(&response, self.selected);
        let painter = ui.painter();
        let c = rect.center()
            - if self.caption {
                egui::vec2(0., 8.)
            } else {
                egui::Vec2::ZERO
            };
        let colour = visuals.fg_stroke.color;
        if response.hovered() || self.selected {
            painter.rect_filled(rect, 4.0, visuals.bg_fill);
        }
        if self.caption {
            painter.text(
                egui::pos2(rect.center().x, rect.bottom() - 5.),
                egui::Align2::CENTER_BOTTOM,
                &self.label,
                egui::FontId::proportional(10.),
                colour,
            );
        }
        match self.icon {
            Icon::Effects => {
                painter.add(egui::Shape::closed_line(
                    vec![
                        c + egui::vec2(0., -8.),
                        c + egui::vec2(2., -2.),
                        c + egui::vec2(8., 0.),
                        c + egui::vec2(2., 2.),
                        c + egui::vec2(0., 8.),
                        c + egui::vec2(-2., 2.),
                        c + egui::vec2(-8., 0.),
                        c + egui::vec2(-2., -2.),
                    ],
                    egui::Stroke::new(1.4_f32, colour),
                ));
            }
            Icon::Materials => {
                painter.circle_stroke(c, 7., egui::Stroke::new(1.4_f32, colour));
                painter.line_segment(
                    [c + egui::vec2(-4., 3.), c + egui::vec2(3., -4.)],
                    (1.4, colour),
                );
            }
            Icon::Templates => {
                for x in [-6., 2.] {
                    for y in [-6., 2.] {
                        painter.rect_stroke(
                            egui::Rect::from_min_size(c + egui::vec2(x, y), egui::Vec2::splat(5.)),
                            1.,
                            (1.2, colour),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
            Icon::Media => {
                painter.rect_stroke(
                    egui::Rect::from_center_size(c, egui::vec2(16., 12.)),
                    1.,
                    (1.3, colour),
                    egui::StrokeKind::Inside,
                );
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        c + egui::vec2(-2., -3.),
                        c + egui::vec2(3., 0.),
                        c + egui::vec2(-2., 3.),
                    ],
                    colour,
                    egui::Stroke::NONE,
                ));
            }
            Icon::Inspector => {
                for (y, x) in [(-5., -3.), (0., 3.), (5., -1.)] {
                    painter.line_segment(
                        [c + egui::vec2(-7., y), c + egui::vec2(7., y)],
                        (1.2, colour),
                    );
                    painter.circle_filled(c + egui::vec2(x, y), 2., colour);
                }
            }

            Icon::AlignLeft | Icon::AlignCenter | Icon::AlignRight => {
                for row in 0..4 {
                    let width = if row % 2 == 0 { 12. } else { 8. };
                    let left = match self.icon {
                        Icon::AlignRight => 6. - width,
                        Icon::AlignCenter => -width / 2.,
                        _ => -6.,
                    };
                    let y = -5. + row as f32 * 3.5;
                    painter.line_segment(
                        [c + egui::vec2(left, y), c + egui::vec2(left + width, y)],
                        egui::Stroke::new(1.5_f32, colour),
                    );
                }
            }

            Icon::Play => {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        c + egui::vec2(-4., -6.),
                        c + egui::vec2(6., 0.),
                        c + egui::vec2(-4., 6.),
                    ],
                    colour,
                    egui::Stroke::NONE,
                ));
            }
            Icon::Pause => {
                for x in [-4., 2.] {
                    painter.rect_filled(
                        egui::Rect::from_min_size(c + egui::vec2(x, -6.), egui::vec2(3., 12.)),
                        1.,
                        colour,
                    );
                }
            }

            Icon::More => {
                for x in [-4.0, 0.0, 4.0] {
                    painter.circle_filled(c + egui::vec2(x, 0.0), 1.2, colour);
                }
            }
            Icon::Anchor => {
                painter.circle_filled(c, 3.5, colour);
            }
            Icon::Info => {
                painter.circle_stroke(c, 7.0, egui::Stroke::new(1.3_f32, colour));
                painter.circle_filled(c + egui::vec2(0.0, -3.2), 1.1, colour);
                painter.line_segment(
                    [c + egui::vec2(0.0, -0.8), c + egui::vec2(0.0, 4.0)],
                    (1.6, colour),
                );
            }
            Icon::Loop => {
                let stroke = egui::Stroke::new(1.5_f32, colour);
                painter.line_segment([c + egui::vec2(-6., -4.), c + egui::vec2(6., -4.)], stroke);
                painter.line_segment([c + egui::vec2(6., -4.), c + egui::vec2(6., 2.)], stroke);
                painter.line_segment([c + egui::vec2(6., 4.), c + egui::vec2(-6., 4.)], stroke);
                painter.line_segment([c + egui::vec2(-6., 4.), c + egui::vec2(-6., -2.)], stroke);
                painter.line_segment([c + egui::vec2(2., -7.), c + egui::vec2(6., -4.)], stroke);
                painter.line_segment([c + egui::vec2(2., -1.), c + egui::vec2(6., -4.)], stroke);
                painter.line_segment([c + egui::vec2(-2., 1.), c + egui::vec2(-6., 4.)], stroke);
                painter.line_segment([c + egui::vec2(-2., 7.), c + egui::vec2(-6., 4.)], stroke);
            }
            Icon::Reset => {
                let points: Vec<_> = (0..=24)
                    .map(|i| {
                        let a = 0.2 + i as f32 / 24.0 * 5.1;
                        c + egui::vec2(a.cos(), a.sin()) * 5.0
                    })
                    .collect();
                let tip = *points.last().unwrap();
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(1.4_f32, colour),
                ));
                painter.line_segment([tip + egui::vec2(-3.0, 0.0), tip], (1.4, colour));
                painter.line_segment([tip + egui::vec2(0.0, 3.0), tip], (1.4, colour));
            }
            icon => {
                let transform = |x: f32, y: f32| {
                    c + match icon {
                        Icon::Left | Icon::PreviousFrame => egui::vec2(-x, y),
                        Icon::Up => egui::vec2(y, -x),
                        Icon::Down => egui::vec2(y, x),
                        _ => egui::vec2(x, y),
                    }
                };
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        transform(4.0, 0.0),
                        transform(-3.0, -4.0),
                        transform(-3.0, 4.0),
                    ],
                    colour,
                    egui::Stroke::NONE,
                ));
                if matches!(icon, Icon::PreviousFrame | Icon::NextFrame) {
                    painter
                        .line_segment([transform(6.0, -5.0), transform(6.0, 5.0)], (1.4, colour));
                }
            }
        }
        if self.tooltip {
            response.on_hover_text(self.label)
        } else {
            response
        }
    }
}

const TILE: egui::Color32 = egui::Color32::from_rgb(0x17, 0x1b, 0x24);
const VIOLET: egui::Color32 = egui::Color32::from_rgb(0x9c, 0x8c, 0xff);
const TEAL: egui::Color32 = egui::Color32::from_rgb(0x5e, 0xea, 0xd4);

/// The Veylvik mark, drawn from the same 128-unit geometry as `pkgbuild/veylvik.svg`.
/// Below 32 physical pixels it switches to the simplified `veylvik-small.svg` form,
/// whose bolder V and single square stay legible where the node detail would blur.
pub fn paint_mark(painter: &egui::Painter, rect: egui::Rect, tile: bool) {
    let unit = rect.width().min(rect.height()) / 128.0;
    let origin = rect.center() - egui::vec2(64.0, 64.0) * unit;
    let at = |x: f32, y: f32| origin + egui::vec2(x, y) * unit;
    let detailed = rect.width() * painter.ctx().pixels_per_point() >= 32.0;
    if tile {
        painter.rect_filled(
            egui::Rect::from_min_size(at(0.0, 0.0), egui::Vec2::splat(128.0 * unit)),
            28.0 * unit,
            TILE,
        );
    }
    let (arm, width, gradient_axis) = if detailed {
        (
            [(27.0, 30.0), (64.0, 98.0), (101.0, 30.0)],
            12.0,
            [(25.0, 29.0), (99.0, 94.0)],
        )
    } else {
        (
            [(26.0, 30.0), (64.0, 102.0), (102.0, 30.0)],
            22.0,
            [(20.0, 24.0), (104.0, 100.0)],
        )
    };
    mitred_stroke(
        painter,
        arm.map(|(x, y)| egui::vec2(x, y)),
        width,
        gradient(gradient_axis.map(|(x, y)| egui::vec2(x, y))),
        |p| at(p.x, p.y),
    );
    if detailed {
        painter.add(egui::Shape::line(
            vec![at(27.0, 30.0), at(56.0, 30.0), at(75.0, 65.0)],
            egui::Stroke::new(3.0 * unit, VIOLET),
        ));
        for x in [20.0, 49.0] {
            let square = egui::Rect::from_min_size(at(x, 23.0), egui::Vec2::splat(14.0 * unit));
            painter.rect(
                square,
                0.0,
                TILE,
                egui::Stroke::new(4.0 * unit, VIOLET),
                egui::StrokeKind::Middle,
            );
        }
        painter.rect_filled(
            egui::Rect::from_min_size(at(94.0, 23.0), egui::Vec2::splat(14.0 * unit)),
            0.0,
            TEAL,
        );
    } else {
        painter.rect_filled(
            egui::Rect::from_min_size(at(86.0, 16.0), egui::Vec2::splat(26.0 * unit)),
            0.0,
            TEAL,
        );
    }
}

/// A mitred two-segment stroke with butt caps, as SVG draws `stroke-linejoin="miter"`.
fn mitred_stroke(
    painter: &egui::Painter,
    [p0, p1, p2]: [egui::Vec2; 3],
    width: f32,
    colour: impl Fn(egui::Vec2) -> egui::Color32,
    place: impl Fn(egui::Vec2) -> egui::Pos2,
) {
    let normal = |a: egui::Vec2, b: egui::Vec2| {
        let d = (b - a).normalized();
        egui::vec2(-d.y, d.x)
    };
    let (n1, n2) = (normal(p0, p1), normal(p1, p2));
    let miter = (n1 + n2) / (1.0 + n1.dot(n2));
    let half = width / 2.0;
    let outline = [
        p0 + n1 * half,
        p1 + miter * half,
        p2 + n2 * half,
        p2 - n2 * half,
        p1 - miter * half,
        p0 - n1 * half,
    ];
    let triangles = [[0, 5, 4], [0, 4, 1], [1, 4, 3], [1, 3, 2]];
    fill_smooth(painter, &outline, &triangles, colour, place);
}

/// Fill a simple polygon with antialiased edges. A raw `egui::Mesh` has hard, jagged
/// edges; like egui's own shapes, this fades a one-pixel band across the outline.
/// `triangles` index `outline`; colours are sampled in the caller's units, so a linear
/// gradient stays exact.
fn fill_smooth(
    painter: &egui::Painter,
    outline: &[egui::Vec2],
    triangles: &[[u32; 3]],
    colour: impl Fn(egui::Vec2) -> egui::Color32,
    place: impl Fn(egui::Vec2) -> egui::Pos2,
) {
    let n = outline.len();
    let points: Vec<egui::Pos2> = outline.iter().map(|&p| place(p)).collect();
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let side = area.signum();
    let outward = |a: egui::Pos2, b: egui::Pos2| {
        let d = (b - a).normalized();
        egui::vec2(d.y, -d.x) * side
    };
    let half = 0.5 / painter.ctx().pixels_per_point();
    let mut mesh = egui::Mesh::default();
    let mut ring = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, here, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let (a, b) = (outward(prev, here), outward(here, next));
        let mut offset = (a + b) / (1.0 + a.dot(b)).max(1e-3);
        if offset.length() > 4.0 {
            offset = offset.normalized() * 4.0; // Bound spikes at very sharp corners.
        }
        mesh.colored_vertex(here - offset * half, colour(outline[i]));
        ring.push(here + offset * half);
    }
    for p in ring {
        mesh.colored_vertex(p, egui::Color32::TRANSPARENT);
    }
    for &[a, b, c] in triangles {
        mesh.add_triangle(a, b, c);
    }
    let n = n as u32;
    for i in 0..n {
        let j = (i + 1) % n;
        mesh.add_triangle(i, j, n + i);
        mesh.add_triangle(j, n + j, n + i);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Linear violet-to-teal gradient between two points, as in the SVG artwork.
fn gradient([from, to]: [egui::Vec2; 2]) -> impl Fn(egui::Vec2) -> egui::Color32 {
    let axis = to - from;
    move |p| {
        VIOLET.lerp_to_gamma(
            TEAL,
            ((p - from).dot(axis) / axis.length_sq()).clamp(0.0, 1.0),
        )
    }
}

/// Width of the wordmark in cap heights, from the first V to the tip of the K.
pub const WORDMARK_WIDTH: f32 = 5.34;

/// The VEYLVIK wordmark from `assets/branding/veylvik-logo.svg`, drawn as geometry in
/// 100-unit caps with its top-left at `left_top`. One ink colour throughout: the icon
/// carries the brand colour. The Y and K keep slanted terminals with a small optical
/// overshoot (about 1.5 units) beyond the cap and base lines.
pub fn paint_wordmark(painter: &egui::Painter, left_top: egui::Pos2, cap: f32, ink: egui::Color32) {
    let unit = cap / 100.0;
    let at = move |p: egui::Vec2| left_top + p * unit;
    let bar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        painter.rect_filled(
            egui::Rect::from_two_pos(at(egui::vec2(x0, y0)), at(egui::vec2(x1, y1))),
            0.0,
            ink,
        );
    };
    let stroke = |points: [(f32, f32); 3]| {
        mitred_stroke(
            painter,
            points.map(|(x, y)| egui::vec2(x, y)),
            20.0,
            |_| ink,
            at,
        );
    };
    let v = |x: f32| {
        let outline = [
            (0.0, 0.0),
            (23.0, 0.0),
            (35.0, 58.0),
            (47.0, 0.0),
            (70.0, 0.0),
            (47.0, 100.0),
            (23.0, 100.0),
        ]
        .map(|(px, py)| egui::vec2(px + x, py));
        let triangles = [[0, 1, 2], [0, 2, 6], [6, 2, 5], [2, 3, 5], [3, 4, 5]];
        fill_smooth(painter, &outline, &triangles, |_| ink, at);
    };
    v(0.0);
    bar(94.0, 0.0, 114.0, 100.0);
    bar(94.0, 0.0, 146.0, 20.0);
    bar(94.0, 40.0, 138.0, 60.0);
    bar(94.0, 80.0, 146.0, 100.0);
    stroke([(178.56, 4.0), (208.0, 50.0), (237.44, 4.0)]);
    bar(198.0, 50.0, 218.0, 100.0);
    bar(266.0, 0.0, 286.0, 100.0);
    bar(266.0, 80.0, 318.0, 100.0);
    v(332.0);
    bar(422.0, 0.0, 442.0, 100.0);
    bar(468.0, 0.0, 488.0, 100.0);
    stroke([(524.54, 4.5), (488.0, 52.0), (526.06, 95.5)]);
}
/// A discreet reflection crossing the wordmark; `progress` runs 0..1 from before the V
/// to past the K. The band leans like light across a bevel, fades in steps towards its
/// core, and carries the icon's violet-to-teal gradient from its top to its bottom. Drawing is clipped to one physical pixel row at a time, so the slant
/// follows the letter shapes exactly. Colours are opaque blends with the ink, so letter
/// pieces that overlap (the E's corners, the Y and K joints) never double in brightness.
pub fn paint_wordmark_sheen(
    painter: &egui::Painter,
    left_top: egui::Pos2,
    cap: f32,
    ink: egui::Color32,
    progress: f32,
) {
    let slant = 0.45 * cap;
    let (from, to) = (-0.8 * cap, WORDMARK_WIDTH * cap + 0.8 * cap + slant);
    let centre = left_top.x + from + (to - from) * progress.clamp(0.0, 1.0) - slant;
    let ppp = painter.ctx().pixels_per_point();
    let (top, bottom) = (left_top.y - 0.03 * cap, left_top.y + 1.03 * cap);
    let rows = (top * ppp).floor() as i32..(bottom * ppp).ceil() as i32;
    for (half, strength) in [
        (0.75, 0.15),
        (0.55, 0.3),
        (0.4, 0.5),
        (0.26, 0.7),
        (0.14, 0.9),
    ] {
        for row in rows.clone() {
            let (y0, y1) = (row as f32 / ppp, (row + 1) as f32 / ppp);
            let down = (((y0 + y1) / 2.0 - top) / (bottom - top)).clamp(0.0, 1.0);
            let colour = ink.lerp_to_gamma(VIOLET.lerp_to_gamma(TEAL, down), strength);
            // Higher rows lean further right, like light raking across the letters.
            let x = centre + slant * (1.0 - ((y0 + y1) / 2.0 - left_top.y) / cap);
            let clip = egui::Rect::from_min_max(
                egui::pos2(x - half * cap, y0),
                egui::pos2(x + half * cap, y1),
            )
            .intersect(painter.clip_rect());
            if clip.is_positive() {
                paint_wordmark(&painter.with_clip_rect(clip), left_top, cap, colour);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn interface_controls_do_not_depend_on_symbol_font_glyphs() {
        // Every source file, including feature folders such as `nodes/`.
        let mut pending = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        let mut files = Vec::new();
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        for path in files {
            if path.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(!source.chars().any(|c| matches!(c as u32,0x2190..=0x21ff|0x25a0..=0x27ff|0x2b00..=0x2bff|0x22ef)),"Use drawn icons instead of symbol glyphs in {}",path.display());
            }
        }
    }
}
