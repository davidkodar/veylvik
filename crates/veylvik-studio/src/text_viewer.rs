//! Readable in-viewer editing, independent of the processed title's appearance.
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use veylvik_document::{NodeId, Operation, Time};
#[derive(Default)]
pub struct Editor {
    pub active: bool,
    pub guides: bool,
    pub layout: bool,
    pub snap: bool,
    selected: Option<NodeId>,
    focus: bool,
    warning: String,
    cached: Option<(Vec<u8>, [f32; 4])>,
}
impl Editor {
    pub fn toggle(&mut self, id: NodeId) {
        self.selected = Some(id);
        self.active = !self.active;
        self.focus = self.active;
    }

    #[allow(clippy::too_many_arguments)] // The overlay shares the viewer geometry and document edit target.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        transform: crate::viewer::ViewTransform,
        image: Vec2,
        id: NodeId,
        operation: &mut Operation,
        time: Time,
    ) -> bool {
        if self.selected != Some(id) {
            self.active = false;
            self.selected = Some(id);
            self.cached = None;
        }
        let [ox, oy, sx, sy] = transform.0;
        let scale = egui::vec2(
            canvas.width() * sx / image.x,
            canvas.height() * sy / image.y,
        );
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let screen = |p: [f32; 2]| origin + egui::vec2(p[0], p[1]) * scale;
        let painter = ui.painter().with_clip_rect(canvas);
        if self.guides {
            for inset in [0.05, 0.1] {
                painter.rect_stroke(
                    Rect::from_min_max(
                        origin + image * scale * inset,
                        origin + image * scale * (1.0 - inset),
                    ),
                    0.0,
                    Stroke::new(1.0_f32, Color32::from_gray(150)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let key = if let Operation::Text(veylvik_document::nodes::Text {
            content,
            font,
            alignment,
            rtl,
            language,
            underline,
            features,
            typography,
            ..
        }) = &operation
        {
            let values: Vec<_> = operation
                .descriptor()
                .parameters
                .iter()
                .filter_map(|p| operation.scalar(p.name).map(|s| s.at(time).unwrap_or(0.0)))
                .collect();
            let spans: Vec<_> = typography
                .spans
                .iter()
                .map(|s| (s.start, s.end, s.scale, s.font.as_ref().map(|f| f.label())))
                .collect();
            serde_json::to_vec(&(
                content,
                font.label(),
                alignment,
                rtl,
                language,
                underline,
                features,
                typography.anchor,
                typography.vertical,
                typography.auto_fit,
                spans,
                &values,
            ))
            .unwrap_or_default()
        } else {
            vec![]
        };
        if self.cached.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut reference = operation.clone();
            if let Operation::Text(veylvik_document::nodes::Text { typography, .. }) =
                &mut reference
            {
                typography.motion = veylvik_document::text::TextMotion::None;
            }
            match veylvik_text::outline(&reference, time) {
                Ok(path) => {
                    self.warning.clear();
                    let sample = |name| {
                        reference
                            .scalar(name)
                            .and_then(|s| s.at(time).ok())
                            .unwrap_or(0.0)
                    };
                    let w = sample("wrap_width");
                    let h = sample("box_height");
                    let padding = sample("padding");
                    let sx = sample("scale_x");
                    let sy = sample("scale_y");
                    if w > 0.0 && h > 0.0 && sx.abs() > 0.0001 && sy.abs() > 0.0001 {
                        let (sin, cos) = sample("rotation").to_radians().sin_cos();
                        let x = sample("x");
                        let y = sample("y");
                        let b = veylvik_text::mapped_bounds(&path, |p| {
                            let dx = p[0] - x;
                            let dy = p[1] - y;
                            [(dx * cos + dy * sin) / sx, (-dx * sin + dy * cos) / sy]
                        });
                        if b[2] - b[0] > w - padding * 2.0 + 1.0
                            || b[3] - b[1] > h - padding * 2.0 + 1.0
                        {
                            self.warning="Text exceeds the layout box. Enlarge it, reduce size, or enable auto-fit.".into();
                        }
                    }
                    self.cached = Some((key, veylvik_text::bounds(&path)));
                }
                Err(e) => {
                    painter.text(
                        canvas.left_top() + egui::vec2(10.0, 10.0),
                        egui::Align2::LEFT_TOP,
                        e.to_string(),
                        egui::FontId::proportional(14.0),
                        Color32::LIGHT_RED,
                    );
                }
            }
        }
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new("text-layout-warning"), self.warning.clone())
        });
        let bounds = self
            .cached
            .as_ref()
            .map_or([0.0, 0.0, 200.0, 80.0], |(_, b)| *b);
        let hit = Rect::from_min_max(
            screen([bounds[0], bounds[1]]),
            screen([bounds[2], bounds[3]]),
        )
        .expand(8.0)
        .intersect(canvas);
        let start = ui.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                && i.pointer.interact_pos().is_some_and(|p| hit.contains(p))
        });
        if start {
            self.active = true;
            self.focus = true;
        }
        let just_opened = self.focus;
        let mut changed = false;
        if self.active {
            let Operation::Text(veylvik_document::nodes::Text {
                content,
                typography,
                ..
            }) = operation
            else {
                return false;
            };
            let before = content.clone();
            // Keep the editor stable as wording changes, and leave the processed title visible.
            let width = 480.0_f32.min((canvas.width() - 20.0).max(1.0));
            let height = 110.0_f32.min((canvas.height() - 20.0).max(1.0));
            let pos = egui::pos2(
                canvas.center().x - width * 0.5,
                canvas.bottom() - height - 10.0,
            );
            let rect = Rect::from_min_size(pos, egui::vec2(width, height));
            let mut finished = false;
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(18, 20, 25, 240))
                    .stroke(Stroke::new(1.0_f32, crate::design::ACCENT))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.small("Edit text · live preview above");
                            finished |= ui.small_button("Done").clicked();
                        });
                        let output = egui::TextEdit::multiline(content)
                            .id_salt(("viewer-text", id))
                            .desired_width(width - 12.0)
                            .desired_rows(3)
                            .font(egui::FontId::proportional(20.0))
                            .char_limit(4096)
                            .show(ui);
                        if let Some(range) = output.cursor_range {
                            let [a, b] = range.sorted_cursors();
                            if a.index != b.index {
                                let byte = |i| {
                                    content
                                        .char_indices()
                                        .nth(i)
                                        .map_or(content.len(), |(b, _)| b)
                                };
                                ui.ctx().data_mut(|d| {
                                    d.insert_temp(
                                        egui::Id::new("text-selection"),
                                        (byte(a.index), byte(b.index)),
                                    )
                                });
                            }
                        }
                        let response = output.response;
                        if self.focus {
                            self.focus = false;
                            response.request_focus();
                        }
                        changed |= response.changed();
                        finished |= ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if finished {
                            response.surrender_focus();
                        }
                    });
            });
            if !just_opened
                && ui.input(|i| {
                    i.pointer.any_pressed()
                        && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
                })
            {
                finished = true;
            }
            if changed {
                typography.remap(&before, content);
            }
            if finished {
                self.active = false;
            }
        } else if self.layout
            && let Operation::Text(veylvik_document::nodes::Text {
                wrap_width,
                box_height,
                x,
                y,
                typography,
                rotation,
                scale_x,
                scale_y,
                ..
            }) = operation
        {
            let w = wrap_width.at(time).unwrap_or(0.0);
            let h = box_height.at(time).unwrap_or(0.0);
            if w > 0.0 && h > 0.0 {
                let a = typography.anchor.unwrap_or([0.5, 0.5]);
                let pivot = egui::vec2(x.at(time).unwrap_or(0.0), y.at(time).unwrap_or(0.0));
                let angle = rotation.at(time).unwrap_or(0.0).to_radians();
                let (sin, cos) = angle.sin_cos();
                let xs = scale_x.at(time).unwrap_or(1.0);
                let ys = scale_y.at(time).unwrap_or(1.0);
                let map = |v: Vec2| {
                    let d = v * egui::vec2(xs, ys);
                    screen([
                        pivot.x + d.x * cos - d.y * sin,
                        pivot.y + d.x * sin + d.y * cos,
                    ])
                };
                let left = -w * a[0];
                let top = -h * a[1];
                let corners = [
                    egui::vec2(left, top),
                    egui::vec2(left + w, top),
                    egui::vec2(left + w, top + h),
                    egui::vec2(left, top + h),
                    egui::vec2(left, top),
                ];
                painter.add(egui::Shape::line(
                    corners.map(map).to_vec(),
                    Stroke::new(1.0_f32, crate::design::ACCENT),
                ));
                let corner = egui::vec2(
                    if a[0] > 0.5 { 0.0 } else { 1.0 },
                    if a[1] > 0.5 { 0.0 } else { 1.0 },
                );
                let handle = map(egui::vec2(left + w * corner.x, top + h * corner.y));
                painter.circle_filled(handle, 5.0, crate::design::ACCENT);
                let response = crate::handles::interact(
                    ui,
                    Rect::from_center_size(handle, Vec2::splat(16.0)).intersect(canvas),
                    ui.id().with("text-box-size"),
                );
                if response.dragged() && xs.abs() > 0.001 && ys.abs() > 0.001 {
                    let d = crate::handles::drag_delta(&response) / scale;
                    changed |= crate::handles::edit(
                        wrap_width,
                        time,
                        (d.x * cos + d.y * sin) / xs / (corner.x - a[0]),
                        1.0,
                        8192.0,
                    )
                    .is_ok();
                    changed |= crate::handles::edit(
                        box_height,
                        time,
                        (-d.x * sin + d.y * cos) / ys / (corner.y - a[1]),
                        1.0,
                        8192.0,
                    )
                    .is_ok();
                }
            }
        }
        changed
    }
}

/// Hit-test editable source contours only where their source coordinates remain valid.
pub fn pick(doc: &veylvik_document::Composition, time: Time, point: [f32; 2]) -> Option<NodeId> {
    doc.nodes
        .values()
        .filter(|n| {
            matches!(
                n.operation,
                Operation::Text(veylvik_document::nodes::Text { .. })
            ) && crate::editable_text::handles_in_scene(doc, n.id)
        })
        .filter_map(|node| {
            let mut op = node.operation.clone();
            if let Operation::Text(veylvik_document::nodes::Text { typography, .. }) = &mut op {
                typography.motion = veylvik_document::text::TextMotion::None;
            }
            let path = veylvik_text::outline(&op, time).ok()?;
            let b = veylvik_text::bounds(&path);
            (point[0] >= b[0] - 6.0
                && point[0] <= b[2] + 6.0
                && point[1] >= b[1] - 6.0
                && point[1] <= b[3] + 6.0)
                .then_some((node.id, (b[2] - b[0]) * (b[3] - b[1])))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}
