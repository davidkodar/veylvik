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
