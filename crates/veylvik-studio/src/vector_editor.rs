use eframe::egui;
use veylvik_document::vector::{
    FillRule, MAX_PATH_COMMANDS, MAX_PATH_COORDINATE, PathCommand, PathMask,
};

pub fn edit(ui: &mut egui::Ui, path: &mut PathMask, locked: bool) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(!locked, |ui| {
        egui::ComboBox::from_id_salt("path_fill")
            .selected_text(match path.fill_rule {
                FillRule::NonZero => "Nonzero winding",
                FillRule::EvenOdd => "Even-odd holes",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut path.fill_rule, FillRule::NonZero, "Nonzero winding")
                    .changed();
                changed |= ui
                    .selectable_value(&mut path.fill_rule, FillRule::EvenOdd, "Even-odd holes")
                    .changed();
            });
    });
    ui.label("Use Draw mask or Add contour above the viewer.");
    ui.label("Click for corners; drag for curves. Close at the first point.");
    ui.weak("Right-click a point to insert, delete, smooth or sharpen.");
    ui.weak("Transform mask moves, rotates or scales all contours together.");
    ui.weak("Preview selected to edit. Apply saves; Revert cancels.");
    ui.add_space(6.0);
    ui.collapsing("Precision coordinates", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("path_points")
            .max_height(300.0)
            .show(ui, |ui| {
                for (index, command) in path.commands.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.separator();
                        match command {
                            PathCommand::Move { to } => {
                                ui.label("Contour start");
                                changed |= point(ui, "Position", to);
                            }
                            PathCommand::Line { to } => {
                                ui.label("Line");
                                changed |= point(ui, "End", to);
                            }
                            PathCommand::Quad { control, to } => {
                                ui.label("Quadratic curve");
                                changed |= point(ui, "Control", control);
                                changed |= point(ui, "End", to);
                            }
                            PathCommand::Cubic {
                                control1,
                                control2,
                                to,
                            } => {
                                ui.label("Cubic curve");
                                changed |= point(ui, "Control 1", control1);
                                changed |= point(ui, "Control 2", control2);
                                changed |= point(ui, "End", to);
                            }
                            PathCommand::Close {} => {
                                ui.weak("Close contour");
                            }
                        }
                    });
                }
            });
        ui.add_enabled_ui(!locked, |ui| {
            ui.horizontal(|ui| {
                let room = path.commands.len() < MAX_PATH_COMMANDS;
                if ui
                    .add_enabled(room, egui::Button::new("Add line"))
                    .clicked()
                {
                    let index = path.commands.len().saturating_sub(1);
                    path.commands
                        .insert(index, PathCommand::Line { to: [250.0, 350.0] });
                    changed = true;
                }
                if ui
                    .add_enabled(room, egui::Button::new("Add curve"))
                    .clicked()
                {
                    let index = path.commands.len().saturating_sub(1);
                    path.commands.insert(
                        index,
                        PathCommand::Cubic {
                            control1: [200.0, 400.0],
                            control2: [300.0, 400.0],
                            to: [250.0, 350.0],
                        },
                    );
                    changed = true;
                }
            });
            if ui
                .add_enabled(
                    path.commands.len() + 5 <= MAX_PATH_COMMANDS,
                    egui::Button::new("Add inner contour"),
                )
                .clicked()
            {
                path.commands.extend([
                    PathCommand::Move { to: [180.0, 150.0] },
                    PathCommand::Line { to: [320.0, 150.0] },
                    PathCommand::Line { to: [320.0, 250.0] },
                    PathCommand::Line { to: [180.0, 250.0] },
                    PathCommand::Close {},
                ]);
                changed = true;
            }
            let start = path
                .commands
                .iter()
                .rposition(|c| matches!(c, PathCommand::Move { .. }))
                .unwrap_or(0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        path.commands.len().saturating_sub(start) > 3,
                        egui::Button::new("Remove last segment"),
                    )
                    .clicked()
                {
                    path.commands.remove(path.commands.len() - 2);
                    changed = true;
                }
                if ui
                    .add_enabled(start > 0, egui::Button::new("Remove last contour"))
                    .clicked()
                {
                    path.commands.truncate(start);
                    changed = true;
                }
            });
        });
    });
    changed
}
fn point(ui: &mut egui::Ui, label: &str, value: &mut [f32; 2]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for coordinate in value {
            changed |= ui
                .add(
                    egui::DragValue::new(coordinate)
                        .speed(0.5)
                        .range(-MAX_PATH_COORDINATE..=MAX_PATH_COORDINATE),
                )
                .changed();
        }
    });
    changed
}
