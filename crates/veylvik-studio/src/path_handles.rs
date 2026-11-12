//! Path overlays edit the same geometry used by the Vulkan mask renderer.
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use veylvik_document::vector::{MAX_PATH_COORDINATE, PathCommand, PathMask};

fn points(command: &mut PathCommand) -> Vec<&mut [f32; 2]> {
    match command {
        PathCommand::Move { to } | PathCommand::Line { to } => vec![to],
        PathCommand::Quad { control, to } => vec![control, to],
        PathCommand::Cubic {
            control1,
            control2,
            to,
        } => vec![control1, control2, to],
        PathCommand::Close {} => vec![],
    }
}

pub fn show(
    ui: &egui::Ui,
    canvas: Rect,
    transform: crate::viewer::ViewTransform,
    image: Vec2,
    path: &mut PathMask,
) -> bool {
    let [ox, oy, sx, sy] = transform.0;
    let scale = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    if !scale.is_finite() || scale.x <= 0.0 || scale.y <= 0.0 {
        return false;
    }
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let screen = |p: [f32; 2]| origin + egui::vec2(p[0], p[1]) * scale;
    let painter = ui.painter().with_clip_rect(canvas);
    let color = Color32::from_rgb(255, 190, 80);
    let stroke = Stroke::new(1.5_f32, color);
    let mut start = [0.0; 2];
    let mut previous = start;
    for command in &path.commands {
        match *command {
            PathCommand::Move { to } => {
                start = to;
                previous = to;
            }
            PathCommand::Line { to } => {
                painter.line_segment([screen(previous), screen(to)], stroke);
                previous = to;
            }
            PathCommand::Quad { control, to } => {
                painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [screen(previous), screen(control), screen(to)],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                ));
                painter.line_segment(
                    [screen(previous), screen(control)],
                    Stroke::new(1.0_f32, Color32::GRAY),
                );
                painter.line_segment(
                    [screen(control), screen(to)],
                    Stroke::new(1.0_f32, Color32::GRAY),
                );
                previous = to;
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        screen(previous),
                        screen(control1),
                        screen(control2),
                        screen(to),
                    ],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                ));
                painter.line_segment(
                    [screen(previous), screen(control1)],
                    Stroke::new(1.0_f32, Color32::GRAY),
                );
                painter.line_segment(
                    [screen(control2), screen(to)],
                    Stroke::new(1.0_f32, Color32::GRAY),
                );
                previous = to;
            }
            PathCommand::Close {} => {
                painter.line_segment([screen(previous), screen(start)], stroke);
            }
        }
    }
    let mut changed = false;
    for (index, command) in path.commands.iter_mut().enumerate() {
        let mut controls = points(command);
        let last = controls.len().saturating_sub(1);
        for (handle, point) in controls.iter_mut().enumerate() {
            let position = screen(**point);
            let anchor = handle == last;
            let hit = Rect::from_center_size(position, Vec2::splat(14.0)).intersect(canvas);
            if !hit.is_positive() {
                continue;
            }
            let response =
                crate::handles::interact(ui, hit, ui.id().with(("path-handle", index, handle)))
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text(if anchor {
                        "Drag mask point. Apply commits; Revert discards."
                    } else {
                        "Drag curve handle. Apply commits; Revert discards."
                    });
            painter.circle_filled(
                position,
                if anchor { 5.0 } else { 3.5 },
                Color32::from_gray(25),
            );
            painter.circle_stroke(position, if anchor { 5.0 } else { 3.5 }, stroke);
            if response.dragged() {
                let delta = crate::handles::drag_delta(&response) / scale;
                changed |= move_point(point, delta);
            }
        }
    }
    changed
}

fn move_point(point: &mut [f32; 2], delta: Vec2) -> bool {
    if !delta.is_finite() {
        return false;
    }
    let before = *point;
    point[0] = (point[0] + delta.x).clamp(-MAX_PATH_COORDINATE, MAX_PATH_COORDINATE);
    point[1] = (point[1] + delta.y).clamp(-MAX_PATH_COORDINATE, MAX_PATH_COORDINATE);
    before != *point
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curve_handles_are_independent_and_bounded() {
        let mut command = PathCommand::Cubic {
            control1: [10.0, 20.0],
            control2: [30.0, 40.0],
            to: [50.0, 60.0],
        };
        assert!(move_point(points(&mut command)[1], Vec2::new(5.0, -8.0)));
        assert_eq!(
            command,
            PathCommand::Cubic {
                control1: [10.0, 20.0],
                control2: [35.0, 32.0],
                to: [50.0, 60.0]
            }
        );
        let mut p = [32760.0, -32760.0];
        assert!(move_point(&mut p, Vec2::new(100.0, -100.0)));
        assert_eq!(p, [32768.0, -32768.0]);
        assert!(!move_point(&mut p, Vec2::splat(f32::NAN)));
    }
}
