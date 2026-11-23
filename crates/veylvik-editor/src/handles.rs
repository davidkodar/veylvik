//! Shape and gradient authoring overlays. All changes stay in the inspector draft.
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use veylvik_document::{Operation, Result, Scalar, Time};

/// Claim the press origin, even when a motion event in the same UI frame has
/// already moved the pointer outside this small handle's hit rectangle.
pub(super) fn interact(ui: &egui::Ui, hit: Rect, id: egui::Id) -> egui::Response {
    let press = ui.input(|input| {
        (input.pointer.primary_pressed() && input.pointer.primary_down())
            .then(|| input.pointer.press_origin())
            .flatten()
    });
    if ui.is_enabled()
        && let Some(origin) = press
        && hit.contains(origin)
        && ui.clip_rect().contains(origin)
        && ui.ctx().layer_id_at(origin) == Some(ui.layer_id())
    {
        ui.ctx().set_dragged_id(id);
    }
    ui.interact(hit, id, Sense::drag())
}

pub(super) fn drag_delta(response: &egui::Response) -> Vec2 {
    response
        .ctx
        .input(|input| {
            input.pointer.primary_pressed().then(|| {
                input
                    .pointer
                    .interact_pos()
                    .zip(input.pointer.press_origin())
                    .map_or(Vec2::ZERO, |(current, origin)| current - origin)
            })
        })
        .unwrap_or_else(|| response.drag_delta())
}

pub(super) fn edit(scalar: &mut Scalar, time: Time, delta: f32, min: f32, max: f32) -> Result<()> {
    let value = (scalar.at(time)? + delta).clamp(min, max);
    if let Scalar::Constant { value: current } = scalar {
        *current = value;
        Ok(())
    } else {
        scalar.set_key_at(time, value)
    }
}

fn move_handle(operation: &mut Operation, handle: usize, delta: Vec2, time: Time) -> Result<()> {
    let mut next = operation.clone();
    match &mut next {
        Operation::Ellipse(veylvik_document::nodes::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            ..
        }) => match handle {
            0 => {
                edit(x, time, delta.x, -1_000_000.0, 1_000_000.0)?;
                edit(y, time, delta.y, -1_000_000.0, 1_000_000.0)?;
            }
            1 => edit(radius_x, time, delta.x, 0.0, 8192.0)?,
            2 => edit(radius_y, time, delta.y, 0.0, 8192.0)?,
            _ => unreachable!(),
        },
        Operation::Gradient(veylvik_document::nodes::Gradient {
            start_x,
            start_y,
            end_x,
            end_y,
            ..
        }) => {
            let (x, y) = if handle == 0 {
                (start_x, start_y)
            } else {
                (end_x, end_y)
            };
            edit(x, time, delta.x, -1_000_000.0, 1_000_000.0)?;
            edit(y, time, delta.y, -1_000_000.0, 1_000_000.0)?;
        }
        Operation::Particles(particles) => {
            let far = 1_000_000.0;
            match handle {
                0 => {
                    edit(&mut particles.x, time, delta.x, -far, far)?;
                    edit(&mut particles.y, time, delta.y, -far, far)?;
                }
                // The corner is half the box from its centre.
                1 => {
                    edit(&mut particles.width, time, 2.0 * delta.x, 0.0, 8192.0)?;
                    edit(&mut particles.height, time, 2.0 * delta.y, 0.0, 8192.0)?;
                }
                _ => {
                    edit(&mut particles.pull_x, time, delta.x, -far, far)?;
                    edit(&mut particles.pull_y, time, delta.y, -far, far)?;
                }
            }
        }
        // A force's point, and the rim of its reach.
        Operation::Force(force) => {
            let far = 1_000_000.0;
            if handle == 0 {
                edit(&mut force.x, time, delta.x, -far, far)?;
                edit(&mut force.y, time, delta.y, -far, far)?;
            } else {
                edit(&mut force.radius, time, delta.x, 0.0, far)?;
            }
        }
        Operation::GridWarp(warp) => {
            let reach = veylvik_document::nodes::GRID_REACH;
            if let Some(point) = warp.points.get_mut(handle) {
                point[0] = (point[0] + delta.x).clamp(-reach, reach);
                point[1] = (point[1] + delta.y).clamp(-reach, reach);
            }
        }
        _ => return Ok(()),
    }
    *operation = next;
    Ok(())
}

/// Whether the viewer edits a shape warp's end shape (otherwise its start shape).
pub fn editing_end_shape(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(egui::Id::new("shape-warp-end")))
        .unwrap_or(true)
}
pub fn edit_end_shape(ctx: &egui::Context, end: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("shape-warp-end"), end));
}
pub fn show(
    ui: &egui::Ui,
    canvas: Rect,
    transform: crate::viewer::ViewTransform,
    image: Vec2,
    operation: &mut Operation,
    time: Time,
) -> Result<bool> {
    if matches!(
        operation,
        Operation::Text(veylvik_document::nodes::Text { .. })
            | Operation::Affine(veylvik_document::nodes::Affine { .. })
            | Operation::Transform(veylvik_document::nodes::Transform { .. })
    ) {
        return crate::transform_handles::show(ui, canvas, transform, image, operation, time);
    }
    if let Operation::Path(veylvik_document::nodes::Path { geometry }) = operation {
        let mut shape = geometry.at(time)?;
        let changed = crate::path_handles::show(
            ui,
            canvas,
            transform,
            image,
            &mut shape,
            !geometry.keys.is_empty(),
        );
        if changed {
            geometry.edit_at(time, shape)?;
        }
        return Ok(changed);
    }
    let [ox, oy, sx, sy] = transform.0;
    let scale = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let painter = ui.painter().with_clip_rect(canvas);
    let color = crate::design::accent(ui.ctx());
    if let Operation::ShapeWarp(warp) = operation {
        // The shape being edited has handles; the other is drawn faintly, with
        // links showing which points of the two outlines belong together.
        let end = editing_end_shape(ui.ctx());
        let (start, target) = (warp.from.at(time)?, warp.to.at(time)?);
        if let (Ok(a), Ok(b)) = (
            veylvik_warp::samples(&start),
            veylvik_warp::samples(&target),
        ) && a.len() == b.len()
        {
            let screen = |p: &[f32; 2]| origin + egui::vec2(p[0], p[1]) * scale;
            let faint = Stroke::new(1.0_f32, color.gamma_multiply(0.45));
            for (a, b) in a.iter().zip(&b) {
                let other = if end { a } else { b };
                painter.add(egui::Shape::closed_line(
                    other.iter().map(screen).collect(),
                    faint,
                ));
                for (p, q) in a.iter().zip(b).step_by(8) {
                    painter.line_segment([screen(p), screen(q)], faint);
                }
            }
        }
        let path = if end { &mut warp.to } else { &mut warp.from };
        let mut shape = if end { target } else { start };
        let changed = crate::path_handles::show(
            ui,
            canvas,
            transform,
            image,
            &mut shape,
            !path.keys.is_empty(),
        );
        if changed {
            path.edit_at(time, shape)?;
        }
        return Ok(changed);
    }
    let ellipse = |center: Pos2, radii: Vec2| {
        let outline: Vec<Pos2> = (0..=96)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 96.0;
                center + egui::vec2(angle.cos(), angle.sin()) * radii
            })
            .collect();
        painter.add(egui::Shape::line(outline, Stroke::new(1.0_f32, color)));
    };
    let points = match operation {
        Operation::Ellipse(veylvik_document::nodes::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            ..
        }) => {
            let center = origin + egui::vec2(x.at(time)?, y.at(time)?) * scale;
            let radii = egui::vec2(radius_x.at(time)?, radius_y.at(time)?) * scale;
            ellipse(center, radii);
            vec![
                (
                    center,
                    "Drag ellipse center. Apply commits; Revert discards.",
                ),
                (
                    center + egui::vec2(radii.x.max(18.0), 0.0),
                    "Drag horizontal radius. Apply commits; Revert discards.",
                ),
                (
                    center + egui::vec2(0.0, radii.y.max(18.0)),
                    "Drag vertical radius. Apply commits; Revert discards.",
                ),
            ]
        }
        Operation::Gradient(veylvik_document::nodes::Gradient {
            mode,
            start_x,
            start_y,
            end_x,
            end_y,
        }) => {
            let start = egui::vec2(start_x.at(time)?, start_y.at(time)?);
            let end = egui::vec2(end_x.at(time)?, end_y.at(time)?);
            let center = origin + start * scale;
            let endpoint = origin + end * scale;
            painter.line_segment([center, endpoint], Stroke::new(1.0_f32, color));
            if *mode == veylvik_document::GradientMode::Radial {
                ellipse(center, Vec2::splat((end - start).length()) * scale);
            }
            let delta = endpoint - center;
            let endpoint = if delta.length() < 18.0 {
                center
                    + if delta == Vec2::ZERO {
                        egui::vec2(18.0, 0.0)
                    } else {
                        delta.normalized() * 18.0
                    }
            } else {
                endpoint
            };
            vec![
                (
                    center,
                    "Drag gradient start. Apply commits; Revert discards.",
                ),
                (
                    endpoint,
                    "Drag gradient end. Apply commits; Revert discards.",
                ),
            ]
        }
        Operation::Particles(particles) => {
            // The emitter box (when nothing is connected as the emitter) and the pull point.
            let boxed = ui
                .data(|d| d.get_temp(egui::Id::new("particles-box")))
                .unwrap_or(true);
            let centre = origin + egui::vec2(particles.x.at(time)?, particles.y.at(time)?) * scale;
            let half =
                egui::vec2(particles.width.at(time)?, particles.height.at(time)?) * 0.5 * scale;
            let pull =
                origin + egui::vec2(particles.pull_x.at(time)?, particles.pull_y.at(time)?) * scale;
            let pulling = particles.pull.at(time)? != 0.0 || particles.vortex.at(time)? != 0.0;
            let mut points = Vec::new();
            if boxed {
                painter.rect_stroke(
                    Rect::from_center_size(centre, half * 2.0),
                    0.0,
                    Stroke::new(1.0_f32, color),
                    egui::StrokeKind::Middle,
                );
                points.push((
                    centre,
                    "Drag where particles are born. Apply commits; Revert discards.",
                ));
                points.push((
                    centre + egui::vec2(half.x.max(18.0), half.y.max(18.0)),
                    "Drag to size the box particles are born in. Apply commits; Revert discards.",
                ));
            }
            if pulling {
                ellipse(pull, Vec2::splat(10.0));
                // Keep the handle numbers fixed: the pull point is always the third.
                while points.len() < 2 {
                    points.push((Pos2::new(f32::NAN, f32::NAN), ""));
                }
                points.push((
                    pull,
                    "Drag the point particles are pulled to or swung round. Apply commits; Revert discards.",
                ));
            }
            points
        }
        Operation::Force(force) => {
            let point = origin + egui::vec2(force.x.at(time)?, force.y.at(time)?) * scale;
            let radius = force.radius.at(time)? * scale.x;
            ellipse(point, Vec2::splat(10.0));
            let mut points = vec![(
                point,
                "Drag the force's point. Apply commits; Revert discards.",
            )];
            if radius > 0.0 {
                painter.circle_stroke(point, radius, Stroke::new(1.0_f32, color));
                points.push((
                    point + egui::vec2(radius, 0.0),
                    "Drag to size how far the force reaches. Apply commits; Revert discards.",
                ));
            }
            points
        }
        Operation::GridWarp(warp) => {
            // The mesh as it stands: rest positions spread over the frame, plus offsets.
            let (columns, rows) = (warp.columns as usize, warp.rows as usize);
            let at = |i: usize, j: usize| {
                let rest = egui::vec2(
                    image.x * i as f32 / (columns - 1) as f32,
                    image.y * j as f32 / (rows - 1) as f32,
                );
                let offset = warp
                    .points
                    .get(j * columns + i)
                    .copied()
                    .unwrap_or([0.0; 2]);
                origin + (rest + egui::vec2(offset[0], offset[1])) * scale
            };
            let line = Stroke::new(1.0_f32, color.gamma_multiply(0.6));
            for j in 0..rows {
                painter.add(egui::Shape::line(
                    (0..columns).map(|i| at(i, j)).collect(),
                    line,
                ));
            }
            for i in 0..columns {
                painter.add(egui::Shape::line(
                    (0..rows).map(|j| at(i, j)).collect(),
                    line,
                ));
            }
            (0..rows)
                .flat_map(|j| (0..columns).map(move |i| (i, j)))
                .map(|(i, j)| {
                    (
                        at(i, j),
                        "Drag to move the image under this point. Apply commits; Revert discards.",
                    )
                })
                .collect()
        }
        _ => return Ok(false),
    };
    let mut changed = false;
    // Keep handles separated for zero-radius ellipses and coincident endpoints.
    for (index, (point, hint)) in points.into_iter().enumerate() {
        let hit = Rect::from_center_size(point, Vec2::splat(16.0)).intersect(canvas);
        if !hit.is_positive() {
            continue;
        }
        let response = interact(ui, hit, ui.id().with(("geometry_handle", index)))
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(hint);
        painter.circle_filled(point, 5.0, Color32::from_gray(25));
        painter.circle_stroke(point, 5.0, Stroke::new(1.5_f32, color));
        let delta = drag_delta(&response);
        if response.dragged() && delta != Vec2::ZERO {
            move_handle(operation, index, delta / scale, time)?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Interpolation, Key};
    #[test]
    fn gradient_handles_edit_endpoints_independently() {
        let mut operation = Operation::Gradient(veylvik_document::nodes::Gradient {
            mode: veylvik_document::GradientMode::Radial,
            start_x: 10.0.into(),
            start_y: 20.0.into(),
            end_x: 100.0.into(),
            end_y: 200.0.into(),
        });
        let time = Time::new(0, 1).unwrap();
        move_handle(&mut operation, 1, egui::vec2(-5.0, 8.0), time).unwrap();
        assert_eq!(operation.scalar("start_x").unwrap().at(time).unwrap(), 10.0);
        assert_eq!(operation.scalar("end_x").unwrap().at(time).unwrap(), 95.0);
        assert_eq!(operation.scalar("end_y").unwrap().at(time).unwrap(), 208.0);
        let start = operation.scalar_mut("start_x").unwrap();
        start.set_key_at(time, 10.0).unwrap();
        start.set_interpolation(Interpolation::Monotone);
        move_handle(
            &mut operation,
            0,
            egui::vec2(2.0, 3.0),
            Time::new(1, 1).unwrap(),
        )
        .unwrap();
        let start = operation.scalar("start_x").unwrap();
        assert_eq!(start.interpolation(), Some(Interpolation::Monotone));
        assert_eq!(start.keys().unwrap().len(), 2);
        assert_eq!(start.at(time).unwrap(), 10.0);
        assert_eq!(start.at(Time::new(1, 1).unwrap()).unwrap(), 12.0);
    }
    #[test]
    fn handle_edits_keep_constants_and_preserve_exact_animation() {
        let mut operation = Operation::Ellipse(veylvik_document::nodes::Ellipse {
            x: 10.0.into(),
            y: Scalar::Smooth {
                keys: vec![Key {
                    segment: None,
                    time: Time { ticks: 0, rate: 24 },
                    value: 20.0,
                }],
            },
            radius_x: 100.0.into(),
            radius_y: 50.0.into(),
            feather: 1.0.into(),
        });
        let time = Time { ticks: 0, rate: 1 };
        move_handle(&mut operation, 0, egui::vec2(5.0, -2.0), time).unwrap();
        assert!(
            matches!(operation.scalar("x").unwrap(), Scalar::Constant { value } if *value == 15.0)
        );
        let y = operation.scalar("y").unwrap();
        assert_eq!(y.interpolation(), Some(Interpolation::Smooth));
        assert_eq!(y.keys().unwrap()[0].time.rate, 24);
        assert_eq!(y.at(time).unwrap(), 18.0);
        move_handle(&mut operation, 1, egui::vec2(-200.0, 0.0), time).unwrap();
        assert_eq!(operation.scalar("radius_x").unwrap().at(time).unwrap(), 0.0);
        let before = serde_json::to_vec(&operation).unwrap();
        assert!(
            move_handle(
                &mut operation,
                0,
                Vec2::splat(3.0),
                Time { ticks: 0, rate: 0 }
            )
            .is_err()
        );
        assert_eq!(serde_json::to_vec(&operation).unwrap(), before);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;

    #[test]
    fn coalesced_press_and_motion_move_the_handle_without_panning() {
        let ctx = egui::Context::default();
        let mut operation = Operation::Ellipse(veylvik_document::nodes::Ellipse {
            x: 100.0.into(),
            y: 100.0.into(),
            radius_x: 25.0.into(),
            radius_y: 25.0.into(),
            feather: 1.0.into(),
        });
        let mut nav = crate::viewer::Navigation::default();
        let mut origin = Pos2::ZERO;
        let mut run = |events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        let image = egui::vec2(320.0, 240.0);
                        let (canvas, response) =
                            ui.allocate_exact_size(image, Sense::click_and_drag());
                        nav.interact(ui, &response, canvas, image);
                        origin = canvas.min + egui::vec2(100.0, 100.0);
                        show(
                            ui,
                            canvas,
                            nav.transform(canvas, image, 1.0),
                            image,
                            &mut operation,
                            Time::new(0, 1).unwrap(),
                        )
                        .unwrap();
                    });
                },
            );
            origin
        };
        let origin = run(vec![]);
        run(vec![egui::Event::PointerMoved(origin)]);
        let moved = origin + egui::vec2(40.0, 20.0);
        run(vec![
            egui::Event::PointerButton {
                pos: origin,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(moved),
        ]);
        run(vec![egui::Event::PointerButton {
            pos: moved,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(nav.pan(), [0.0, 0.0]);
        let time = Time::new(0, 1).unwrap();
        assert_eq!(operation.scalar("x").unwrap().at(time).unwrap(), 140.0);
        assert_eq!(operation.scalar("y").unwrap().at(time).unwrap(), 120.0);
    }
}
