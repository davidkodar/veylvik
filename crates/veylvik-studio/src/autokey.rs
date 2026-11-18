//! Automatic keyframes, on or off. Off (the default): changing an animated setting where it has no key is a try-out that is shown but not kept until the artist adds the keyframe; it is dropped by moving on. One check covers every slider and handle: a pending edit that holds more keys than the saved nodes, without a key button having been pressed, is a try-out.
use crate::Studio;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use veylvik_document::{Command, Operation};

/// Set by the key buttons: the keys added in this frame were asked for.
static ASKED: AtomicBool = AtomicBool::new(false);
/// A key button was pressed; what it added is the artist's own.
pub fn asked() {
    ASKED.store(true, Ordering::Relaxed);
}
/// Keys held anywhere in `operation`: animated numbers, colours, vectors and shapes.
pub(crate) fn keys(operation: &Operation) -> usize {
    fn count(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(name, inner)| match inner {
                    serde_json::Value::Array(keys) if name == "keys" => keys.len(),
                    other => count(other),
                })
                .sum(),
            serde_json::Value::Array(items) => items.iter().map(count).sum(),
            _ => 0,
        }
    }
    serde_json::to_value(operation).map_or(0, |value| count(&value))
}
/// What the try-out bar asked for.
enum Choice {
    Keep,
    Discard,
}
impl Studio {
    /// Keys in the pending edit, and in the same nodes as saved.
    fn pending_keys(&self) -> (usize, usize) {
        let doc = self.project.document.snapshot();
        let (mut pending, mut saved) = (0, 0);
        for command in self.draft_commands() {
            if let Command::Put { node } = command {
                pending += keys(&node.operation);
                saved += doc
                    .nodes
                    .get(&node.id)
                    .map_or(0, |node| keys(&node.operation));
            }
        }
        (pending, saved)
    }
    /// Notice keys that an edit added by itself. Called once a frame, before pending
    /// edits may be applied.
    pub(crate) fn autokey_tick(&mut self, ctx: &egui::Context) {
        let asked = ASKED.swap(false, Ordering::Relaxed);
        if !self.draft_dirty {
            self.tryout = None;
            self.keys_accepted = None;
            return;
        }
        let (pending, saved) = self.pending_keys();
        let accepted = self.keys_accepted.get_or_insert(saved);
        if asked || pending < *accepted {
            *accepted = pending;
        }
        if pending <= *accepted {
            self.tryout = None;
            return;
        }
        if self.preferences.values.auto_key {
            *accepted = pending;
            self.status = format!(
                "Keyframe added at {} (automatic keyframes are on)",
                self.position_text()
            );
        } else if self.tryout.is_none() {
            self.tryout = Some(self.project.frame);
            ctx.request_repaint();
        }
    }
    fn position_text(&self) -> String {
        let doc = self.project.document.snapshot();
        if self.preferences.values.time_display == crate::timecode::Display::Timecode {
            crate::timecode::format(self.project.frame, doc.frame_rate)
        } else {
            format!("frame {}", self.project.frame)
        }
    }
    /// Keep the try-out: its value becomes a keyframe.
    pub(crate) fn keep_tryout(&mut self) -> bool {
        self.tryout = None;
        self.keys_accepted = Some(usize::MAX);
        self.apply_draft()
    }
    /// Drop a try-out instead of applying it. True when there was one.
    pub(crate) fn drop_tryout(&mut self) -> bool {
        if self.tryout.take().is_none() {
            return false;
        }
        self.keys_accepted = None;
        self.refresh_draft();
        self.request_preview();
        self.status =
            "Try-out dropped: this setting is animated and automatic keyframes are off".into();
        true
    }
    pub(crate) fn toggle_auto_key(&mut self) {
        let on = !self.preferences.values.auto_key;
        self.preferences.values.auto_key = on;
        self.status = if on {
            "Automatic keyframes on: changing an animated setting adds a keyframe".into()
        } else {
            "Automatic keyframes off: changes to animated settings are try-outs until you add the keyframe".into()
        };
        if let Err(error) = self.preferences.persist() {
            self.fail(format!("Preferences: {error}"));
        }
    }
    /// The bar shown while a try-out is pending.
    pub(crate) fn tryout_bar(&mut self, ctx: &egui::Context) {
        if self.tryout.is_none() {
            return;
        }
        let mut choice = None;
        egui::Area::new(egui::Id::new("tryout-bar"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 92.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .stroke(egui::Stroke::new(1.0_f32, crate::design::accent(ctx)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Trying a value on an animated setting at {}. It is not a keyframe yet.",
                                self.position_text()
                            ));
                            if crate::design::primary(ui, "Add keyframe").clicked() {
                                choice = Some(Choice::Keep);
                            }
                            if ui
                                .button("Discard")
                                .on_hover_text("Go back to the animation as it was. Moving to another frame does the same")
                                .clicked()
                            {
                                choice = Some(Choice::Discard);
                            }
                            if ui
                                .button("Always add keyframes")
                                .on_hover_text("Turn automatic keyframes on (Edit > Automatic keyframes) and keep this one")
                                .clicked()
                            {
                                self.toggle_auto_key();
                                choice = Some(Choice::Keep);
                            }
                        });
                    });
            });
        match choice {
            Some(Choice::Keep) => {
                self.keep_tryout();
            }
            Some(Choice::Discard) => {
                self.drop_tryout();
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Scalar, Time, nodes};
    #[test]
    fn keys_are_counted_wherever_a_setting_holds_them() {
        let t = |s| Time::new(s, 1).unwrap();
        let mut blur = nodes::Blur {
            radius: 4.0.into(),
            filter: Default::default(),
        };
        assert_eq!(keys(&Operation::Blur(blur.clone())), 0);
        // The first key and each one after it count; changing a key's value does not.
        blur.radius.set_key_at(t(0), 4.0).unwrap();
        assert_eq!(keys(&Operation::Blur(blur.clone())), 1);
        blur.radius.set_key_at(t(2), 9.0).unwrap();
        assert_eq!(keys(&Operation::Blur(blur.clone())), 2);
        blur.radius.set_key_at(t(2), 12.0).unwrap();
        assert_eq!(keys(&Operation::Blur(blur.clone())), 2);
        assert!(matches!(
            blur.radius,
            Scalar::Linear { .. } | Scalar::Smooth { .. } | Scalar::Step { .. }
        ));
        // Several animated settings in one node add up.
        let mut shadow = nodes::Shadow {
            follow_lighting: false,
            distance: 20.0.into(),
            softness: 8.0.into(),
            strength: 0.65.into(),
            x: 8.0.into(),
            y: 12.0.into(),
            light: 0.0.into(),
        };
        assert_eq!(keys(&Operation::Shadow(shadow.clone())), 0);
        shadow.softness.set_key_at(t(0), 8.0).unwrap();
        shadow.softness.set_key_at(t(1), 20.0).unwrap();
        shadow.x.set_key_at(t(1), 30.0).unwrap();
        assert_eq!(keys(&Operation::Shadow(shadow)), 3);
    }
}
