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
    /// SDR display transform for the viewer, PNG and video.
    pub display: veylvik_document::media::DisplayTransform,
}
/// Common frame sizes, in the order shown.
const FORMATS: [(&str, u32, u32); 8] = [
    ("HD", 1920, 1080),
    ("4K UHD", 3840, 2160),
    ("Vertical", 1080, 1920),
    ("Square", 1080, 1080),
    ("HD 720", 1280, 720),
    ("2K cinema", 2048, 1080),
    ("4K cinema", 4096, 2160),
    ("Vertical 4K", 2160, 3840),
];
/// Common frame rates as exact fractions.
const RATES: [(&str, i64, u32); 8] = [
    ("23.976", 24000, 1001),
    ("24", 24, 1),
    ("25", 25, 1),
    ("29.97", 30000, 1001),
    ("30", 30, 1),
    ("50", 50, 1),
    ("59.94", 60000, 1001),
    ("60", 60, 1),
];
/// Common lengths in seconds.
const LENGTHS: [u32; 6] = [5, 10, 15, 30, 60, 120];

#[derive(Default)]
pub struct Controls {
    pub open: bool,
    settings: Option<Settings>,
    /// Showing the manual fields for size, frame rate and length.
    custom: [bool; 3],
    /// The length in seconds, kept when the frame rate changes.
    seconds: f64,
}
fn rate(settings: &Settings) -> f64 {
    settings.frame_rate.ticks as f64 / f64::from(settings.frame_rate.rate.max(1))
}
fn frames(seconds: f64, settings: &Settings) -> u32 {
    ((seconds * rate(settings)).round() as u32).clamp(1, veylvik_document::MAX_FRAME_COUNT)
}
impl Controls {
    pub fn begin(&mut self, document: &Composition, new: bool) {
        self.open = true;
        let settings = if new {
            Settings {
                width: 1920,
                height: 1080,
                frame_rate: Time { ticks: 24, rate: 1 },
                frame_count: veylvik_document::DEFAULT_FRAME_COUNT,
                new: true,
                scale_scene: false,
                display: Default::default(),
            }
        } else {
            Settings {
                width: document.width,
                height: document.height,
                frame_rate: document.frame_rate,
                frame_count: document.frame_count,
                new: false,
                scale_scene: false,
                display: document.display,
            }
        };
        self.seconds = f64::from(settings.frame_count) / rate(&settings);
        // Anything that is not one of the presets opens with its manual fields.
        self.custom = [
            !FORMATS
                .iter()
                .any(|f| (f.1, f.2) == (settings.width, settings.height)),
            !RATES.iter().any(|r| {
                i128::from(r.1) * i128::from(settings.frame_rate.rate)
                    == i128::from(settings.frame_rate.ticks) * i128::from(r.2)
            }),
            !LENGTHS
                .iter()
                .any(|l| frames(f64::from(*l), &settings) == settings.frame_count),
        ];
        self.settings = Some(settings);
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Settings> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        let mut result = None;
        let mut close = false;
        let (custom, seconds) = (&mut self.custom, &mut self.seconds);
        let settings = self.settings.as_mut()?;
        egui::Window::new(if settings.new {
            "New composition"
        } else {
            "Composition settings"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_width(622.0_f32.min(ctx.content_rect().width() - 40.0));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            crate::design::heading(ui, "FORMAT");
            ui.horizontal_wrapped(|ui| {
                for (name, width, height) in FORMATS {
                    let chosen = !custom[0] && (settings.width, settings.height) == (width, height);
                    if crate::design::format_card(
                        ui,
                        name,
                        &format!("{width} × {height}"),
                        Some([width as f32, height as f32]),
                        chosen,
                    )
                    .clicked()
                    {
                        (settings.width, settings.height) = (width, height);
                        custom[0] = false;
                    }
                }
                if crate::design::format_card(ui, "Custom", "Any size", None, custom[0]).clicked() {
                    custom[0] = true;
                }
            });
            if custom[0] {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut settings.width).range(1..=8192).suffix(" px"));
                    ui.add_space(12.0);
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut settings.height).range(1..=8192).suffix(" px"));
                    if ui.button("Swap").on_hover_text("Turn the frame on its side").clicked() {
                        (settings.width, settings.height) = (settings.height, settings.width);
                    }
                });
            }
            crate::design::heading(ui, "FRAME RATE");
            ui.horizontal_wrapped(|ui| {
                for (label, ticks, per) in RATES {
                    let chosen = !custom[1]
                        && i128::from(ticks) * i128::from(settings.frame_rate.rate)
                            == i128::from(settings.frame_rate.ticks) * i128::from(per);
                    if crate::design::chip(ui, label, chosen).clicked() {
                        settings.frame_rate = Time { ticks, rate: per };
                        custom[1] = false;
                    }
                }
                if crate::design::chip(ui, "Custom", custom[1]).clicked() {
                    custom[1] = true;
                }
            });
            if custom[1] {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut settings.frame_rate.ticks).range(1..=i64::MAX));
                    ui.label("frames every");
                    ui.add(egui::DragValue::new(&mut settings.frame_rate.rate).range(1..=u32::MAX));
                    ui.label("seconds")
                        .on_hover_text("An exact fraction: 30000 frames every 1001 seconds is 29.97 fps");
                });
            }
            crate::design::heading(ui, "LENGTH");
            ui.horizontal_wrapped(|ui| {
                for length in LENGTHS {
                    let chosen = !custom[2] && (*seconds - f64::from(length)).abs() < 1e-6;
                    let label = if length >= 60 {
                        format!("{} min", length / 60)
                    } else {
                        format!("{length} s")
                    };
                    if crate::design::chip(ui, &label, chosen).clicked() {
                        *seconds = f64::from(length);
                        custom[2] = false;
                    }
                }
                if crate::design::chip(ui, "Custom", custom[2]).clicked() {
                    custom[2] = true;
                }
            });
            if custom[2] {
                ui.horizontal(|ui| {
                    let mut count = frames(*seconds, settings);
                    if ui
                        .add(egui::DragValue::new(seconds).range(0.01..=86_400.0).speed(0.1).suffix(" s"))
                        .changed()
                    {
                        count = frames(*seconds, settings);
                    }
                    ui.label("or");
                    if ui
                        .add(
                            egui::DragValue::new(&mut count)
                                .range(1..=veylvik_document::MAX_FRAME_COUNT)
                                .suffix(" frames"),
                        )
                        .changed()
                    {
                        *seconds = f64::from(count) / rate(settings);
                    }
                });
            }
            // The length is kept in seconds, so it follows a change of frame rate.
            settings.frame_count = frames(*seconds, settings);
            if !settings.new {
                crate::design::heading(ui, "DISPLAY");
                egui::ComboBox::from_id_salt("composition-display")
                    .selected_text(settings.display.label())
                    .show_ui(ui, |ui| {
                        for display in veylvik_document::media::DisplayTransform::ALL {
                            ui.selectable_value(&mut settings.display, display, display.label());
                        }
                    })
                    .response
                    .on_hover_text("How scene-linear light is shown in the viewer and written to PNG and video. Filmic rolls highlights off smoothly (ACES-style), which suits HDR and log footage; Standard clips at white. EXR always stays scene-linear.");
                ui.checkbox(
                    &mut settings.scale_scene,
                    "Scale everything in the scene to fit the new size",
                )
                .on_hover_text("Unchecked changes the canvas only. Media keeps its placement mode; key times remain in seconds.");
            }
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                let fps = rate(settings);
                ui.label(
                    egui::RichText::new(format!(
                        "{} × {}  ·  {} fps  ·  {}  ·  {} frames",
                        settings.width,
                        settings.height,
                        if (fps - fps.round()).abs() < 1e-9 { format!("{fps:.0}") } else { format!("{fps:.3}") },
                        if *seconds >= 60.0 {
                            format!("{}:{:04.1}", (*seconds / 60.0).floor(), *seconds % 60.0)
                        } else {
                            format!("{seconds:.1} s")
                        },
                        settings.frame_count
                    ))
                    .color(ui.visuals().weak_text_color()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::design::primary(ui, if settings.new { "Create" } else { "Apply" }).clicked() {
                        result = Some(*settings);
                        close = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 32.0))).clicked() {
                        close = true;
                    }
                });
            });
            if settings.new {
                ui.weak("Starts empty, with a transparent background.");
            }
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
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
            display: doc.display,
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
        if self.display != doc.display {
            commands.push(Command::Display {
                display: self.display,
            });
        }
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
            if let Operation::Blur(veylvik_document::nodes::Blur { radius, .. }) = &node.operation {
                let Operation::Blur(veylvik_document::nodes::Blur {
                    radius: changed, ..
                }) = &document.snapshot().nodes[id].operation
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
