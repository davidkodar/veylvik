//! Font-independent interface controls. Geometry stays crisp at every UI scale.
use eframe::egui::{self, Widget};
#[derive(Clone, Copy)]
pub enum Icon {
    Left,
    Right,
    Up,
    Down,
    PreviousFrame,
    NextFrame,
    Reset,
    Anchor,
    More,
}
pub struct Button {
    icon: Icon,
    label: String,
    selected: bool,
}
impl Button {
    pub fn new(icon: Icon, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            selected: false,
        }
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}
impl Widget for Button {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.label)
        });
        let visuals = ui.style().interact_selectable(&response, self.selected);
        let painter = ui.painter();
        let c = rect.center();
        let colour = visuals.fg_stroke.color;
        painter.rect_filled(rect, 3.0, visuals.bg_fill);
        match self.icon {
            Icon::More => {
                for x in [-4.0, 0.0, 4.0] {
                    painter.circle_filled(c + egui::vec2(x, 0.0), 1.2, colour);
                }
            }
            Icon::Anchor => {
                painter.circle_filled(c, 3.5, colour);
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
        response.on_hover_text(self.label)
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
    let (arm, width, gradient) = if detailed {
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
    painter.add(gradient_v(
        arm.map(|(x, y)| egui::vec2(x, y)),
        width,
        gradient.map(|(x, y)| egui::vec2(x, y)),
        |p| at(p.x, p.y),
    ));
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

/// A mitred two-segment stroke, coloured by a linear violet-to-teal gradient. Vertex
/// colours interpolate linearly, which is exact for a linear gradient.
fn gradient_v(
    [p0, p1, p2]: [egui::Vec2; 3],
    width: f32,
    [from, to]: [egui::Vec2; 2],
    place: impl Fn(egui::Vec2) -> egui::Pos2,
) -> egui::Shape {
    let normal = |a: egui::Vec2, b: egui::Vec2| {
        let d = (b - a).normalized();
        egui::vec2(-d.y, d.x)
    };
    let (n1, n2) = (normal(p0, p1), normal(p1, p2));
    let miter = (n1 + n2) / (1.0 + n1.dot(n2));
    let half = width / 2.0;
    let points = [
        p0 + n1 * half,
        p0 - n1 * half,
        p1 + miter * half,
        p1 - miter * half,
        p2 + n2 * half,
        p2 - n2 * half,
    ];
    let axis = to - from;
    let mut mesh = egui::Mesh::default();
    for p in points {
        let t = ((p - from).dot(axis) / axis.length_sq()).clamp(0.0, 1.0);
        mesh.colored_vertex(place(p), VIOLET.lerp_to_gamma(TEAL, t));
    }
    for [a, b, c] in [[0, 1, 3], [0, 3, 2], [2, 3, 5], [2, 5, 4]] {
        mesh.add_triangle(a, b, c);
    }
    egui::Shape::mesh(mesh)
}
#[cfg(test)]
mod tests {
    #[test]
    fn interface_controls_do_not_depend_on_symbol_font_glyphs() {
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(!source.chars().any(|c| matches!(c as u32,0x2190..=0x21ff|0x25a0..=0x27ff|0x2b00..=0x2bff|0x22ef)),"Use drawn icons instead of symbol glyphs in {}",path.display());
            }
        }
    }
}
