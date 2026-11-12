//! Whole-mask gestures are evaluated from their starting geometry, avoiding drift.
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use veylvik_document::vector::{PathCommand, PathMask};

struct Drag {
    original: PathMask,
    bounds: Rect,
    pointer: Pos2,
    handle: usize,
}
pub struct Editor {
    pub active: bool,
    proportional: bool,
    drag: Option<Drag>,
    limited: bool,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            active: false,
            proportional: true,
            drag: None,
            limited: false,
        }
    }
}
impl Editor {
    pub fn reset_gesture(&mut self) {
        self.drag = None;
        self.limited = false;
    }
    pub fn toolbar(&mut self, ui: &mut egui::Ui) -> bool {
        let before = self.active;
        ui.selectable_value(&mut self.active, false, "Points")
            .on_hover_text("Edit individual mask points and curve handles");
        ui.selectable_value(&mut self.active, true, "Transform mask")
            .on_hover_text("Move, rotate or scale every contour together");
        if self.active {
            ui.checkbox(&mut self.proportional, "Keep proportions")
                .on_hover_text("Corner handles scale uniformly; side handles stretch one axis");
        }
        if self.active != before {
            self.reset_gesture();
        }
        self.active && !before
    }
    pub fn show(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        view: crate::viewer::ViewTransform,
        image: Vec2,
        path: &mut PathMask,
    ) -> bool {
        let [ox, oy, sx, sy] = view.0;
        let scale = egui::vec2(
            canvas.width() * sx / image.x,
            canvas.height() * sy / image.y,
        );
        if !scale.is_finite() || scale.x <= 0.0 || scale.y <= 0.0 {
            return false;
        }
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let screen = |p: Pos2| origin + p.to_vec2() * scale;
        let local = |p: Pos2| Pos2::ZERO + (p - origin) / scale;
        let b = veylvik_text::bounds(path);
        if !b.iter().all(|v| v.is_finite()) {
            return false;
        }
        let bounds = Rect::from_min_max(egui::pos2(b[0], b[1]), egui::pos2(b[2], b[3]));
        let rect = Rect::from_min_max(screen(bounds.min), screen(bounds.max));
        let painter = ui.painter().with_clip_rect(canvas);
        let color = Color32::from_rgb(100, 195, 255);
        let stroke = Stroke::new(1.5_f32, color);
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Middle);
        let rotation = egui::pos2(
            rect.center().x,
            (rect.top() - 28.0).max(canvas.top() + 10.0),
        );
        painter.line_segment([rect.center_top(), rotation], stroke);
        let handles = [
            rect.center(),
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.center_top(),
            rect.right_center(),
            rect.center_bottom(),
            rect.left_center(),
            rotation,
        ];
        let mut changed = false;
        // The interior is the move target. Smaller controls win overlap by order.
        let mut responses = vec![];
        for (index, position) in handles.iter().copied().enumerate() {
            let hit = if index == 0 {
                rect.shrink(10.0)
                    .union(Rect::from_center_size(position, Vec2::splat(16.0)))
            } else {
                Rect::from_center_size(position, Vec2::splat(16.0))
            }
            .intersect(canvas);
            if !hit.is_positive() {
                continue;
            }
            let response =
                crate::handles::interact(ui, hit, ui.id().with(("mask-transform", index)))
                    .on_hover_cursor(match index {
                        0 => egui::CursorIcon::Move,
                        1 | 3 => egui::CursorIcon::ResizeNwSe,
                        2 | 4 => egui::CursorIcon::ResizeNeSw,
                        5 | 7 => egui::CursorIcon::ResizeVertical,
                        6 | 8 => egui::CursorIcon::ResizeHorizontal,
                        _ => egui::CursorIcon::Grab,
                    })
                    .on_hover_text(match index {
                        0 => "Move entire mask",
                        9 => "Rotate mask about its centre. Shift snaps to 15 degrees.",
                        1..=4 => "Scale from the opposite corner",
                        _ => "Stretch mask from the opposite edge",
                    });
            if index == 0 {
                painter.line_segment(
                    [
                        position - egui::vec2(7.0, 0.0),
                        position + egui::vec2(7.0, 0.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        position - egui::vec2(0.0, 7.0),
                        position + egui::vec2(0.0, 7.0),
                    ],
                    stroke,
                );
            } else if index == 9 {
                painter.circle_filled(position, 5.0, Color32::from_gray(22));
                painter.circle_stroke(position, 5.0, stroke);
            } else {
                let r = Rect::from_center_size(position, Vec2::splat(8.0));
                painter.rect_filled(r, 1.0, Color32::from_gray(22));
                painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
            }
            responses.push((index, response));
        }
        for (index, response) in responses {
            if response.dragged() && ui.ctx().dragged_id() == Some(response.id) {
                if self.drag.is_none() {
                    let pointer = ui
                        .input(|i| i.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos());
                    if let Some(pointer) = pointer {
                        self.drag = Some(Drag {
                            original: path.clone(),
                            bounds,
                            pointer: local(pointer),
                            handle: index,
                        });
                    }
                }
                if let (Some(drag), Some(pointer)) = (&self.drag, response.interact_pointer_pos()) {
                    let next = transformed(
                        drag,
                        local(pointer),
                        self.proportional,
                        ui.input(|i| i.modifiers.shift),
                    );
                    self.limited = next.is_none();
                    if let Some(next) = next
                        && next != *path
                    {
                        *path = next;
                        changed = true;
                    }
                }
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.drag = None;
        }
        let hint = if self.limited {
            "Coordinate limit reached. Move back to continue."
        } else {
            "Drag inside to move · corners scale · sides stretch · round handle rotates · Apply saves"
        };
        painter.text(
            canvas.left_bottom() + egui::vec2(12.0, -14.0),
            egui::Align2::LEFT_BOTTOM,
            hint,
            egui::FontId::proportional(13.0),
            color,
        );
        changed
    }
}
fn transformed(drag: &Drag, pointer: Pos2, proportional: bool, snap: bool) -> Option<PathMask> {
    let delta = pointer - drag.pointer;
    let b = drag.bounds;
    let mut pivot = b.center();
    let mut scales = Vec2::splat(1.0);
    let mut angle = 0.0_f32;
    let mut translation = Vec2::ZERO;
    match drag.handle {
        0 => translation = delta,
        9 => {
            let a = drag.pointer - pivot;
            let z = pointer - pivot;
            if a.length_sq() < 0.0001 || z.length_sq() < 0.0001 {
                return None;
            }
            angle = z.y.atan2(z.x) - a.y.atan2(a.x);
            if snap {
                let step = 15.0_f32.to_radians();
                angle = (angle / step).round() * step;
            }
        }
        handle => {
            let (anchor, target, axis) = match handle {
                1 => (b.right_bottom(), b.left_top(), [true, true]),
                2 => (b.left_bottom(), b.right_top(), [true, true]),
                3 => (b.left_top(), b.right_bottom(), [true, true]),
                4 => (b.right_top(), b.left_bottom(), [true, true]),
                5 => (b.center_bottom(), b.center_top(), [false, true]),
                6 => (b.left_center(), b.right_center(), [true, false]),
                7 => (b.center_top(), b.center_bottom(), [false, true]),
                8 => (b.right_center(), b.left_center(), [true, false]),
                _ => return None,
            };
            pivot = anchor;
            let original = target - anchor;
            let desired = original + delta;
            if proportional && handle <= 4 {
                let length = original.length_sq();
                if length < 0.0001 {
                    return None;
                }
                scales = Vec2::splat((desired.dot(original) / length).max(0.001));
            } else {
                if axis[0] && original.x.abs() > 0.0001 {
                    scales.x = (desired.x / original.x).max(0.001);
                }
                if axis[1] && original.y.abs() > 0.0001 {
                    scales.y = (desired.y / original.y).max(0.001);
                }
            }
        }
    }
    let (sin, cos) = angle.sin_cos();
    let mut result = drag.original.clone();
    let map = |p: &mut [f32; 2]| {
        let q = (egui::pos2(p[0], p[1]) - pivot) * scales;
        let q = pivot + egui::vec2(q.x * cos - q.y * sin, q.x * sin + q.y * cos) + translation;
        *p = [q.x, q.y];
    };
    for command in &mut result.commands {
        match command {
            PathCommand::Move { to } | PathCommand::Line { to } => map(to),
            PathCommand::Quad { control, to } => {
                map(control);
                map(to);
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                map(control1);
                map(control2);
                map(to);
            }
            PathCommand::Close {} => {}
        }
    }
    result.validate().ok()?;
    Some(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn drag(handle: usize, pointer: Pos2) -> Drag {
        Drag {
            original: PathMask::default(),
            bounds: Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(400.0, 300.0)),
            pointer,
            handle,
        }
    }
    #[test]
    fn moves_every_contour_and_control_without_changing_topology() {
        let mut d = drag(0, egui::pos2(200.0, 200.0));
        d.original.commands.extend(PathMask::default().commands);
        d.original.commands[1] = PathCommand::Cubic {
            control1: [150.0, 50.0],
            control2: [350.0, 50.0],
            to: [400.0, 100.0],
        };
        let p = transformed(&d, egui::pos2(210.0, 180.0), true, false).unwrap();
        assert_eq!(
            p.commands[1],
            PathCommand::Cubic {
                control1: [160.0, 30.0],
                control2: [360.0, 30.0],
                to: [410.0, 80.0]
            }
        );
        assert_eq!(p.commands[5], PathCommand::Move { to: [110.0, 80.0] });
        assert_eq!(p.fill_rule, d.original.fill_rule);
        assert_eq!(transformed(&d, d.pointer, true, false).unwrap(), d.original);
    }
    #[test]
    fn scales_about_opposite_corner_and_edges_keep_other_axis() {
        let d = drag(3, egui::pos2(400.0, 300.0));
        let p = transformed(&d, egui::pos2(700.0, 500.0), true, false).unwrap();
        assert_eq!(p.commands[0], PathCommand::Move { to: [100.0, 100.0] });
        assert_eq!(p.commands[2], PathCommand::Line { to: [700.0, 500.0] });
        let d = drag(6, egui::pos2(400.0, 200.0));
        let p = transformed(&d, egui::pos2(700.0, 250.0), true, false).unwrap();
        assert_eq!(p.commands[2], PathCommand::Line { to: [700.0, 300.0] });
    }
    #[test]
    fn rotation_is_about_center_and_invalid_moves_are_rejected() {
        let d = drag(9, egui::pos2(250.0, 100.0));
        let p = transformed(&d, egui::pos2(350.0, 200.0), true, true).unwrap();
        let PathCommand::Move { to } = p.commands[0] else {
            panic!()
        };
        assert!((to[0] - 350.0).abs() < 0.001 && (to[1] - 50.0).abs() < 0.001);
        let d = drag(0, egui::pos2(200.0, 200.0));
        assert!(transformed(&d, egui::pos2(40000.0, 200.0), true, false).is_none());
        assert_eq!(d.original, PathMask::default());
    }
}
