//! Cached navigation over the applied scalar keys of one node.
use eframe::egui;
use veylvik_document::{Composition, NodeId, Time};

#[derive(Default)]
pub struct Keys {
    source: Option<(u64, NodeId)>,
    frames: Vec<i64>,
}

// Round to the nearest output frame, with exact integer arithmetic. Keys outside
// the composition remain authored but are excluded from this navigation strip.
fn frame(time: Time, fps: Time, count: u32) -> Option<i64> {
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

impl Keys {
    pub fn sync(&mut self, doc: &Composition, revision: u64, selected: NodeId) {
        if self.source == Some((revision, selected)) {
            return;
        }
        self.source = Some((revision, selected));
        self.frames.clear();
        if let Some(node) = doc.nodes.get(&selected) {
            for parameter in node.operation.descriptor().parameters {
                if let Some(keys) = node.operation.scalar(parameter.name).and_then(|s| s.keys()) {
                    self.frames.extend(
                        keys.iter()
                            .filter_map(|key| frame(key.time, doc.frame_rate, doc.frame_count)),
                    );
                }
            }
        }
        self.frames.sort_unstable();
        self.frames.dedup();
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

    pub fn show(&self, ui: &mut egui::Ui, current: i64, count: u32) -> Option<i64> {
        let mut seek = None;
        for (forward, label, hint) in [
            (false, "Prev key", "Alt+Left"),
            (true, "Next key", "Alt+Right"),
        ] {
            let target = self.adjacent(current, forward);
            if ui
                .add_enabled(target.is_some(), egui::Button::new(label))
                .on_hover_text(hint)
                .clicked()
            {
                seek = target;
            }
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(40.0), 18.0),
            egui::Sense::click_and_drag(),
        );
        let rect = rect.shrink2(egui::vec2(4.0, 2.0));
        let end = count.saturating_sub(1).max(1) as f32;
        let x = |frame: i64| rect.left() + frame as f32 / end * rect.width();
        let painter = ui.painter();
        painter.hline(
            rect.x_range(),
            rect.center().y,
            ui.visuals().widgets.inactive.bg_stroke,
        );
        // Dense keys share pixels; paint at most one marker per logical pixel.
        let mut previous = None;
        for frame in &self.frames {
            let at = x(*frame).round();
            if previous != Some(at) {
                painter.vline(
                    at,
                    rect.top()..=rect.bottom(),
                    (2.0, ui.visuals().selection.stroke.color),
                );
                previous = Some(at);
            }
        }
        if (0..i64::from(count)).contains(&current) {
            painter.circle_filled(
                egui::pos2(x(current), rect.center().y),
                3.0,
                ui.visuals().text_color(),
            );
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
        if response.clicked() || response.dragged() {
            seek = nearest;
        }
        response.on_hover_text(match nearest {
            Some(frame) => format!("Selected node: {} keyed frames. Click to seek frame {frame}.\nApplied keys only; subframe keys use the nearest frame. Keys outside the composition are hidden.", self.frames.len()),
            None => "No applied scalar keys inside this composition on the selected node.".into(),
        });
        seek
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Key, Node, Operation, Scalar};
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
            Operation::Transform {
                x: curve.clone(),
                y: curve,
            },
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
