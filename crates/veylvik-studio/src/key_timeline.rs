//! Cached navigation over the applied scalar keys of one node.
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{Command, Composition, Node, NodeId, Operation, Time};
mod editing;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    Selected,
    #[default]
    Layer,
    All,
}
#[derive(Clone)]
struct Mark {
    node: NodeId,
    parameter: String,
    index: usize,
    hint: String,
}
pub enum Action {
    Seek(i64),
    Commit(Vec<Command>),
    Preview,
    Open(NodeId),
    Error(String),
}
#[derive(Default)]
pub struct Keys {
    pub playback_range: Option<(i64, i64)>,
    pub cached_gpu: Vec<i64>,
    pub cached_disk: Vec<i64>,
    source: Option<(u64, NodeId, Scope, Vec<u8>)>,
    pub scope: Scope,
    frames: Vec<i64>,
    marks: BTreeMap<i64, Vec<Mark>>,
    selected: BTreeSet<i64>,
    drag: Option<editing::Drag>,
    preview: Option<Vec<Command>>,
    error: Option<String>,
    focus: Option<egui::Id>,
}

fn layer_nodes(doc: &Composition, selected: NodeId, scope: Scope) -> BTreeSet<NodeId> {
    if scope == Scope::All {
        return doc.nodes.keys().copied().collect();
    }
    let mut ids = BTreeSet::from([selected]);
    if scope == Scope::Selected {
        return ids;
    }
    let mut pending = vec![selected];
    while let Some(id) = pending.pop() {
        let mut candidates = Vec::new();
        for g in doc.groups.values().filter(|g| g.members.contains(&id)) {
            candidates.extend(g.members.iter().copied());
        }
        if let Some(n) = doc.nodes.get(&id) {
            if !matches!(n.operation, Operation::Merge(_)) {
                candidates.extend(n.inputs.values().copied().filter(|id| {
                    doc.nodes
                        .get(id)
                        .is_some_and(|n| !matches!(n.operation, Operation::Merge(_)))
                }));
            }
            candidates.extend(
                doc.nodes
                    .values()
                    .filter(|n| {
                        !matches!(n.operation, Operation::Merge(_))
                            && n.inputs.values().any(|source| *source == id)
                    })
                    .map(|n| n.id),
            );
        }
        for next in candidates {
            if ids.insert(next) {
                pending.push(next);
            }
        }
    }
    ids
}

// Round to the nearest output frame, with exact integer arithmetic. Keys outside
// the composition remain authored but are excluded from this navigation strip.
pub(super) fn frame(time: Time, fps: Time, count: u32) -> Option<i64> {
    if time.rate == 0 || fps.rate == 0 || fps.ticks <= 0 || time.ticks < 0 || count == 0 {
        return None;
    }
    let numerator = i128::from(time.ticks) * i128::from(fps.ticks);
    let denominator = i128::from(time.rate) * i128::from(fps.rate);
    if numerator > i128::from(count - 1) * denominator {
        return None;
    }
    i64::try_from((numerator + denominator / 2) / denominator).ok()
}

// Choose whole-frame intervals with enough room for readable labels at any duration.
fn tick_step(count: u32, width: f32) -> u32 {
    let target = (count.saturating_sub(1) as f64 / (width as f64 / 64.).max(1.)).max(1.);
    let power = 10_f64.powf(target.log10().floor());
    [1., 2., 5., 10.]
        .into_iter()
        .map(|m| (m * power) as u32)
        .find(|s| f64::from(*s) >= target)
        .unwrap_or(u32::MAX)
        .max(1)
}

impl Keys {
    #[cfg(test)]
    pub fn sync(&mut self, doc: &Composition, revision: u64, selected: NodeId) {
        self.scope = Scope::Selected;
        self.sync_live(doc, revision, selected, &[]);
    }
    pub fn sync_live(
        &mut self,
        doc: &Composition,
        revision: u64,
        selected: NodeId,
        drafts: &[Node],
    ) {
        let signature = serde_json::to_vec(drafts).unwrap_or_default();
        let source = (revision, selected, self.scope, signature);
        if self.source.as_ref() == Some(&source) {
            return;
        }
        let scope_changed = self
            .source
            .as_ref()
            .is_some_and(|s| s.1 != selected || s.2 != self.scope);
        self.source = Some(source);
        self.drag = None;
        self.preview = None;
        if scope_changed {
            self.selected.clear();
        }
        self.marks.clear();
        for id in layer_nodes(doc, selected, self.scope) {
            let Some(node) = drafts
                .iter()
                .find(|n| n.id == id)
                .or_else(|| doc.nodes.get(&id))
            else {
                continue;
            };
            if let Operation::Path(veylvik_document::nodes::Path { geometry }) = &node.operation {
                for (index, key) in geometry.keys.iter().enumerate() {
                    if let Some(f) = frame(key.time, doc.frame_rate, doc.frame_count) {
                        self.marks.entry(f).or_default().push(Mark {
                            node: id,
                            parameter: "geometry".into(),
                            index,
                            hint: format!("{} · Shape", node.label),
                        });
                    }
                }
            }
            for parameter in node.operation.descriptor().parameters {
                if let Some(keys) = node.operation.scalar(parameter.name).and_then(|s| s.keys()) {
                    for (index, key) in keys.iter().enumerate() {
                        if let Some(f) = frame(key.time, doc.frame_rate, doc.frame_count) {
                            self.marks.entry(f).or_default().push(Mark {
                                node: id,
                                parameter: parameter.name.into(),
                                index,
                                hint: format!(
                                    "{} · {} = {:.3}",
                                    node.label, parameter.name, key.value
                                ),
                            });
                        }
                    }
                }
            }
        }
        self.frames = self.marks.keys().copied().collect();
        self.selected.retain(|f| self.marks.contains_key(f));
    }
    pub fn scope_controls(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("ruler-scope")
            .width(90.)
            .selected_text(match self.scope {
                Scope::Selected => "Selected",
                Scope::Layer => "Layer",
                Scope::All => "All",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.scope, Scope::Selected, "Selected node");
                ui.selectable_value(&mut self.scope, Scope::Layer, "Layer + effects");
                ui.selectable_value(&mut self.scope, Scope::All, "All nodes");
            })
            .response
            .on_hover_text(
                "Keyframe scope: one node, its connected layer and effects, or all nodes",
            );
    }
    pub fn preview_commands(&self) -> Option<&[Command]> {
        self.preview.as_deref()
    }
    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.focus.is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn adjacent(&self, current: i64, forward: bool) -> Option<i64> {
        if forward {
            self.frames
                .get(self.frames.partition_point(|f| *f <= current))
                .copied()
        } else {
            self.frames
                .partition_point(|f| *f < current)
                .checked_sub(1)
                .map(|index| self.frames[index])
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: i64,
        doc: &Composition,
        editable: bool,
    ) -> Vec<Action> {
        let count = doc.frame_count;
        let mut actions = Vec::new();
        let mut seek = None;
        ui.weak(format!(
            "{} keys",
            self.marks.values().map(Vec::len).sum::<usize>()
        )).on_hover_text(if editable {"Keys in the chosen scope; stacked diamonds move all keys at that frame together."} else {"Includes inspector edits. Finish or apply edits and pause playback before dragging keys."});
        for (forward, label, hint) in [
            (false, "Prev key", "Alt+Left"),
            (true, "Next key", "Alt+Right"),
        ] {
            let target = self.adjacent(current, forward);
            if ui
                .add_enabled(
                    target.is_some(),
                    crate::icons::Button::new(
                        if forward {
                            crate::icons::Icon::Right
                        } else {
                            crate::icons::Icon::Left
                        },
                        label,
                    ),
                )
                .on_hover_text(hint)
                .clicked()
            {
                seek = target;
            }
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(40.0), 44.0),
            egui::Sense::click_and_drag(),
        );
        self.focus = Some(response.id);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                ui.is_enabled(),
                "Frame ruler and keyframes",
            )
        });
        let rect = rect.shrink2(egui::vec2(4.0, 2.0));
        let end = count.saturating_sub(1).max(1) as f32;
        let x = |frame: i64| rect.left() + frame as f32 / end * rect.width();
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, crate::theme::palette(ui.ctx()).canvas);
        if let Some((first, last)) = self.playback_range {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x(first), rect.top()),
                    egui::pos2(x(last).max(x(first) + 2.), rect.bottom()),
                ),
                0.,
                crate::design::accent(ui.ctx()).gamma_multiply(0.16),
            );
            for at in [x(first), x(last)] {
                painter.vline(at, rect.y_range(), (2_f32, crate::design::accent(ui.ctx())));
            }
        }
        for (frames, colour, y) in [
            (
                &self.cached_disk,
                egui::Color32::from_rgb(85, 150, 230),
                rect.bottom() - 1.,
            ),
            (
                &self.cached_gpu,
                egui::Color32::from_rgb(55, 190, 140),
                rect.bottom() - 4.,
            ),
        ] {
            for f in frames {
                painter.hline(
                    x(*f)..=x((*f + 1).min(i64::from(count - 1))).max(x(*f) + 1.),
                    y,
                    (2_f32, colour),
                );
            }
        }
        let step = tick_step(count, rect.width());
        for frame in (0..count).step_by(step as usize) {
            let at = x(i64::from(frame));
            painter.vline(
                at,
                rect.top() + 15.0..=rect.bottom(),
                (1., crate::theme::palette(ui.ctx()).border),
            );
            painter.text(
                egui::pos2(at + 2., rect.top()),
                egui::Align2::LEFT_TOP,
                frame.to_string(),
                egui::FontId::proportional(11.),
                ui.visuals().weak_text_color(),
            );
            if step >= 5 {
                for minor in 1..5 {
                    let f = u64::from(frame) + u64::from(step) * minor / 5;
                    if f < u64::from(count) {
                        painter.vline(
                            x(f as i64),
                            rect.bottom() - 4. ..=rect.bottom(),
                            (1., crate::theme::palette(ui.ctx()).border),
                        );
                    }
                }
            }
        }
        painter.hline(
            rect.x_range(),
            rect.bottom() - 10.,
            (1.0, crate::theme::palette(ui.ctx()).border),
        );
        if (0..i64::from(count)).contains(&current) {
            painter.vline(x(current), rect.y_range(), (1.0, ui.visuals().text_color()));
        }
        // Bound marker density without losing the current-key highlight.
        let mut previous = None;
        for frame in &self.frames {
            let shown_frame = if self.selected.contains(frame) {
                frame + self.drag.as_ref().map_or(0, |d| d.offset)
            } else {
                *frame
            };
            let at = x(shown_frame).round();
            if previous.is_none_or(|last: f32| at - last >= 6.0) || *frame == current {
                let c = egui::pos2(at, rect.bottom() - 10.);
                let colour = if self.selected.contains(frame) {
                    crate::design::accent(ui.ctx())
                } else if *frame == current {
                    egui::Color32::from_rgb(239, 99, 109)
                } else {
                    egui::Color32::from_rgb(210, 165, 90)
                };
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        c + egui::vec2(0.0, -4.5),
                        c + egui::vec2(4.5, 0.0),
                        c + egui::vec2(0.0, 4.5),
                        c + egui::vec2(-4.5, 0.0),
                    ],
                    colour,
                    egui::Stroke::NONE,
                ));
                if self.marks[frame].len() > 1 {
                    painter.text(
                        c + egui::vec2(7., -9.),
                        egui::Align2::LEFT_BOTTOM,
                        self.marks[frame].len().to_string(),
                        egui::FontId::proportional(10.),
                        colour,
                    );
                }
                previous = Some(at);
            }
        }
        let nearest = response.hover_pos().and_then(|pointer| {
            let target = ((pointer.x - rect.left()) / rect.width() * end).round() as i64;
            let index = self.frames.partition_point(|f| *f < target);
            [index.checked_sub(1), Some(index)]
                .into_iter()
                .flatten()
                .filter_map(|i| self.frames.get(i).copied())
                .min_by_key(|f| f.abs_diff(target))
        });
        let hit = nearest.filter(|f| {
            response.hover_pos().is_some_and(|p| {
                (x(*f) - p.x).abs() <= 7. && (p.y - (rect.bottom() - 10.)).abs() <= 12.
            })
        });
        self.interact(
            ui,
            &response,
            hit,
            rect,
            end,
            doc,
            current,
            editable,
            &mut actions,
        );
        if (response.clicked() || response.dragged())
            && hit.is_none()
            && !self.dragging()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            seek = Some(
                ((pointer.x - rect.left()) / rect.width() * end)
                    .round()
                    .clamp(0., count.saturating_sub(1) as f32) as i64,
            );
        }
        let hint = if let Some(f) = hit {
            let lines = self.marks[&f]
                .iter()
                .map(|m| m.hint.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Frame {f} · {:.3} s · {} keys\n{lines}\nClick to select and seek; drag to retime together. Shift: multiple; Alt-drag: copy. Right-click for actions.",
                f as f64 * doc.frame_rate.rate as f64 / doc.frame_rate.ticks as f64,
                self.marks[&f].len()
            )
        } else {
            "Frames · click or drag empty space to scrub. Diamonds show keys for the chosen scope."
                .into()
        };
        response.on_hover_text(hint);
        if let Some(frame) = seek {
            actions.push(Action::Seek(frame));
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Key, Node, Operation, Scalar};
    #[test]
    fn ruler_intervals_are_readable_whole_frames() {
        assert_eq!(tick_step(96, 800.), 10);
        assert_eq!(tick_step(96, 300.), 50);
        assert_eq!(tick_step(1, 800.), 1);
        assert_eq!(tick_step(10, 800.), 1);
        assert!(tick_step(u32::MAX, 40.) > 0);
    }
    #[test]
    fn rational_frames_and_bounds() {
        let fps = Time {
            ticks: 30000,
            rate: 1001,
        };
        assert_eq!(
            frame(
                Time {
                    ticks: 1001,
                    rate: 10000
                },
                fps,
                10
            ),
            Some(3)
        );
        assert_eq!(
            frame(
                Time {
                    ticks: 1001,
                    rate: 60000
                },
                fps,
                10
            ),
            Some(1)
        );
        assert_eq!(
            frame(
                Time {
                    ticks: -1,
                    rate: 24
                },
                fps,
                10
            ),
            None
        );
        assert_eq!(frame(Time { ticks: 1, rate: 1 }, fps, 10), None);
        assert_eq!(frame(Time { ticks: 0, rate: 1 }, fps, 1), Some(0));
        assert_eq!(
            frame(
                Time {
                    ticks: i64::MAX,
                    rate: 1
                },
                Time {
                    ticks: i64::MAX,
                    rate: 1
                },
                100
            ),
            None
        );
        assert_eq!(frame(Time { ticks: 1, rate: 0 }, fps, 10), None);
    }
    #[test]
    fn shape_keys_share_the_timeline_and_navigation() {
        let mut doc = veylvik_document::demo();
        let mut geometry = veylvik_document::vector::PathMask::default();
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
        let mut keys = Keys::default();
        keys.sync(&doc, 0, id);
        assert_eq!(keys.frames, [0, 24]);
        assert_eq!(keys.adjacent(12, false), Some(0));
        assert_eq!(keys.adjacent(12, true), Some(24));
    }
    #[test]
    fn shared_parameter_frames_and_revision_refresh() {
        let mut doc = veylvik_document::demo();
        let curve = Scalar::Linear {
            keys: [0, 12, 24]
                .map(|ticks| Key {
                    segment: None,
                    time: Time { ticks, rate: 24 },
                    value: ticks as f32,
                })
                .into(),
        };
        let node = Node::new(
            "Animated",
            Operation::Transform(veylvik_document::nodes::Transform {
                x: curve.clone(),
                y: curve,
            }),
            &[("image", doc.output)],
        );
        let id = node.id;
        doc.nodes.insert(id, node);
        let mut keys = Keys::default();
        keys.sync(&doc, 0, id);
        assert_eq!(keys.frames, [0, 12, 24]);
        assert_eq!(keys.adjacent(12, true), Some(24));
        assert_eq!(keys.adjacent(12, false), Some(0));
        assert_eq!(keys.adjacent(0, false), None);
        assert_eq!(keys.adjacent(24, true), None);
        doc.frame_count = 13;
        keys.sync(&doc, 1, id);
        assert_eq!(keys.frames, [0, 12]);
        keys.sync(&doc, 1, doc.output);
        assert!(keys.frames.is_empty());
    }
}
