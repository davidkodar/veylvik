//! Two lists for looking back: the undo history, where clicking a named step goes straight to it, and the messages the status line has shown, kept after the line moved on (the last 300, errors marked).
use crate::Studio;
use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

/// Messages kept.
const KEPT: usize = 300;

struct Entry {
    at: Instant,
    error: bool,
    text: String,
}
#[derive(Default)]
pub struct Log {
    pub history: bool,
    pub messages: bool,
    entries: VecDeque<Entry>,
    last: String,
    /// Errors that arrived while the messages window was closed.
    pub unseen: usize,
}
/// Status texts that only say work is under way or over; not worth keeping.
pub(crate) fn passing(text: &str) -> bool {
    text.is_empty()
        || text == "Ready"
        || text.starts_with("Rendering")
        || text.starts_with("Sampling colour")
}
pub(crate) fn ago(seconds: u64) -> String {
    match seconds {
        0..=4 => "now".into(),
        5..=59 => format!("{seconds} s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        _ => format!("{} h ago", seconds / 3600),
    }
}
/// How many undos (negative) or redos (positive) lead from the current state to the
/// one after row `row` of a list of `undo` undoable steps followed by the redoable ones.
/// Row 0 is the earliest kept state.
pub(crate) fn distance(row: usize, undo: usize) -> i64 {
    row as i64 - undo as i64
}
impl Studio {
    /// Keep what the status line shows. Called once a frame.
    pub(crate) fn log_tick(&mut self) {
        if self.status == self.log.last {
            return;
        }
        self.log.last = self.status.clone();
        if passing(&self.status) {
            return;
        }
        if self.log.entries.len() == KEPT {
            self.log.entries.pop_front();
        }
        self.log.entries.push_back(Entry {
            at: Instant::now(),
            error: self.error,
            text: self.status.clone(),
        });
        if self.error && !self.log.messages {
            self.log.unseen += 1;
        }
    }
    /// Undo or redo several steps in a row; stops at the first that cannot be done.
    fn travel(&mut self, steps: i64) {
        for _ in 0..steps.unsigned_abs() {
            let before = self.project.document.revision();
            self.history(steps > 0);
            if self.project.document.revision() == before {
                break;
            }
        }
    }
    pub(crate) fn log_windows(&mut self, ctx: &egui::Context) {
        self.history_window(ctx);
        self.messages_window(ctx);
    }
    fn history_window(&mut self, ctx: &egui::Context) {
        if !self.log.history {
            return;
        }
        let (undo, redo) = self.project.document.steps();
        let mut open = true;
        let mut go = None;
        egui::Window::new("Undo history")
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([360.0, 420.0])
            .show(ctx, |ui| {
                ui.weak("Click a step to go to the composition as it was after it.");
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(360.0).show(ui, |ui| {
                    let rows = std::iter::once("Earliest kept state")
                        .chain(undo.iter().map(String::as_str))
                        .chain(redo.iter().map(String::as_str));
                    for (row, label) in rows.enumerate() {
                        let away = distance(row, undo.len());
                        let text = if away > 0 {
                            egui::RichText::new(label).weak()
                        } else {
                            egui::RichText::new(label)
                        };
                        let response = ui.selectable_label(away == 0, text);
                        if away == 0 {
                            response.scroll_to_me(None);
                        }
                        let response = response.on_hover_text(match away {
                            0 => "The composition as it is now".to_owned(),
                            n if n < 0 => format!("Undo {} step(s)", -n),
                            n => format!("Redo {n} step(s)"),
                        });
                        if response.clicked() && away != 0 {
                            go = Some(away);
                        }
                    }
                });
                ui.separator();
                ui.weak(format!(
                    "{} to undo, {} to redo. Up to 64 steps are kept; a new change forgets the steps that could be redone.",
                    undo.len(),
                    redo.len()
                ));
            });
        self.log.history = open;
        if let Some(steps) = go {
            self.travel(steps);
        }
    }
    fn messages_window(&mut self, ctx: &egui::Context) {
        if !self.log.messages {
            return;
        }
        self.log.unseen = 0;
        let mut open = true;
        let mut clear = false;
        egui::Window::new("Messages")
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([620.0, 420.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let errors = self.log.entries.iter().filter(|e| e.error).count();
                    ui.weak(format!(
                        "{} messages, {errors} of them errors",
                        self.log.entries.len()
                    ));
                    if ui
                        .add_enabled(!self.log.entries.is_empty(), egui::Button::new("Copy all"))
                        .clicked()
                    {
                        let text: Vec<String> = self
                            .log
                            .entries
                            .iter()
                            .map(|e| format!("{}{}", if e.error { "Error: " } else { "" }, e.text))
                            .collect();
                        ui.ctx().copy_text(text.join("\n"));
                    }
                    clear = ui
                        .add_enabled(!self.log.entries.is_empty(), egui::Button::new("Clear"))
                        .clicked();
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(360.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if self.log.entries.is_empty() {
                            ui.weak("Nothing has been reported yet.");
                        }
                        for entry in &self.log.entries {
                            ui.horizontal_wrapped(|ui| {
                                ui.add_sized(
                                    [74.0, 18.0],
                                    egui::Label::new(
                                        egui::RichText::new(ago(entry.at.elapsed().as_secs()))
                                            .weak()
                                            .monospace(),
                                    ),
                                );
                                if entry.error {
                                    ui.colored_label(egui::Color32::LIGHT_RED, &entry.text);
                                } else {
                                    ui.label(&entry.text);
                                }
                            });
                        }
                    });
            });
        self.log.messages = open;
        if clear {
            self.log.entries.clear();
        }
        // Keep the ages current while the window is open.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rows_know_their_distance_and_passing_status_is_not_kept() {
        // Three steps to undo: row 0 is the earliest state, row 3 is now, row 4 the first redo.
        assert_eq!(distance(0, 3), -3);
        assert_eq!(distance(2, 3), -1);
        assert_eq!(distance(3, 3), 0);
        assert_eq!(distance(5, 3), 2);
        assert_eq!(distance(0, 0), 0);
        for text in [
            "",
            "Ready",
            "Rendering…",
            "Rendering preview…",
            "Sampling colour…",
        ] {
            assert!(passing(text), "{text}");
        }
        for text in [
            "Marker added; right-click its flag to name it",
            "Preferences: disk full",
        ] {
            assert!(!passing(text), "{text}");
        }
        assert_eq!(ago(0), "now");
        assert_eq!(ago(42), "42 s ago");
        assert_eq!(ago(125), "2 min ago");
        assert_eq!(ago(7300), "2 h ago");
    }
}
