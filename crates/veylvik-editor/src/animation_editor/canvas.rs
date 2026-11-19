use super::*;

impl Editor {
    pub(super) fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        context: &CanvasContext<'_>,
        track: Track<'_>,
        actions: &mut Vec<Action>,
    ) {
        let CanvasContext {
            doc,
            revision,
            current,
            now,
            focus,
            range,
        } = *context;
        let node = track.node;
        let parameter = track.parameter;
        let scalar = node.operation.scalar(parameter).unwrap();
        let descriptor = node
            .operation
            .descriptor()
            .parameters
            .iter()
            .find(|p| p.name == parameter)
            .unwrap();
        ui.add(
            egui::Label::new(format!(
                "{} / {}",
                node.label.chars().take(80).collect::<String>(),
                crate::names::setting(parameter, descriptor.unit)
            ))
            .truncate(),
        );
        let height = track.height;
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().max(50.0), height),
            egui::Sense::click_and_drag(),
        );
        let graph = rect.shrink2(Vec2::new(12.0, 22.0));
        let is_active = self
            .active
            .as_ref()
            .is_some_and(|(id, p)| *id == node.id && p == parameter);
        let current_scalar = if is_active {
            self.drag
                .as_ref()
                .and_then(|d| d.preview.as_ref())
                .unwrap_or(scalar)
        } else {
            self.drag
                .as_ref()
                .and_then(|d| d.group_preview.as_ref())
                .and_then(|edits| {
                    edits
                        .iter()
                        .find(|(id, p, _)| *id == node.id && p == parameter)
                })
                .map(|(_, _, s)| s)
                .unwrap_or(scalar)
        };
        let plot = crate::curve_preview::plot(current_scalar, now, graph.width());
        let (min, max) = self
            .drag
            .as_ref()
            .map(|d| (d.min, d.max))
            .unwrap_or_else(|| plot.as_ref().map(|p| (p.min, p.max)).unwrap_or((-1.0, 1.0)));
        let curve_view = self.curve;
        let point = |key: &Key| {
            Pos2::new(
                x_at(frame_number(key.time, doc.frame_rate), graph, range),
                if curve_view {
                    graph.bottom()
                        - ((f64::from(key.value) - min) / (max - min)) as f32 * graph.height()
                } else {
                    graph.center().y
                },
            )
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        for i in 0..=8 {
            let frame = range[0] + (range[1] - range[0]) * f64::from(i) / 8.0;
            let x = x_at(frame, graph, range);
            painter.vline(
                x,
                graph.y_range(),
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            painter.text(
                Pos2::new(x, rect.top() + 3.0),
                egui::Align2::CENTER_TOP,
                format!("{frame:.0}"),
                egui::FontId::monospace(10.0),
                ui.visuals().weak_text_color(),
            );
        }
        if self.curve {
            if let Some(plot) = &plot {
                let keys = current_scalar.keys().unwrap();
                let first = frame_number(keys[0].time, doc.frame_rate);
                let last = frame_number(keys[keys.len() - 1].time, doc.frame_rate);
                let points = plot
                    .points
                    .iter()
                    .map(|p| {
                        let value = plot.max - f64::from(p[1]) * (plot.max - plot.min);
                        Pos2::new(
                            x_at(first + f64::from(p[0]) * (last - first), graph, range),
                            graph.bottom() - ((value - min) / (max - min)) as f32 * graph.height(),
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    points,
                    (1.5, ui.visuals().selection.stroke.color),
                ));
            }
            painter.text(
                graph.left_top(),
                egui::Align2::LEFT_TOP,
                format!("{max:.3}"),
                egui::FontId::monospace(10.0),
                ui.visuals().weak_text_color(),
            );
            painter.text(
                graph.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                format!("{min:.3}"),
                egui::FontId::monospace(10.0),
                ui.visuals().weak_text_color(),
            );
        }
        for (index, key) in current_scalar.keys().unwrap_or(&[]).iter().enumerate() {
            let at = point(key);
            if graph.expand(5.0).contains(at) {
                self.hit_keys.push((node.id, parameter.into(), index, at));
                let selected = if is_active {
                    self.selection.contains(&index)
                } else {
                    self.linked
                        .get(&(node.id, parameter.into()))
                        .is_some_and(|keys| keys.contains(&index))
                };
                painter.circle_filled(
                    at,
                    if selected { 5.0 } else { 3.5 },
                    if selected {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().text_color()
                    },
                );
            }
        }
        painter.vline(
            x_at(current as f64, graph, range),
            graph.y_range(),
            (1.0, egui::Color32::GOLD),
        );
        let controls = if self.curve && is_active {
            tangents::controls(
                current_scalar,
                &self.selection,
                graph,
                range,
                doc.frame_rate,
                min,
                max,
            )
        } else {
            Vec::new()
        };
        for (_, _, handle, endpoint) in &controls {
            painter.line_segment([*endpoint, *handle], (1.0, egui::Color32::GOLD));
            painter.rect_filled(
                Rect::from_center_size(*handle, Vec2::splat(7.0)),
                1.0,
                egui::Color32::GOLD,
            );
        }
        let pointer = response.interact_pointer_pos();
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        if pressed
            && response.hovered()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin()).or(pointer)
        {
            ui.memory_mut(|m| m.request_focus(focus));
            if !is_active {
                self.activate(node.id, parameter, ui.input(|i| i.modifiers.shift));
                self.selected_node = Some(node.id);
                actions.push(Action::Select(node.id));
            }
            let handle_hit = controls
                .iter()
                .filter(|(_, _, p, _)| p.distance(origin) <= 8.0)
                .min_by(|a, b| a.2.distance(origin).total_cmp(&b.2.distance(origin)))
                .map(|(i, right, _, _)| (*i, *right));
            let hit = scalar
                .keys()
                .unwrap_or(&[])
                .iter()
                .enumerate()
                .map(|(i, k)| (i, point(k).distance(origin)))
                .filter(|(_, d)| *d <= 9.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            if let Some(handle) = handle_hit {
                self.drag = Some(Drag {
                    original: scalar.clone(),
                    handle: Some(handle),
                    companions: Selection::new(),
                    group_preview: None,
                    selection: self.selection.clone(),
                    origin,
                    revision,
                    preview: None,
                    moved: false,
                    min,
                    max,
                });
            } else if let Some(index) = hit {
                if ui.input(|i| i.modifiers.shift) {
                    if !self.selection.insert(index) {
                        self.selection.remove(&index);
                    }
                } else if !self.selection.contains(&index) {
                    self.selection = [index].into();
                    self.linked.clear();
                }
                if self.selection.contains(&index) {
                    self.drag = Some(Drag {
                        original: scalar.clone(),
                        handle: None,
                        companions: self.linked.clone(),
                        group_preview: None,
                        selection: self.selection.clone(),
                        origin,
                        revision,
                        preview: None,
                        moved: false,
                        min,
                        max,
                    });
                }
            } else {
                let base = if ui.input(|i| i.modifiers.shift) {
                    self.selections()
                } else {
                    Selection::new()
                };
                self.install_selection(base.clone());
                self.marquee = Some(Marquee { origin, base });
                actions.push(Action::Seek(
                    frame_at(origin.x, graph, range).clamp(0, i64::from(doc.frame_count) - 1),
                ));
            }
        }
        if self
            .active
            .as_ref()
            .is_some_and(|(id, p)| *id == node.id && p == parameter)
            && let Some(drag) = &mut self.drag
        {
            if let Some(pointer) = pointer {
                let delta = pointer - drag.origin;
                if delta.length() > 2.0 {
                    drag.moved = true;
                }
                if drag.moved {
                    let offset =
                        (f64::from(delta.x / graph.width()) * (range[1] - range[0])).round() as i64;
                    let value = if self.curve && drag.companions.is_empty() {
                        -f64::from(delta.y / graph.height()) * (drag.max - drag.min)
                    } else {
                        0.0
                    };
                    let result = if let Some((index, right)) = drag.handle {
                        tangents::moved_handle(
                            &drag.original,
                            (index, right),
                            delta,
                            graph,
                            range,
                            doc.frame_rate,
                            [drag.min, drag.max],
                        )
                    } else {
                        Time::at_frame(offset, doc.frame_rate).and_then(|offset| {
                            key_edit::moved(&drag.original, &drag.selection, offset, value)
                        })
                    }
                    .and_then(|edited| {
                        if edited.keys().unwrap().iter().any(|k| {
                            k.value < descriptor.min.unwrap_or(f32::MIN)
                                || k.value > descriptor.max.unwrap_or(f32::MAX)
                        }) {
                            Err(veylvik_document::Error(
                                "Value outside parameter range".into(),
                            ))
                        } else {
                            Ok(edited)
                        }
                    });
                    let result = result.and_then(|edited| {
                        let mut operation = node.operation.clone();
                        *operation.scalar_mut(parameter).unwrap() = edited.clone();
                        operation.validate()?;
                        let others = Time::at_frame(offset, doc.frame_rate).and_then(|offset| {
                            groups::moved_group(doc, &drag.companions, offset)
                        })?;
                        drag.group_preview = Some(others);
                        Ok(edited)
                    });
                    match result {
                        Ok(edited) => {
                            drag.preview = Some(edited);
                            self.error = None;
                        }
                        Err(error) => {
                            drag.preview = None;
                            self.error = Some(error.to_string());
                        }
                    }
                }
            }
            if ui.input(|i| i.pointer.primary_released()) {
                let drag = self.drag.take().unwrap();
                if drag.moved
                    && let Some(edited) = drag.preview
                    && serde_json::to_value(&edited).ok()
                        != serde_json::to_value(&drag.original).ok()
                {
                    if drag.companions.is_empty() {
                        if drag.handle.is_some() {
                            self.pending_selection = Some(self.selection.clone());
                        }
                        self.commit(Ok(edited), drag.revision, actions);
                    } else {
                        let mut edits = drag.group_preview.unwrap_or_default();
                        edits.push((node.id, parameter.into(), edited));
                        self.batch(Ok(edits), doc, drag.revision, actions);
                    }
                }
            }
        }
        if response.hovered() && !self.dragging() {
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            if scroll != Vec2::ZERO {
                let span = range[1] - range[0];
                if ui.input(|i| i.modifiers.shift) {
                    let shift = f64::from(scroll.x + scroll.y) * span / 500.0;
                    let center = ((range[0] + range[1]) * 0.5 - shift).clamp(-1.0e12, 1.0e12);
                    self.range = Some([center - span * 0.5, center + span * 0.5]);
                } else {
                    let anchor = response
                        .hover_pos()
                        .map(|p| f64::from((p.x - graph.left()) / graph.width()).clamp(0.0, 1.0))
                        .unwrap_or(0.5);
                    let size =
                        (span * (-f64::from(scroll.y) * 0.005).exp()).clamp(1.0, 2_000_000.0);
                    let start = range[0] + (span - size) * anchor;
                    self.range = Some([start, start + size]);
                }
            }
        }
    }
}
