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
    pub fn sync(&mut self, doc: &Composition, revision: u64, selected: NodeId) {
        if self.source == Some((revision, selected)) {
            return;
        }
        self.source = Some((revision, selected));
        self.frames.clear();
        if let Some(node) = doc.nodes.get(&selected) {
            if let veylvik_document::Operation::Path(veylvik_document::nodes::Path { geometry }) =
                &node.operation
            {
                self.frames.extend(
                    geometry
                        .keys
                        .iter()
                        .filter_map(|key| frame(key.time, doc.frame_rate, doc.frame_count)),
                );
            }
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
        ui.weak(format!("{} keys", self.frames.len()));
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
        let rect = rect.shrink2(egui::vec2(4.0, 2.0));
        let end = count.saturating_sub(1).max(1) as f32;
        let x = |frame: i64| rect.left() + frame as f32 / end * rect.width();
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(28, 32, 40));
        let step = tick_step(count, rect.width());
        for frame in (0..count).step_by(step as usize) {
            let at = x(i64::from(frame));
            painter.vline(
                at,
                rect.top() + 15.0..=rect.bottom(),
                (1., egui::Color32::from_gray(65)),
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
                            (1., egui::Color32::from_gray(65)),
                        );
                    }
                }
            }
        }
        painter.hline(
            rect.x_range(),
            rect.bottom() - 10.,
            (1.0, egui::Color32::from_gray(70)),
        );
        if (0..i64::from(count)).contains(&current) {
            painter.vline(x(current), rect.y_range(), (1.0, ui.visuals().text_color()));
        }
        // Bound marker density without losing the current-key highlight.
        let mut previous = None;
        for frame in &self.frames {
            let at = x(*frame).round();
            if previous.is_none_or(|last: f32| at - last >= 6.0) || *frame == current {
                let c = egui::pos2(at, rect.bottom() - 10.);
                let colour = if *frame == current {
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
        if (response.clicked() || response.dragged())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let target = ((pointer.x - rect.left()) / rect.width() * end)
                .round()
                .clamp(0., count.saturating_sub(1) as f32) as i64;
            seek = Some(
                nearest
                    .filter(|f| (x(*f) - pointer.x).abs() <= 7.)
                    .unwrap_or(target),
            );
        }
        response.on_hover_text(match nearest {
            Some(frame) => format!("Selected node: {} keyed frames. Click to seek frame {frame}.\nApplied keys only; subframe keys use the nearest frame. Keys outside the composition are hidden.", self.frames.len()),
            None => "Frames · click or drag to scrub. Select an animated node to see its keyframes.".into(),
        });
        seek
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
