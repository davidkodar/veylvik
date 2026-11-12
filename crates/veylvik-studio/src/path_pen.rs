//! An unfinished contour is local UI state, never invalid document geometry.
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use veylvik_document::vector::{MAX_PATH_COMMANDS, MAX_PATH_COORDINATE, PathCommand, PathMask};

#[derive(Clone, Copy)]
struct Knot {
    position: [f32; 2],
    tangent: [f32; 2],
}
#[derive(Default)]
pub struct Pen {
    pub active: bool,
    append: bool,
    knots: Vec<Knot>,
    pressed: bool,
}
impl Pen {
    pub fn begin(&mut self) {
        self.cancel();
        self.active = true;
    }
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if !self.active || ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.cancel();
        } else if ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        }) {
            self.knots.pop();
            self.pressed = false;
        }
    }
    /// Returns true when drawing has just begun.
    pub fn toolbar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut started = false;
        if self.active {
            ui.colored_label(Color32::from_rgb(255, 190, 80), "Drawing mask");
            if ui
                .button("Undo point")
                .on_hover_text("Backspace or Ctrl+Z")
                .clicked()
            {
                self.knots.pop();
            }
            if ui
                .button("Cancel")
                .on_hover_text("Escape discards the unfinished contour")
                .clicked()
            {
                self.cancel();
            }
        } else {
            for (label, append) in [("Draw mask", false), ("Add contour", true)] {
                if ui.button(label).on_hover_text(if append { "Draw another contour in this mask" } else { "Draw a replacement mask; the original stays intact until you close the shape" }).clicked() {
                    self.cancel(); self.active = true; self.append = append; started = true;
                }
            }
        }
        started
    }
    pub fn show(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        transform: crate::viewer::ViewTransform,
        image: Vec2,
        path: &PathMask,
    ) -> Option<PathMask> {
        if !self.active {
            return None;
        }
        let [ox, oy, sx, sy] = transform.0;
        let scale = egui::vec2(
            canvas.width() * sx / image.x,
            canvas.height() * sy / image.y,
        );
        if !scale.is_finite() || scale.x <= 0.0 || scale.y <= 0.0 {
            return None;
        }
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let screen = |p: [f32; 2]| origin + egui::vec2(p[0], p[1]) * scale;
        let local = |p: egui::Pos2| {
            let v = (p - origin) / scale;
            [
                v.x.clamp(-MAX_PATH_COORDINATE, MAX_PATH_COORDINATE),
                v.y.clamp(-MAX_PATH_COORDINATE, MAX_PATH_COORDINATE),
            ]
        };
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let close = pointer.is_some_and(|p| {
            self.knots.len() >= 3 && screen(self.knots[0].position).distance(p) <= 10.0
        });
        let available =
            MAX_PATH_COMMANDS.saturating_sub(if self.append { path.commands.len() } else { 0 });
        if let Some(p) = pointer.filter(|p| canvas.contains(*p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if ui.input(|i| i.pointer.primary_pressed())
                && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
            {
                if close {
                    let mut result = path.clone();
                    if !self.append {
                        result.commands.clear();
                    }
                    result.commands.extend(contour(&self.knots));
                    if result.validate().is_ok() {
                        self.cancel();
                        return Some(result);
                    }
                } else if self.knots.len() + 3 <= available {
                    self.knots.push(Knot {
                        position: local(ui.input(|i| i.pointer.press_origin()).unwrap_or(p)),
                        tangent: [0.0; 2],
                    });
                    self.pressed = true;
                }
            }
        }
        if self.pressed {
            if let (Some(p), Some(knot)) = (pointer, self.knots.last_mut()) {
                let q = local(p);
                // A click remains a corner; deliberate drags create symmetric tangents.
                if screen(knot.position).distance(p) > 3.0 {
                    knot.tangent = [q[0] - knot.position[0], q[1] - knot.position[1]];
                }
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                self.pressed = false;
            }
        }
        let painter = ui.painter().with_clip_rect(canvas);
        let color = Color32::from_rgb(255, 190, 80);
        let stroke = Stroke::new(1.5_f32, color);
        for pair in self.knots.windows(2) {
            let a = pair[0];
            let b = pair[1];
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [
                    screen(a.position),
                    screen(offset(a, 1.0)),
                    screen(offset(b, -1.0)),
                    screen(b.position),
                ],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
        }
        if !self.pressed
            && let (Some(last), Some(p)) =
                (self.knots.last(), pointer.filter(|p| canvas.contains(*p)))
        {
            painter.line_segment(
                [screen(last.position), p],
                Stroke::new(1.0_f32, Color32::GRAY),
            );
        }
        for (index, knot) in self.knots.iter().enumerate() {
            if knot.tangent != [0.0; 2] {
                painter.line_segment(
                    [screen(offset(*knot, -1.0)), screen(offset(*knot, 1.0))],
                    Stroke::new(1.0_f32, Color32::GRAY),
                );
                for sign in [-1.0, 1.0] {
                    painter.circle_filled(screen(offset(*knot, sign)), 3.0, color);
                }
            }
            let p = screen(knot.position);
            painter.circle_filled(p, 5.0, Color32::from_gray(22));
            painter.circle_stroke(p, if index == 0 && close { 8.0 } else { 5.0 }, stroke);
        }
        let hint = if self.knots.len() + 3 > available {
            "Mask point limit reached. Close the contour or undo a point."
        } else if close {
            "Click to close mask"
        } else {
            "Click for corners · drag for curves · click first point to close · Esc cancels"
        };
        painter.text(
            canvas.left_bottom() + egui::vec2(12.0, -14.0),
            egui::Align2::LEFT_BOTTOM,
            hint,
            egui::FontId::proportional(13.0),
            color,
        );
        None
    }
}
fn offset(k: Knot, sign: f32) -> [f32; 2] {
    [
        (k.position[0] + sign * k.tangent[0]).clamp(-MAX_PATH_COORDINATE, MAX_PATH_COORDINATE),
        (k.position[1] + sign * k.tangent[1]).clamp(-MAX_PATH_COORDINATE, MAX_PATH_COORDINATE),
    ]
}
fn contour(knots: &[Knot]) -> Vec<PathCommand> {
    if knots.len() < 3 {
        return vec![];
    }
    let mut result = vec![PathCommand::Move {
        to: knots[0].position,
    }];
    for i in 0..knots.len() {
        let a = knots[i];
        let b = knots[(i + 1) % knots.len()];
        if i + 1 == knots.len() && a.tangent == [0.0; 2] && b.tangent == [0.0; 2] {
            break;
        }
        result.push(if a.tangent == [0.0; 2] && b.tangent == [0.0; 2] {
            PathCommand::Line { to: b.position }
        } else {
            PathCommand::Cubic {
                control1: offset(a, 1.0),
                control2: offset(b, -1.0),
                to: b.position,
            }
        });
    }
    result.push(PathCommand::Close {});
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_curve_uses_first_and_last_tangents_and_is_valid() {
        let k = [
            Knot {
                position: [10.0, 20.0],
                tangent: [5.0, 0.0],
            },
            Knot {
                position: [100.0, 20.0],
                tangent: [0.0; 2],
            },
            Knot {
                position: [50.0, 100.0],
                tangent: [0.0, 10.0],
            },
        ];
        let path = PathMask {
            fill_rule: Default::default(),
            commands: contour(&k),
        };
        path.validate().unwrap();
        assert_eq!(
            path.commands[3],
            PathCommand::Cubic {
                control1: [50.0, 110.0],
                control2: [5.0, 20.0],
                to: [10.0, 20.0]
            }
        );
        assert!(contour(&k[..2]).is_empty());
    }
    #[test]
    fn coalesced_drag_keeps_press_origin_and_closes_in_one_result() {
        let ctx = egui::Context::default();
        let mut pen = Pen {
            active: true,
            ..Default::default()
        };
        let path = PathMask::default();
        let mut result = None;
        let mut run = |events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    result = pen.show(
                        ui,
                        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0)),
                        crate::viewer::ViewTransform::default(),
                        egui::vec2(400.0, 300.0),
                        &path,
                    );
                },
            );
        };
        let button = |x, y, pressed| egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        run(vec![]);
        run(vec![
            egui::Event::PointerMoved(egui::pos2(50.0, 50.0)),
            button(50.0, 50.0, true),
            egui::Event::PointerMoved(egui::pos2(70.0, 60.0)),
        ]);
        run(vec![button(70.0, 60.0, false)]);
        for (x, y) in [(250.0, 50.0), (150.0, 200.0), (50.0, 50.0)] {
            run(vec![
                egui::Event::PointerMoved(egui::pos2(x, y)),
                button(x, y, true),
            ]);
            if x != 50.0 {
                run(vec![button(x, y, false)]);
            }
        }
        let result = result.expect("closing returns valid geometry");
        result.validate().unwrap();
        assert_eq!(result.commands[0], PathCommand::Move { to: [50.0, 50.0] });
        assert!(matches!(
            result.commands[1],
            PathCommand::Cubic {
                control1: [70.0, 60.0],
                ..
            }
        ));
        assert!(!pen.active);
        assert_eq!(path, PathMask::default());
    }
    #[test]
    fn cancel_and_local_undo_do_not_mutate_document() {
        let ctx = egui::Context::default();
        let mut pen = Pen {
            active: true,
            knots: vec![Knot {
                position: [1.0, 2.0],
                tangent: [0.0; 2],
            }],
            ..Default::default()
        };
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| pen.shortcuts(ui.ctx()),
        );
        assert!(pen.active);
        assert!(pen.knots.is_empty());
        pen.cancel();
        assert!(!pen.active);
    }
}
