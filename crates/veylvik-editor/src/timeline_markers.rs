//! Markers on the ruler: flags that jump to their time when clicked and rename or delete from their menu; M adds one at the current position, comma and period step between them. Every change is one document command, so it undoes.
use crate::{Studio, key_timeline::Action};
use eframe::egui::{self, Color32, FontId, Rect, Stroke};
use veylvik_document::{Command, Composition, Time, markers};

const INK: Color32 = Color32::from_rgb(240, 180, 70);

/// The markers that fall inside the composition, as (frame, index).
pub(crate) fn frames(doc: &Composition) -> Vec<(i64, usize)> {
    doc.markers
        .iter()
        .enumerate()
        .filter_map(|(index, marker)| {
            crate::key_timeline::frame(marker.time, doc.frame_rate, doc.frame_count)
                .map(|frame| (frame, index))
        })
        .collect()
}
/// The marker frame after or before `current`.
pub(crate) fn adjacent(doc: &Composition, current: i64, forward: bool) -> Option<i64> {
    let frames = frames(doc);
    if forward {
        frames.iter().map(|(f, _)| *f).find(|f| *f > current)
    } else {
        frames.iter().rev().map(|(f, _)| *f).find(|f| *f < current)
    }
}
/// Draw the flags over the ruler `rect` and answer clicks and their menus.
pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    end: f32,
    doc: &Composition,
    timecode: bool,
    editable: bool,
    actions: &mut Vec<Action>,
) {
    let placed = frames(doc);
    let x = |frame: i64| rect.left() + frame as f32 / end * rect.width();
    for (slot, (frame, index)) in placed.iter().enumerate() {
        let marker = &doc.markers[*index];
        let at = x(*frame);
        let painter = ui.painter().with_clip_rect(rect);
        painter.vline(
            at,
            rect.y_range(),
            Stroke::new(1.0_f32, INK.gamma_multiply(0.55)),
        );
        let flag = Rect::from_min_size(egui::pos2(at, rect.top()), egui::vec2(9.0, 9.0));
        painter.add(egui::Shape::convex_polygon(
            vec![
                flag.left_top(),
                flag.right_top(),
                egui::pos2(flag.right(), flag.center().y),
                flag.left_bottom(),
            ],
            INK,
            Stroke::NONE,
        ));
        // The name, as far as the next flag leaves room.
        let room = placed
            .get(slot + 1)
            .map_or(rect.right(), |(next, _)| x(*next))
            - at
            - 14.0;
        if !marker.label.is_empty() && room > 24.0 {
            let galley =
                painter.layout_no_wrap(marker.label.clone(), FontId::proportional(10.0), INK);
            let clip = Rect::from_min_size(
                egui::pos2(at + 12.0, rect.top() - 1.0),
                egui::vec2(room, 12.0),
            );
            ui.painter()
                .with_clip_rect(clip.intersect(rect))
                .galley(clip.min, galley, INK);
        }
        let response = ui.interact(
            flag.expand(3.0),
            ui.id().with(("marker", *index)),
            egui::Sense::click(),
        );
        let position = if timecode {
            crate::timecode::format(*frame, doc.frame_rate)
        } else {
            format!("Frame {frame}")
        };
        let name = if marker.label.is_empty() {
            "Marker"
        } else {
            marker.label.as_str()
        };
        let response = response.on_hover_text(format!(
            "{name} · {position}\nClick to go there; right-click to rename or delete"
        ));
        if response.clicked() {
            actions.push(Action::Seek(*frame));
        }
        let draft = ui.id().with(("marker-name", *index));
        let time = marker.time;
        response.context_menu(|ui| {
            if !editable {
                ui.weak("Finish the current edit to change markers");
                return;
            }
            let mut text: String = ui.data_mut(|d| {
                d.get_temp_mut_or_insert_with(draft, || marker.label.clone())
                    .clone()
            });
            ui.label("Marker name");
            let field = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .char_limit(markers::MAX_LABEL)
                    .desired_width(180.0),
            );
            ui.data_mut(|d| d.insert_temp(draft, text.clone()));
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Rename").clicked() || entered {
                actions.push(Action::Commit(vec![Command::Markers {
                    markers: markers::with(&doc.markers, time, text.trim()),
                }]));
                ui.data_mut(|d| d.remove::<String>(draft));
                ui.close();
            }
            if ui.button("Delete marker").clicked() {
                actions.push(Action::Commit(vec![Command::Markers {
                    markers: markers::without(&doc.markers, time),
                }]));
                ui.data_mut(|d| d.remove::<String>(draft));
                ui.close();
            }
        });
    }
}
impl Studio {
    /// Add a marker at the current position; with one there already, nothing changes.
    pub(crate) fn add_marker(&mut self) {
        if !self.apply_draft() {
            return;
        }
        let doc = self.project.document.snapshot();
        let Ok(time) = Time::at_frame(self.project.frame, doc.frame_rate) else {
            return;
        };
        if frames(doc)
            .iter()
            .any(|(frame, _)| *frame == self.project.frame)
        {
            self.status =
                "There is a marker here already; right-click it to rename or delete".into();
            return;
        }
        if doc.markers.len() >= markers::MAX_MARKERS {
            self.fail("A composition holds at most 256 markers");
            return;
        }
        let next = markers::with(&doc.markers, time, "");
        match self.project.apply(&[Command::Markers { markers: next }]) {
            Ok(()) => {
                self.refresh_draft();
                self.status = "Marker added; right-click its flag to name it".into();
            }
            Err(error) => self.fail(error),
        }
    }
    /// Go to the marker before or after the current position.
    pub(crate) fn go_to_marker(&mut self, forward: bool) {
        if let Some(frame) = adjacent(
            self.project.document.snapshot(),
            self.project.frame,
            forward,
        ) && self.apply_draft()
        {
            self.project.frame = frame;
            self.transport.pause();
            self.request_preview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markers_map_to_frames_and_stepping_skips_those_outside_the_composition() {
        let rate = Time::new(24, 1).unwrap();
        let mut doc = veylvik_document::blank(64, 36, rate).unwrap();
        doc.frame_count = 48;
        for (seconds, label) in [(0, "Start"), (1, "Middle"), (10, "Beyond the end")] {
            doc.markers = markers::with(&doc.markers, Time::new(seconds, 1).unwrap(), label);
        }
        assert_eq!(frames(&doc), vec![(0, 0), (24, 1)]);
        assert_eq!(adjacent(&doc, 0, true), Some(24));
        assert_eq!(adjacent(&doc, 24, true), None);
        assert_eq!(adjacent(&doc, 30, false), Some(24));
        assert_eq!(adjacent(&doc, 24, false), Some(0));
        assert_eq!(adjacent(&doc, 0, false), None);
    }
}
