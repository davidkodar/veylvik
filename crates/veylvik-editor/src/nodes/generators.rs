//! Inspector controls for generator nodes.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Solid {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Solid { color } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Linear RGB and alpha");
                crate::eyedropper::button(ui, cx, "color");
            });
            cx.dirty |= animation::color(ui, color);
        }
    }
}
impl Inspect for veylvik_document::nodes::Gradient {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Gradient { mode, .. } = self;
        {
            egui::ComboBox::from_id_salt("gradient-mode")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::GradientMode::ALL {
                        cx.dirty |= ui.selectable_value(mode, choice, choice.label()).changed();
                    }
                });
            ui.label("Use Colorize for colors or Channel mask for coverage.");
        }
    }
}
impl Inspect for veylvik_document::nodes::Noise {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Noise { seed, .. } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Seed");
                cx.dirty |= ui.add(egui::DragValue::new(seed)).changed();
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::FractalNoise {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::FractalNoise { seed, .. } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Seed");
                cx.dirty |= ui.add(egui::DragValue::new(seed)).changed();
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::Fire {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Fire { seed, .. } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Seed");
                cx.dirty |= ui.add(egui::DragValue::new(seed)).changed();
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::Atmosphere {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Atmosphere { kind, seed, .. } = self;
        {
            cx.dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
            egui::ComboBox::from_id_salt("Atmosphere-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::AtmosphereKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
        }
    }
}
impl Inspect for veylvik_document::nodes::Ellipse {}
impl Inspect for veylvik_document::nodes::Rectangle {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.label("Fill (linear RGB and opacity)");
        cx.dirty |= animation::color(ui, &mut self.color);
    }
}

impl Inspect for veylvik_document::nodes::Dust {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            ui.label("Seed");
            cx.dirty |= ui.add(egui::DragValue::new(&mut self.seed)).changed();
        });
    }
}

impl Inspect for veylvik_document::nodes::Drip {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            ui.label("Seed");
            cx.dirty |= ui.add(egui::DragValue::new(&mut self.seed)).changed();
        });
    }
}
impl Inspect for veylvik_document::nodes::Particles {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Connect Emitter to text, a shape or footage and particles are born on it; Target and they fly onto that image; Field and Obstacle to steer and bounce them; Sprite to draw a picture as each particle; Scene to film them with a 3D camera. Depth spreads them towards and away from the viewer. Put a Bloom after it for glow.");
        // The frame's size, for looks that fill the frame.
        let [width, height] = cx.frame;
        ui.menu_button("Start from a look", |ui| {
            for look in &veylvik_presets::particles::LOOKS {
                if ui.button(look.name).on_hover_text(look.hint).clicked() {
                    look.apply_wired(self, width, height, cx.is_wired("emitter"));
                    cx.dirty = true;
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(
            "Set every setting for a ready-made effect, scaled to this frame; then adjust",
        );
        ui.horizontal_wrapped(|ui| {
            cx.dirty |= ui
                .checkbox(&mut self.points, "Points")
                .on_hover_text("In a 3D scene: the same size on screen at any distance, as stars are points however near. Unticked, particles grow as they come close")
                .changed();
            cx.dirty |= ui
                .checkbox(&mut self.in_focus, "Always in focus")
                .on_hover_text("In a 3D scene: the camera's depth of field does not blur these particles")
                .changed();
            cx.dirty |= ui
                .checkbox(&mut self.with_camera, "With the camera")
                .on_hover_text("In a 3D scene: placed in front of the camera and moving with it, as dust on the lens or grains near it. X, Y and Z are where the box sits in the camera's home view; the lens still blurs them")
                .changed();
            cx.dirty |= ui
                .checkbox(&mut self.grains, "Grains")
                .on_hover_text("Sand, salt, gravel: each particle is a grain Size wide that lands on the others and the floor, slides off and piles up. Simulated (as a flock is) and can be baked")
                .changed();
        });
        if self.grains {
            ui.weak("Grains: Friction sets how steep they pile (0.2 dry sand runs into a low cone, 0.5 a steeper one, near 1 sticks like wet sand); Floor is where they come to rest, pixels above the frame's bottom edge. They also land on the scene's solids with Collide. Pour them slowly enough to fall apart: a spout with more grains than fit in it fans them out.");
        }
        // Born on a 3D object of the connected scene, in place of the box.
        let scene: bool = cx.is_wired("scene");
        if scene {
            ui.horizontal(|ui| {
                ui.label("Emit from");
                let shown = match self.emit_from {
                    None => "The box".to_string(),
                    Some(id) => cx
                        .objects
                        .iter()
                        .find(|(o, _)| *o == id)
                        .map_or("Missing object".into(), |(_, label)| label.clone()),
                };
                egui::ComboBox::from_id_salt("particle-emit-from")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        cx.dirty |= ui
                            .selectable_value(&mut self.emit_from, None, "The box")
                            .on_hover_text("Born in the box of width by height at x, y")
                            .changed();
                        for (id, label) in cx.objects.iter() {
                            cx.dirty |= ui
                                .selectable_value(&mut self.emit_from, Some(*id), label)
                                .changed();
                        }
                    })
                    .response
                    .on_hover_text("Born on the surface of a 3D object of the scene, spread evenly over it and launched straight out of it; a moving object leaves its particles behind. An imported model emits from all its parts; a flock cannot be born on one");
            });
            // Shaded by another system's grains as by its own (with Self shadow).
            let others: Vec<_> = cx
                .systems
                .iter()
                .filter(|(id, _)| *id != cx.node_id)
                .collect();
            if !others.is_empty() || self.shaded_by.is_some() {
                ui.horizontal(|ui| {
                    ui.label("Shaded by");
                    let shown = match self.shaded_by {
                        None => "Only its own grains".to_string(),
                        Some(id) => others
                            .iter()
                            .find(|(o, _)| *o == id)
                            .map_or("Missing system".into(), |(_, label)| label.clone()),
                    };
                    egui::ComboBox::from_id_salt("particle-shaded-by")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            cx.dirty |= ui
                                .selectable_value(&mut self.shaded_by, None, "Only its own grains")
                                .changed();
                            for (id, label) in &others {
                                cx.dirty |= ui
                                    .selectable_value(&mut self.shaded_by, Some(*id), label.as_str())
                                    .changed();
                            }
                        })
                        .response
                        .on_hover_text("Another particle system, filmed by the same scene and camera, whose grains shade these as their own do with Self shadow (smoke shading the sparks in it). Needs Self shadow above 0");
                });
            }
            // Gathering onto a 3D object, as onto a target picture but in depth.
            ui.horizontal(|ui| {
                ui.label("Gather on");
                let shown = match self.gather_on {
                    None => "Nothing".to_string(),
                    Some(id) => cx
                        .objects
                        .iter()
                        .find(|(o, _)| *o == id)
                        .map_or("Missing object".into(), |(_, label)| label.clone()),
                };
                egui::ComboBox::from_id_salt("particle-gather-on")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        cx.dirty |= ui
                            .selectable_value(&mut self.gather_on, None, "Nothing")
                            .changed();
                        for (id, label) in cx.objects.iter() {
                            cx.dirty |= ui
                                .selectable_value(&mut self.gather_on, Some(*id), label)
                                .changed();
                        }
                    })
                    .response
                    .on_hover_text("Particles fly to places on the surface of this 3D object over the last Homing share of their lives and stay for Hold seconds, so they become the object in 3D and follow it as it moves. Not with a Target picture, nor for a flock");
            });
        }
        // A sprite cut into a sheet of cells.
        let sprite: bool = cx.is_wired("sprite");
        if sprite {
            use veylvik_document::nodes::{SPRITE_CELLS_MOST, SpritePlay};
            ui.horizontal(|ui| {
                ui.label("Sprite sheet");
                cx.dirty |= ui
                    .add(
                        egui::DragValue::new(&mut self.sprite_columns)
                            .range(1..=SPRITE_CELLS_MOST)
                            .suffix(" across"),
                    )
                    .on_hover_text("Cells across the sprite image (1: the whole image)")
                    .changed();
                cx.dirty |= ui
                    .add(
                        egui::DragValue::new(&mut self.sprite_rows)
                            .range(1..=SPRITE_CELLS_MOST)
                            .suffix(" down"),
                    )
                    .on_hover_text("Cells down the sprite image")
                    .changed();
            });
            if self.sprite_columns * self.sprite_rows > 1 {
                ui.horizontal(|ui| {
                    ui.label("Each particle shows");
                    let choices = [
                        (SpritePlay::Random, "One cell at random", "A cell picked at random for its whole life: several sprites in one system"),
                        (SpritePlay::OverLife, "The cells over its life", "The cells in turn from first to last over its life: a sequence"),
                        (SpritePlay::AtRate, "The cells at a rate", "The cells in turn at Sprite rate a second, from a random one, round and round"),
                    ];
                    let current = choices.iter().find(|c| c.0 == self.sprite_play).map_or("", |c| c.1);
                    egui::ComboBox::from_id_salt("sprite-play")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (choice, name, hint) in choices {
                                cx.dirty |= ui
                                    .selectable_value(&mut self.sprite_play, choice, name)
                                    .on_hover_text(hint)
                                    .changed();
                            }
                        });
                });
            }
        }
        // How a flock's steering settings balance, which is not obvious from their units.
        if !matches!(self.flock_radius, veylvik_document::Scalar::Constant { value } if value <= 0.0)
        {
            ui.weak("Flock: Cohesion draws birds together, Separation keeps each clear of the birds right round it (within a third of Flock radius) and Alignment turns them the same way. At Separation 0 the birds fall into one line; much higher and the flock thins to a haze (the Murmuration look uses 40 to Cohesion 400). Pull and Vortex lead the flock; it turns in a circle of about Top speed squared over Pull.");
        }
        // A flock's bake.
        if let Some(status) = cx.flock_bake {
            ui.weak(status);
            ui.horizontal(|ui| {
                if ui
                    .button(if self.bake.is_empty() { "Bake the flock" } else { "Bake again" })
                    .on_hover_text("Simulates it over the whole composition once and writes where every particle is at every frame to a file beside the project; it is read from there from now on, exactly as it is")
                    .clicked()
                {
                    cx.ask(super::Request::BakeFlock(true));
                }
                if !self.bake.is_empty()
                    && ui
                        .button("Simulate again")
                        .on_hover_text("Stops reading the bake (the file stays) and simulates the flock")
                        .clicked()
                {
                    cx.ask(super::Request::BakeFlock(false));
                }
            });
        }
        // The colours: at birth, at the end of the life, and of a burst.
        egui::CollapsingHeader::new("Colours")
            .id_salt("particle-colours")
            .show(ui, |ui| {
                ui.weak("Linear RGBA; values above 1 glow");
                ui.label("At birth");
                cx.dirty |= animation::color(ui, &mut self.colour);
                ui.label("At the end of the life");
                cx.dirty |= animation::color(ui, &mut self.end_colour);
                ui.label("Burst");
                cx.dirty |= animation::color(ui, &mut self.burst_colour);
            });
        egui::CollapsingHeader::new("Over the life")
            .id_salt("particle-over-life")
            .show(ui, |ui| {
                ui.weak("Shape each particle from birth (left) to its end (right): drag the points");
                let flat = |v: f32| [v; 8];
                cx.dirty |= life_curve(ui, "Size", &mut self.size_over_life, 4.0, flat(1.0),
                    "Size over the life as a factor of the size, in place of Grow");
                cx.dirty |= life_curve(ui, "Opacity", &mut self.opacity_over_life, 1.0,
                    [0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0],
                    "Opacity over the life, in place of Fade");
                cx.dirty |= life_curve(ui, "Speed", &mut self.speed_over_life, 4.0, flat(1.0),
                    "How fast particles travel over their life, as a factor: 0 holds them still");
                let mut ramp = self.colour_over_life.is_some();
                if ui
                    .checkbox(&mut ramp, "Colour")
                    .on_hover_text("Eight colours over the life, in place of the colour at birth and at the end")
                    .changed()
                {
                    let (a, b) = (self.colour, self.end_colour);
                    self.colour_over_life = ramp.then(|| {
                        std::array::from_fn(|k| {
                            let t = k as f32 / 7.0;
                            std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
                        })
                    });
                    cx.dirty = true;
                }
                if let Some(colours) = &mut self.colour_over_life {
                    ui.horizontal(|ui| {
                        for colour in colours.iter_mut() {
                            cx.dirty |= ui.color_edit_button_rgba_unmultiplied(colour).changed();
                        }
                    });
                }
            });
        let obstacle: bool = cx.is_wired("obstacle");
        let scene: bool = cx.is_wired("scene");
        if scene {
            cx.dirty |= ui
                .checkbox(&mut self.collide, "Bounce off the scene's solids")
                .on_hover_text("Particles bounce off the 3D shapes, texts, cards and models of the connected scene (the first 16), coming back with Bounce of the speed they hit them at and sliding along them. A 3D text is met by its letters' outline, a model by its box")
                .changed();
        }
        if obstacle || (scene && self.collide) {
            cx.dirty |= ui
                .checkbox(&mut self.burst_on_impact, "Burst on impact")
                .on_hover_text("A particle ends where it first touches the obstacle and its burst is thrown back off it, for splashes and sparks off a surface. Unticked, bursts come at the end of each life")
                .changed();
        }
        // Only an emitter image can move out from under its particles.
        let boxed: bool = !cx.is_wired("emitter");
        // A flock reads its emitter once, at its start: following it does not apply.
        let flock = !matches!(self.flock_radius, veylvik_document::Scalar::Constant { value } if value <= 0.0);
        if !boxed && !flock {
            let name = |moments: u32| match moments {
                0 => "Off",
                1..=4 => "4 moments",
                5..=9 => "9 moments",
                _ => "16 moments",
            };
            ui.horizontal(|ui| {
                ui.label("Follow a moving emitter");
                egui::ComboBox::from_id_salt("particle-history")
                    .selected_text(name(self.history))
                    .show_ui(ui, |ui| {
                        for moments in [0, 4, 9, 16] {
                            if ui
                                .selectable_label(name(self.history) == name(moments), name(moments))
                                .clicked()
                            {
                                self.history = moments;
                                cx.dirty = true;
                            }
                        }
                    })
                    .response
                    .on_hover_text("Particles are born where the emitter was when they were born, so moving text or footage leaves them behind. The emitter is rendered that many extra times for every frame, at a half, a third or a quarter of its size; more moments give a smoother trail");
            });
        }
    }
}

/// A curve over a particle's life, switched on with its checkbox and shaped by dragging
/// its eight points between 0 and `top`; true when it changed.
fn life_curve(
    ui: &mut egui::Ui,
    name: &str,
    curve: &mut Option<[f32; 8]>,
    top: f32,
    start: [f32; 8],
    hint: &str,
) -> bool {
    let mut on = curve.is_some();
    let mut changed = false;
    if ui.checkbox(&mut on, name).on_hover_text(hint).changed() {
        *curve = on.then_some(start);
        changed = true;
    }
    let Some(values) = curve else {
        return changed;
    };
    let size = egui::vec2(ui.available_width().min(260.0), 64.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);
    let at = |k: usize, v: f32| {
        egui::pos2(
            rect.left() + rect.width() * k as f32 / 7.0,
            rect.bottom() - rect.height() * (v / top).clamp(0.0, 1.0),
        )
    };
    // Where 1 is, when the curve reaches above it.
    if top > 1.0 {
        let y = at(0, 1.0).y;
        painter.hline(
            rect.x_range(),
            y,
            egui::Stroke::new(1.0_f32, visuals.weak_text_color().gamma_multiply(0.4)),
        );
    }
    let points: Vec<egui::Pos2> = (0..8).map(|k| at(k, values[k])).collect();
    let accent = crate::design::accent(ui.ctx());
    painter.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(1.5_f32, accent),
    ));
    for point in &points {
        painter.circle_filled(*point, 3.5, accent);
    }
    if let Some(pointer) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked())
    {
        let k = (((pointer.x - rect.left()) / rect.width()) * 7.0)
            .round()
            .clamp(0.0, 7.0) as usize;
        let v = ((rect.bottom() - pointer.y) / rect.height() * top).clamp(0.0, top);
        if values[k] != v {
            values[k] = v;
            changed = true;
        }
    }
    response.on_hover_text(format!(
        "{name}: {}",
        values
            .iter()
            .map(|v| format!("{v:.2}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    changed
}
impl Inspect for veylvik_document::nodes::Force {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        use veylvik_document::nodes::ForceKind::*;
        let choices = [
            (
                Attract,
                "Attract",
                "Draws particles to its point; a negative strength drives them away",
            ),
            (
                Vortex,
                "Vortex",
                "Swings particles round an axis through its point (pointing at you; Lean tilts it towards Direction)",
            ),
            (
                Wind,
                "Wind",
                "Blows particles along Direction, leaned Lean towards you",
            ),
            (
                Turbulence,
                "Turbulence",
                "Stirs particles in swirls about Scale pixels across, never bunching them",
            ),
            (
                Drag,
                "Drag",
                "Slows particles down: 1 loses about two thirds of their speed in a second",
            ),
        ];
        ui.horizontal(|ui| {
            ui.label("Force");
            let current = choices
                .iter()
                .find(|c| c.0 == self.kind)
                .map_or("", |c| c.1);
            egui::ComboBox::from_id_salt("force-kind")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (choice, name, hint) in choices {
                        cx.dirty |= ui
                            .selectable_value(&mut self.kind, choice, name)
                            .on_hover_text(hint)
                            .changed();
                    }
                });
        });
        if let Some((_, _, hint)) = choices.iter().find(|c| c.0 == self.kind) {
            ui.weak(*hint);
        }
        ui.weak("Connect to a Particles node's Forces input; chain more forces into this one's Forces input. Radius 0 acts everywhere.");
    }
}
