use super::*;
use veylvik_document::{Result, key_edit};

pub(super) struct Drag {
    pub offset: i64,
    origin: f32,
    duplicate: bool,
}

impl Keys {
    pub(super) fn commands(
        &self,
        doc: &Composition,
        offset: i64,
        duplicate: bool,
        delete: bool,
        current: i64,
    ) -> Result<Vec<Command>> {
        let time = Time::at_frame(offset, doc.frame_rate)?;
        let now = Time::at_frame(current, doc.frame_rate)?;
        let mut tracks = BTreeMap::<(NodeId, String), BTreeSet<usize>>::new();
        for mark in self
            .selected
            .iter()
            .filter_map(|f| self.marks.get(f))
            .flatten()
        {
            tracks
                .entry((mark.node, mark.parameter.clone()))
                .or_default()
                .insert(mark.index);
        }
        let mut nodes = BTreeMap::new();
        for ((id, parameter), indices) in tracks {
            let node = nodes.entry(id).or_insert_with(|| doc.nodes[&id].clone());
            if !delete {
                for index in &indices {
                    let original = if parameter == "geometry" {
                        if let Operation::Path(p) = &node.operation {
                            p.geometry.keys[*index].time
                        } else {
                            continue;
                        }
                    } else if let Some(scalar) = node.operation.scalar(&parameter) {
                        scalar.keys().unwrap()[*index].time
                    } else {
                        let found = node
                            .operation
                            .keyed_properties()
                            .into_iter()
                            .find(|(path, _)| *path == parameter);
                        match found {
                            Some((_, times)) => times[*index],
                            None => continue,
                        }
                    };
                    if frame(original.checked_add(time)?, doc.frame_rate, doc.frame_count).is_none()
                    {
                        return Err(veylvik_document::Error(
                            "Moved keys must remain inside the composition.".into(),
                        ));
                    }
                }
            }
            if parameter == "geometry"
                && let Operation::Path(p) = &mut node.operation
            {
                if delete {
                    let shape = p.geometry.at(now)?;
                    p.geometry.keys = std::mem::take(&mut p.geometry.keys)
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, k)| (!indices.contains(&i)).then_some(k))
                        .collect();
                    if p.geometry.keys.is_empty() {
                        p.geometry = shape;
                    }
                } else {
                    let original = p.geometry.keys.clone();
                    if !duplicate {
                        p.geometry.keys = original
                            .iter()
                            .enumerate()
                            .filter_map(|(i, k)| (!indices.contains(&i)).then_some(k.clone()))
                            .collect();
                    }
                    for index in indices {
                        let mut key = original[index].clone();
                        key.time = key.time.checked_add(time)?;
                        p.geometry.keys.push(key);
                    }
                    p.geometry
                        .keys
                        .sort_by(|a, b| a.time.compare(b.time).unwrap());
                }
                p.geometry.validate()?;
            } else if node.operation.scalar(&parameter).is_none()
                && let Some(property) = node.operation.property_mut(&parameter)
            {
                match property {
                    veylvik_document::track::Property::Vector(track) => {
                        *track = if delete {
                            track.without(&indices, now)?
                        } else {
                            track.shifted(&indices, time, duplicate)?
                        };
                    }
                    veylvik_document::track::Property::Number(scalar) => {
                        *scalar = if delete {
                            key_edit::deleted(scalar, &indices, now)?
                        } else if duplicate {
                            let first = scalar.keys().unwrap()[*indices.first().unwrap()].time;
                            key_edit::pasted(
                                scalar,
                                &key_edit::copied(scalar, &indices)?,
                                first.checked_add(time)?,
                            )?
                        } else {
                            key_edit::moved(scalar, &indices, time, 0.)?
                        };
                    }
                }
            } else if let Some(scalar) = node.operation.scalar_mut(&parameter) {
                *scalar = if delete {
                    key_edit::deleted(scalar, &indices, now)?
                } else if duplicate {
                    let first = scalar.keys().unwrap()[*indices.first().unwrap()].time;
                    key_edit::pasted(
                        scalar,
                        &key_edit::copied(scalar, &indices)?,
                        first.checked_add(time)?,
                    )?
                } else {
                    key_edit::moved(scalar, &indices, time, 0.)?
                };
            }
            node.operation.validate()?;
        }
        Ok(nodes
            .into_values()
            .map(|node| Command::Put {
                node: Box::new(node),
            })
            .collect())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        hit: Option<i64>,
        rect: egui::Rect,
        end: f32,
        doc: &Composition,
        current: i64,
        editable: bool,
        actions: &mut Vec<Action>,
    ) {
        if response.clicked()
            && let Some(frame) = hit
        {
            response.request_focus();
            if ui.input(|i| i.modifiers.shift) {
                if !self.selected.insert(frame) {
                    self.selected.remove(&frame);
                }
            } else {
                self.selected = BTreeSet::from([frame]);
            }
            actions.push(Action::Seek(frame));
        }
        if response.drag_started() && editable {
            let start = ui.input(|i| i.pointer.press_origin());
            let start_frame = start.and_then(|p| {
                self.frames.iter().copied().find(|f| {
                    let x = rect.left() + *f as f32 / end * rect.width();
                    (x - p.x).abs() <= 7. && (p.y - (rect.bottom() - 10.)).abs() <= 12.
                })
            });
            if let Some(frame) = start_frame {
                response.request_focus();
                if !self.selected.contains(&frame) {
                    if !ui.input(|i| i.modifiers.shift) {
                        self.selected.clear();
                    }
                    self.selected.insert(frame);
                }
                self.drag = Some(Drag {
                    origin: start.unwrap().x,
                    offset: 0,
                    duplicate: ui.input(|i| i.modifiers.alt),
                });
                self.error = None;
            }
        }
        if self.drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.drag = None;
            self.preview = None;
            self.error = None;
            actions.push(Action::Preview);
            return;
        }
        if response.dragged()
            && let Some(drag) = &self.drag
        {
            let offset = ((response.interact_pointer_pos().map_or(drag.origin, |p| p.x)
                - drag.origin)
                / rect.width()
                * end)
                .round() as i64;
            let offset = offset.clamp(
                -self.selected.first().copied().unwrap_or(0),
                i64::from(doc.frame_count.saturating_sub(1))
                    - self.selected.last().copied().unwrap_or(0),
            );
            if offset != drag.offset {
                let duplicate = drag.duplicate;
                self.drag.as_mut().unwrap().offset = offset;
                match self.commands(doc, offset, duplicate, false, current) {
                    Ok(commands) => {
                        self.preview = Some(commands);
                        self.error = None;
                    }
                    Err(e) => {
                        self.preview = None;
                        self.error = Some(e.to_string());
                    }
                }
                actions.push(Action::Preview);
            }
        }
        if response.drag_stopped()
            && let Some(drag) = self.drag.take()
        {
            if drag.offset != 0
                && let Some(commands) = self.preview.take()
            {
                self.selected = self.selected.iter().map(|f| f + drag.offset).collect();
                actions.push(Action::Commit(commands));
            } else {
                self.preview = None;
                actions.push(Action::Preview);
                if let Some(error) = self.error.take() {
                    actions.push(Action::Error(error));
                }
            }
        }
        let mut delete = false;
        let mut duplicate = false;
        response.context_menu(|ui| {
            if let Some(frame) = hit
                && !self.selected.contains(&frame)
            {
                self.selected = BTreeSet::from([frame]);
            }
            ui.label(format!("{} selected keyed frames", self.selected.len()));
            if ui
                .add_enabled(
                    editable && !self.selected.is_empty(),
                    egui::Button::new("Duplicate at playhead"),
                )
                .clicked()
            {
                duplicate = true;
                ui.close();
            }
            if ui
                .add_enabled(
                    editable && !self.selected.is_empty(),
                    egui::Button::new("Delete selected keys"),
                )
                .clicked()
            {
                delete = true;
                ui.close();
            }
            if let Some(frame) = hit
                && let Some(marks) = self.marks.get(&frame)
            {
                ui.separator();
                ui.label("Open in Animation");
                for mark in marks {
                    if ui.button(&mark.hint).clicked() {
                        actions.push(Action::Open(mark.node));
                        ui.close();
                    }
                }
            }
        });
        if response.has_focus()
            && editable
            && !self.dragging()
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
        {
            delete = true;
        }
        if (delete || duplicate) && !self.selected.is_empty() {
            let offset = if duplicate {
                current - *self.selected.first().unwrap()
            } else {
                0
            };
            match self.commands(doc, offset, duplicate, delete, current) {
                Ok(commands) => {
                    actions.push(Action::Commit(commands));
                    self.selected.clear();
                }
                Err(e) => actions.push(Action::Error(e.to_string())),
            }
        }
        if let Some(error) = &self.error {
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                response.layer_id,
                response.id.with("error"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Composition, NodeId) {
        let mut doc = veylvik_presets::showcase::build("gold_text")
            .unwrap()
            .composition;
        let node = doc
            .nodes
            .values_mut()
            .find(|n| matches!(n.operation, Operation::Text(_)))
            .unwrap();
        for parameter in ["x", "y"] {
            let scalar = node.operation.scalar_mut(parameter).unwrap();
            scalar.set_key_at(Time::new(0, 1).unwrap(), 100.).unwrap();
            scalar.set_key_at(Time::new(1, 1).unwrap(), 200.).unwrap();
        }
        let id = node.id;
        (doc, id)
    }
    #[test]
    fn grouped_moves_are_atomic_preserve_values_and_reject_collisions() {
        let (doc, id) = setup();
        let mut ruler = Keys {
            scope: Scope::Selected,
            ..Default::default()
        };
        ruler.sync_live(&doc, 0, id, &[]);
        ruler.selected.insert(0);
        assert_eq!(ruler.marks[&0].len(), 2);
        assert!(ruler.commands(&doc, 24, false, false, 0).is_err());
        let commands = ruler.commands(&doc, 6, false, false, 0).unwrap();
        let document = veylvik_document::Document::new(doc).unwrap();
        let moved = document.prepare(0, &commands).unwrap();
        for parameter in ["x", "y"] {
            let keys = moved.nodes[&id]
                .operation
                .scalar(parameter)
                .unwrap()
                .keys()
                .unwrap();
            assert_eq!(
                frame(keys[0].time, moved.frame_rate, moved.frame_count),
                Some(6)
            );
            assert_eq!(keys[0].value, 100.);
            assert_eq!(
                frame(keys[1].time, moved.frame_rate, moved.frame_count),
                Some(24)
            );
        }
    }
    #[test]
    fn layer_includes_material_and_drafts_refresh_without_revision_change() {
        let (doc, id) = setup();
        let mut ruler = Keys::default();
        ruler.sync_live(&doc, 0, id, &[]);
        assert!(ruler.marks.values().flatten().any(|m| m.node != id));
        let mut draft = doc.nodes[&id].clone();
        draft
            .operation
            .scalar_mut("x")
            .unwrap()
            .set_key_at(Time::new(1, 2).unwrap(), 150.)
            .unwrap();
        ruler.sync_live(&doc, 0, id, &[draft]);
        assert!(
            ruler.marks[&12]
                .iter()
                .any(|m| m.node == id && m.parameter == "x")
        );
        ruler.sync_live(&doc, 0, id, &[]);
        assert!(
            !ruler
                .marks
                .get(&12)
                .is_some_and(|marks| marks.iter().any(|m| m.node == id))
        );
        let background = doc.nodes.values().find(|n| n.scene_background).unwrap().id;
        assert!(!layer_nodes(&doc, id, Scope::Layer).contains(&background));
    }
    #[test]
    fn duplicate_and_delete_leave_other_frames_intact() {
        let (doc, id) = setup();
        let mut ruler = Keys {
            scope: Scope::Selected,
            ..Default::default()
        };
        ruler.sync_live(&doc, 0, id, &[]);
        ruler.selected.insert(0);
        let document = veylvik_document::Document::new(doc.clone()).unwrap();
        let duplicated = document
            .prepare(0, &ruler.commands(&doc, 12, true, false, 0).unwrap())
            .unwrap();
        assert_eq!(
            duplicated.nodes[&id]
                .operation
                .scalar("x")
                .unwrap()
                .keys()
                .unwrap()
                .len(),
            3
        );
        let deleted = document
            .prepare(0, &ruler.commands(&doc, 0, false, true, 0).unwrap())
            .unwrap();
        assert_eq!(
            deleted.nodes[&id]
                .operation
                .scalar("x")
                .unwrap()
                .keys()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn shape_markers_retime_without_losing_geometry() {
        use veylvik_document::vector::{FillRule, PathCommand, PathMask};
        let mut doc = veylvik_document::demo();
        let mut geometry = PathMask {
            keys: vec![],
            fill_rule: FillRule::NonZero,
            commands: vec![
                PathCommand::Move { to: [0., 0.] },
                PathCommand::Line { to: [10., 0.] },
                PathCommand::Line { to: [0., 10.] },
                PathCommand::Close {},
            ],
        };
        let shape = geometry.clone();
        geometry.set_key(Time::new(0, 1).unwrap(), &shape).unwrap();
        geometry.set_key(Time::new(1, 1).unwrap(), &shape).unwrap();
        let node = Node::new(
            "Mask",
            Operation::Path(veylvik_document::nodes::Path { geometry }),
            &[],
        );
        let id = node.id;
        doc.nodes.insert(id, node);
        let mut ruler = Keys {
            scope: Scope::Selected,
            ..Default::default()
        };
        ruler.sync_live(&doc, 0, id, &[]);
        ruler.selected.insert(0);
        assert!(ruler.commands(&doc, 24, false, false, 0).is_err());
        let document = veylvik_document::Document::new(doc.clone()).unwrap();
        let result = document
            .prepare(0, &ruler.commands(&doc, 6, false, false, 0).unwrap())
            .unwrap();
        let Operation::Path(path) = &result.nodes[&id].operation else {
            panic!("path expected")
        };
        assert_eq!(path.geometry.keys.len(), 2);
        assert_eq!(
            frame(path.geometry.keys[0].time, doc.frame_rate, doc.frame_count),
            Some(6)
        );
        assert_eq!(
            path.geometry.at(Time::new(1, 4).unwrap()).unwrap().commands,
            shape.commands
        );
    }
}
