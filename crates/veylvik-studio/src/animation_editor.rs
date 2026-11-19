//! Applied animation workspace. Gestures keep a local preview and submit one
//! revision-checked SetParameter transaction on release, never per pointer event.
mod canvas;
mod groups;
use groups::{ClipboardTrack, Edits, Marquee, Selection};
mod tangents;

use eframe::egui::{self, Pos2, Rect, Vec2};
use std::collections::BTreeSet;
use veylvik_document::{Composition, Interpolation, Key, NodeId, Scalar, Time, key_edit};

pub enum Action {
    Batch {
        revision: u64,
        edits: Edits,
    },
    Select(NodeId),
    Seek(i64),
    Commit {
        revision: u64,
        node: NodeId,
        parameter: String,
        scalar: Scalar,
    },
}
struct CanvasContext<'a> {
    doc: &'a Composition,
    revision: u64,
    current: i64,
    now: Time,
    focus: egui::Id,
    range: [f64; 2],
}
struct Track<'a> {
    node: &'a veylvik_document::Node,
    parameter: &'a str,
    height: f32,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Clipboard {
    veylvik_keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tracks: Vec<ClipboardTrack>,
}
struct Drag {
    original: Scalar,
    handle: Option<(usize, bool)>,
    companions: Selection,
    group_preview: Option<Edits>,
    selection: BTreeSet<usize>,
    origin: Pos2,
    revision: u64,
    preview: Option<Scalar>,
    moved: bool,
    min: f64,
    max: f64,
}
#[derive(Default)]
pub struct Editor {
    pub visible: bool,
    focus: Option<egui::Id>,
    curve: bool,
    active: Option<(NodeId, String)>,
    selection: BTreeSet<usize>,
    linked: Selection,
    marquee: Option<Marquee>,
    hit_keys: Vec<(NodeId, String, usize, Pos2)>,
    clipboard_tracks: Vec<ClipboardTrack>,
    pending_selection: Option<BTreeSet<usize>>,
    pending_group: Option<Selection>,
    revision: Option<u64>,
    selected_node: Option<NodeId>,
    nodes: Vec<NodeId>,
    drag: Option<Drag>,
    clipboard: Vec<Key>,
    range: Option<[f64; 2]>,
    error: Option<String>,
}
impl Editor {
    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.visible && self.focus.is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.marquee.is_some()
    }
    fn commit(
        &mut self,
        result: veylvik_document::Result<Scalar>,
        revision: u64,
        actions: &mut Vec<Action>,
    ) {
        match result {
            Ok(scalar) => {
                if let Some((node, parameter)) = &self.active {
                    actions.push(Action::Commit {
                        revision,
                        node: *node,
                        parameter: parameter.clone(),
                        scalar,
                    });
                }
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        doc: &Composition,
        revision: u64,
        selected: NodeId,
        current: i64,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        self.hit_keys.clear();
        let selection_changed = self.selected_node != Some(selected);
        if self.revision != Some(revision) || selection_changed {
            self.nodes = doc
                .nodes
                .values()
                .filter(|n| {
                    n.id == selected
                        || n.operation.descriptor().parameters.iter().any(|p| {
                            n.operation
                                .scalar(p.name)
                                .is_some_and(|s| s.keys().is_some())
                        })
                })
                .map(|n| n.id)
                .collect();
            self.nodes
                .sort_by(|a, b| doc.nodes[a].label.cmp(&doc.nodes[b].label).then(a.cmp(b)));
        }
        if self.revision != Some(revision) {
            self.selection = self.pending_selection.take().unwrap_or_default();
            self.drag = None;
            self.linked.clear();
            if let Some(selected) = self.pending_group.take() {
                self.install_selection(selected);
            }
            self.marquee = None;
            self.revision = Some(revision);
        }
        if selection_changed
            || self.active.as_ref().is_none_or(|(id, parameter)| {
                doc.nodes
                    .get(id)
                    .and_then(|n| n.operation.scalar(parameter))
                    .is_none()
            })
        {
            self.active = doc.nodes.get(&selected).and_then(|node| {
                let parameters = node.operation.descriptor().parameters;
                parameters
                    .iter()
                    .find(|p| {
                        node.operation
                            .scalar(p.name)
                            .is_some_and(|s| s.keys().is_some())
                    })
                    .or_else(|| {
                        parameters
                            .iter()
                            .find(|p| node.operation.scalar(p.name).is_some())
                    })
                    .map(|p| (selected, p.name.into()))
            });
            self.selection.clear();
            self.linked.clear();
            self.marquee = None;
        }
        self.selected_node = Some(selected);
        let active = self
            .active
            .as_ref()
            .and_then(|(id, p)| doc.nodes.get(id)?.operation.scalar(p))
            .cloned();
        self.selection.retain(|index| {
            active
                .as_ref()
                .and_then(|s| s.keys())
                .is_some_and(|keys| *index < keys.len())
        });
        let now = Time::at_frame(current, doc.frame_rate).expect("validated frame rate");
        let focus = ui.id().with("animation_keyboard");
        self.focus = Some(focus);
        ui.interact(
            ui.max_rect(),
            focus,
            egui::Sense::focusable_noninteractive(),
        );
        let focused = ui.is_enabled() && ui.memory(|m| m.has_focus(focus));
        let (copy_event, paste_event) = if focused {
            ui.input_mut(|input| {
                let mut copy = false;
                let mut paste = None;
                input.events.retain(|event| match event {
                    egui::Event::Copy => {
                        copy = true;
                        false
                    }
                    egui::Event::Paste(text) => {
                        paste = Some(text.clone());
                        false
                    }
                    _ => true,
                });
                (copy, paste)
            })
        } else {
            (false, None)
        };
        let paste_requested = paste_event.is_some();
        let mut valid_paste = true;
        if let Some(text) = paste_event {
            match (text.len() <= 2 * 1024 * 1024)
                .then(|| serde_json::from_str::<Clipboard>(&text).ok())
                .flatten()
            {
                Some(clip)
                    if (clip.veylvik_keys.is_empty() != clip.tracks.is_empty())
                        && clip.tracks.len() <= 64
                        && clip.veylvik_keys.len()
                            + clip.tracks.iter().map(|t| t.keys.len()).sum::<usize>()
                            <= veylvik_document::animation::MAX_KEYS =>
                {
                    self.clipboard = clip.veylvik_keys;
                    self.clipboard_tracks = clip.tracks;
                }
                _ => {
                    valid_paste = false;
                    self.error = Some("Clipboard contains no Veylvik keyframes.".into());
                }
            }
        }
        let escape = ui.is_enabled()
            && self.dragging()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            self.drag = None;
            if let Some(marquee) = self.marquee.take() {
                self.install_selection(marquee.base);
            }
            self.error = None;
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.curve, false, "Keyframes");
            ui.selectable_value(&mut self.curve, true, "Curves");
            ui.separator();
            if ui.button("Fit composition").clicked() {
                self.range = None;
            }
            if ui.button("Fit keys").clicked()
                && let Some(keys) = active.as_ref().and_then(|s| s.keys())
            {
                let first = frame_number(keys[0].time, doc.frame_rate);
                let last = frame_number(keys[keys.len() - 1].time, doc.frame_rate);
                let pad = ((last - first) * 0.05).max(1.0);
                if first.abs().max(last.abs()) <= 1.0e12 {
                    self.range = Some([first - pad, last + pad]);
                } else {
                    self.error = Some("These timestamps exceed the visual editor range; use the exact inspector fields.".into());
                }
            }
            let ready = active.is_some() && !self.dragging();
            let keys = ready && !self.selections().is_empty();
            let copy = ui
                .add_enabled(keys, egui::Button::new("Copy keys"))
                .clicked()
                || (focused
                    && keys
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::C)));
            if copy || (keys && copy_event) {
                ui.memory_mut(|m|m.request_focus(focus));
                match self.copy_selection(doc) {
                    Ok(()) => {
                        ui.ctx().copy_text(serde_json::to_string(&Clipboard{veylvik_keys:self.clipboard.clone(),tracks:self.clipboard_tracks.clone()}).expect("validated keys"));self.error=None;
                    }
                    Err(e)=>self.error=Some(e.to_string()),
                }
            }

            let paste = ui
                .add_enabled(
                    ready && (!self.clipboard.is_empty() || !self.clipboard_tracks.is_empty()),
                    egui::Button::new("Paste at playhead"),
                )
                .clicked()
                || (focused
                    && ready
                    && (!self.clipboard.is_empty() || !self.clipboard_tracks.is_empty())
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::V)));
            if paste || (ready && paste_requested && valid_paste) {
                if self.clipboard_tracks.is_empty() {
                    self.commit(key_edit::pasted(active.as_ref().unwrap(),&self.clipboard,now),revision,&mut actions);
                } else { self.batch(self.paste_group(doc,now),doc,revision,&mut actions); }
            }

            let delete = ui
                .add_enabled(keys, egui::Button::new("Delete keys"))
                .clicked()
                || (focused
                    && keys
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)));
            if delete {
                let edits=self.selections().iter().map(|((id,p),indices)| Ok((*id,p.clone(),key_edit::deleted(doc.nodes[id].operation.scalar(p).unwrap(),indices,now)?))).collect();
                self.batch(edits,doc,revision,&mut actions);
            }

            if ui
                .add_enabled(ready, egui::Button::new("Add key"))
                .clicked()
            {
                let mut scalar = active.clone().unwrap();
                let result = scalar
                    .at(now)
                    .and_then(|value| scalar.set_key_at(now, value))
                    .map(|_| scalar);
                self.commit(result, revision, &mut actions);
            }
            if let Some(scalar) = active.as_ref()
                && let Some(mut mode) = scalar.interpolation()
            {
                let before = mode;
                ui.add_enabled_ui(!self.dragging(), |ui| {
                    egui::ComboBox::from_id_salt("curve_mode")
                        .selected_text(crate::animation::mode_label(mode))
                        .show_ui(ui, |ui| {
                            for choice in [
                                Interpolation::Linear,
                                Interpolation::Step,
                                Interpolation::Smooth,
                                Interpolation::Monotone,
                            ] {
                                ui.selectable_value(
                                    &mut mode,
                                    choice,
                                    crate::animation::mode_label(choice),
                                );
                            }
                        }).response.on_hover_text("Set interpolation for the entire active parameter; replaces per-segment easing.");
                });
                if mode != before {
                    let mut edited = scalar.clone();
                    edited.set_interpolation(mode);
                    self.commit(Ok(edited), revision, &mut actions);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.set_min_height(20.0);
            ui.add_enabled_ui(!self.dragging(), |ui| {
                if active.is_some() {
                    self.segment_controls(ui, doc, revision, &mut actions);
                }
            });
        });
        let hint = self.error.as_deref().unwrap_or("Drag empty space to box-select · Shift adds keys/tracks · drag keys or yellow handles · Escape cancels · scroll zooms");
        let color = if self.error.is_some() {
            egui::Color32::LIGHT_RED
        } else {
            ui.visuals().weak_text_color()
        };
        ui.add(egui::Label::new(egui::RichText::new(hint).color(color)).truncate())
            .on_hover_text(hint);
        let range = self
            .range
            .unwrap_or([0.0, f64::from(doc.frame_count).max(1.0)]);
        // The composition endpoint remains visible, including loop keys one frame
        // after the final rendered frame. Fit keys reveals out-of-range animation.
        let available = ui.available_size().max(Vec2::new(100.0, 60.0));
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(210.0_f32.min(available.x * 0.35), available.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("animation_tracks")
                        .show(ui, |ui| {
                            for id in self.nodes.clone() {
                                let node = &doc.nodes[&id];
                                let label: String = node.label.chars().take(80).collect();
                                egui::CollapsingHeader::new(label)
                                    .id_salt(node.id)
                                    .default_open(node.id == selected)
                                    .show(ui, |ui| {
                                        for parameter in node.operation.descriptor().parameters {
                                            let Some(scalar) =
                                                node.operation.scalar(parameter.name)
                                            else {
                                                continue;
                                            };
                                            let is_active =
                                                self.active.as_ref().is_some_and(|(id, p)| {
                                                    *id == node.id && p == parameter.name
                                                });
                                            let label = format!(
                                                "{}  {}",
                                                crate::names::setting(parameter.name, ""),
                                                scalar.keys().map_or(String::from("—"), |k| k
                                                    .len()
                                                    .to_string())
                                            );
                                            if ui
                                                .add_enabled(
                                                    !self.dragging(),
                                                    egui::Button::selectable(is_active, label),
                                                )
                                                .clicked()
                                            {
                                                self.activate(
                                                    node.id,
                                                    parameter.name,
                                                    ui.input(|i| i.modifiers.shift),
                                                );
                                                self.error = None;
                                                self.selected_node = Some(node.id);
                                                actions.push(Action::Select(node.id));
                                            }
                                        }
                                    });
                            }
                        });
                },
            );
            ui.separator();
            ui.vertical(|ui| {
                let Some((id, parameter)) = self.active.clone() else {
                    ui.label("Select a node with scalar parameters.");
                    return;
                };
                let Some(node) = doc.nodes.get(&id) else {
                    return;
                };
                let context = CanvasContext {
                    doc,
                    revision,
                    current,
                    now,
                    focus,
                    range,
                };
                if self.curve {
                    let height = (ui.available_height() - 24.0).max(60.0);
                    self.canvas(
                        ui,
                        &context,
                        Track {
                            node,
                            parameter: &parameter,
                            height,
                        },
                        &mut actions,
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("keyframe_lanes")
                        .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
                        .show(ui, |ui| {
                            for parameter in node.operation.descriptor().parameters {
                                if node.operation.scalar(parameter.name).is_some() {
                                    ui.push_id(parameter.name, |ui| {
                                        self.canvas(
                                            ui,
                                            &context,
                                            Track {
                                                node,
                                                parameter: parameter.name,
                                                height: 64.0,
                                            },
                                            &mut actions,
                                        );
                                    });
                                }
                            }
                        });
                }
            });
        });
        self.finish_marquee(ui);
        if !actions.is_empty() {
            ui.memory_mut(|m| m.request_focus(focus));
        }
        actions
    }
}

fn frame_number(time: Time, fps: Time) -> f64 {
    time.ticks as f64 / f64::from(time.rate) * fps.ticks as f64 / f64::from(fps.rate)
}
fn x_at(frame: f64, rect: Rect, range: [f64; 2]) -> f32 {
    rect.left()
        + ((frame - range[0]) / (range[1] - range[0])).clamp(-10_000.0, 10_000.0) as f32
            * rect.width()
}
fn frame_at(x: f32, rect: Rect, range: [f64; 2]) -> i64 {
    (range[0] + f64::from((x - rect.left()) / rect.width()) * (range[1] - range[0])).round() as i64
}

#[cfg(test)]
mod tests;
