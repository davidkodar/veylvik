//! Composition format drafts are applied as one document transaction.
use eframe::egui;
use veylvik_document::{Composition, Time};
#[derive(Clone, Copy)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Time,
    pub frame_count: u32,
    pub new: bool,
    pub scale_scene: bool,
}
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    settings: Option<Settings>,
}
impl Controls {
    pub fn begin(&mut self, document: &Composition, new: bool) {
        self.open = true;
        self.settings = Some(if new {
            Settings {
                width: 1920,
                height: 1080,
                frame_rate: Time { ticks: 24, rate: 1 },
                frame_count: veylvik_document::DEFAULT_FRAME_COUNT,
                new: true,
                scale_scene: false,
            }
        } else {
            Settings {
                width: document.width,
                height: document.height,
                frame_rate: document.frame_rate,
                frame_count: document.frame_count,
                new: false,
                scale_scene: false,
            }
        });
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Settings> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        let mut result = None;
        let mut close = false;
        let settings = self.settings.as_mut()?;
        egui::Window::new(if settings.new {
            "New composition"
        } else {
            "Composition settings"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("composition-format").show(ui, |ui| {
                ui.label("Width");
                ui.add(
                    egui::DragValue::new(&mut settings.width)
                        .range(1..=8192)
                        .suffix(" px"),
                );
                ui.end_row();
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut settings.height)
                        .range(1..=8192)
                        .suffix(" px"),
                );
                ui.end_row();
                ui.label("Frame count");
                ui.add(
                    egui::DragValue::new(&mut settings.frame_count)
                        .range(1..=veylvik_document::MAX_FRAME_COUNT),
                );
                ui.end_row();
                ui.label("Frame rate numerator");
                ui.add(egui::DragValue::new(&mut settings.frame_rate.ticks).range(1..=i64::MAX));
                ui.end_row();
                ui.label("Frame rate denominator");
                ui.add(egui::DragValue::new(&mut settings.frame_rate.rate).range(1..=u32::MAX));
                ui.end_row();
            });
            ui.label(format!(
                "{:.3} frames per second",
                settings.frame_rate.ticks as f64 / settings.frame_rate.rate as f64
            ));
            if !settings.new {
                ui.checkbox(
                    &mut settings.scale_scene,
                    "Scale scene controls proportionally to fit new canvas",
                );
            }
            ui.weak(if settings.new {
                "Starts with a transparent background."
            } else {
                "Unchecked changes canvas bounds only. Media keeps its placement mode; key times remain in seconds."
            });
            ui.horizontal(|ui| {
                if ui
                    .button(if settings.new { "Create" } else { "Apply" })
                    .clicked()
                {
                    result = Some(*settings);
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        self.open = open && !close;
        result
    }
}

impl Settings {
    pub fn current(doc: &Composition) -> Self {
        Self {
            width: doc.width,
            height: doc.height,
            frame_rate: doc.frame_rate,
            frame_count: doc.frame_count,
            new: false,
            scale_scene: false,
        }
    }
    pub fn commands(
        self,
        doc: &Composition,
    ) -> veylvik_document::Result<Vec<veylvik_document::Command>> {
        use veylvik_document::{
            Command,
            presets::resolution::{Sizing, map_graph},
        };
        let mut commands = Vec::new();
        if self.scale_scene && [doc.width, doc.height] != [self.width, self.height] {
            let mut nodes = doc.nodes.clone();
            let mut groups = doc.groups.clone();
            map_graph(
                &mut nodes,
                &mut groups,
                [doc.width, doc.height],
                [self.width, self.height],
                Sizing::Fit,
            )?;
            commands.extend(nodes.into_values().map(|node| Command::Put {
                node: Box::new(node),
            }));
            commands.extend(groups.into_values().map(|group| Command::PutGroup {
                group: Box::new(group),
            }));
        }
        commands.extend([
            Command::Resize {
                width: self.width,
                height: self.height,
            },
            Command::FrameRate {
                frame_rate: self.frame_rate,
            },
            Command::FrameCount {
                frame_count: self.frame_count,
            },
        ]);
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canvas_only_keeps_nodes_and_scaled_resize_is_one_reversible_edit() {
        use veylvik_document::{Command, Document, Operation};
        let doc = veylvik_document::motion_demo();
        let mut settings = Settings::current(&doc);
        settings.width = doc.width * 2;
        settings.height = doc.height * 2;
        let mut document = Document::new(doc.clone()).unwrap();
        let commands = settings.commands(&doc).unwrap();
        assert_eq!(commands.len(), 3);
        assert!(matches!(commands[0], Command::Resize { .. }));
        settings.scale_scene = true;
        let commands = settings.commands(&doc).unwrap();
        document.apply(document.revision(), &commands).unwrap();
        assert_eq!(document.history().undo_steps, 1);
        for (id, node) in &doc.nodes {
            if let Operation::Blur { radius } = &node.operation {
                let Operation::Blur { radius: changed } = &document.snapshot().nodes[id].operation
                else {
                    panic!()
                };
                assert_eq!(
                    changed.at(Time::new(0, 1).unwrap()).unwrap(),
                    radius.at(Time::new(0, 1).unwrap()).unwrap() * 2.0
                );
            }
        }
        document.undo(document.revision()).unwrap();
        assert_eq!(
            serde_json::to_value(document.snapshot()).unwrap(),
            serde_json::to_value(doc).unwrap()
        );
    }
}
