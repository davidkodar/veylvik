//! Editor-only area-light overlay; never enters a render plan.
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use veylvik_document::{NodeId, Operation};
fn state_id() -> egui::Id {
    egui::Id::new("viewer-light-guides")
}
#[derive(Clone)]
struct State {
    target: Option<NodeId>,
    visible: bool,
    selected: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            target: None,
            visible: true,
            selected: 0,
        }
    }
}
fn state(ctx: &egui::Context) -> State {
    ctx.data_mut(|d| d.get_temp::<State>(state_id()).unwrap_or_default())
}
fn save(ctx: &egui::Context, s: State) {
    ctx.data_mut(|d| d.insert_temp(state_id(), s));
}
pub fn begin_frame(ctx: &egui::Context) {
    let mut s = state(ctx);
    s.target = None;
    save(ctx, s);
}
pub fn controls(ui: &mut egui::Ui, id: NodeId) {
    let mut s = state(ui.ctx());
    s.target = Some(id);
    ui.checkbox(&mut s.visible,"Show light guides in viewer")
        .on_hover_text("Editor overlay only. Drag enabled lights in X/Y; depth, size and rotation are in the inspector.");
    save(ui.ctx(), s);
}
pub fn select(ui: &mut egui::Ui, index: usize) {
    let mut s = state(ui.ctx());
    if ui
        .selectable_label(s.selected == index, "Select in viewer")
        .on_hover_text("Highlight this emitter and its outline")
        .clicked()
    {
        s.selected = index;
        s.visible = true;
        save(ui.ctx(), s);
    }
}
pub fn active(ctx: &egui::Context) -> bool {
    let s = state(ctx);
    s.visible && s.target.is_some()
}
pub fn label(index: usize) -> String {
    format!(
        "{} · {}",
        index + 1,
        match index {
            0 => "Key",
            1 => "Fill",
            2 => "Rim",
            _ => "Area light",
        }
    )
}
impl crate::Studio {
    pub fn light_overlay(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        transform: crate::viewer::ViewTransform,
        image: Vec2,
    ) {
        let mut s = state(ui.ctx());
        let Some(id) = s.target.filter(|_| s.visible) else {
            return;
        };
        let mut node = if self.draft.id == id {
            self.draft.clone()
        } else {
            self.text_edits
                .get(&id)
                .or_else(|| self.project.document.snapshot().nodes.get(&id))
                .unwrap()
                .clone()
        };
        // Following lights sit on the rig's tracked point; their X/Y are an offset.
        let base = node
            .inputs
            .get("track")
            .filter(|_| matches!(node.operation, Operation::LightRig(_)))
            .and_then(|track| self.project.document.snapshot().nodes.get(track))
            .and_then(|n| match &n.operation {
                Operation::PlanarTrack(track) => Some(track.data.clone()),
                _ => None,
            })
            .zip(self.project.time().ok())
            .and_then(|(track, time)| {
                veylvik_document::optical::tracked_point(&track, time)
                    .ok()
                    .flatten()
            })
            .map(|point| {
                let doc = self.project.document.snapshot();
                veylvik_document::optical::scene_point(point, doc.width, doc.height)
            });
        let studio = match &mut node.operation {
            Operation::Material(m) => &mut m.studio,
            Operation::LightRig(rig) => &mut rig.studio,
            _ => return,
        };
        let [ox, oy, sx, sy] = transform.0;
        let origin = canvas.min
            + Vec2::new(
                canvas.width() * (ox + sx * 0.5),
                canvas.height() * (oy + sy * 0.5),
            );
        // Studio coordinates are relative to a sphere radius. The viewer is a
        // lighting diagram, not a claim that the image has reconstructed depth.
        let scale = (canvas.width() * sx).min(canvas.height() * sy) / 8.0;
        if scale <= 0.0 {
            return;
        }
        let painter = ui.painter().with_clip_rect(canvas);
        painter.text(
            canvas.left_top() + Vec2::splat(12.0),
            egui::Align2::LEFT_TOP,
            "Light guides · drag X/Y · depth in inspector",
            egui::FontId::proportional(12.0),
            Color32::from_gray(190),
        );
        let mut changed = false;
        for (index, light) in studio
            .lights
            .iter_mut()
            .enumerate()
            .filter(|(_, l)| l.enabled)
        {
            let offset = base.filter(|_| light.follow).unwrap_or([0.0; 2]);
            let at = [
                light.position[0] + offset[0],
                light.position[1] + offset[1],
                light.position[2],
            ];
            let rotation = if light.follow && base.is_some() {
                veylvik_document::optical::aim(at)
            } else {
                light.rotation
            };
            let actual = origin + Vec2::new(at[0], at[1]) * scale;
            let bounds = canvas.shrink(22.0);
            let center = actual.clamp(bounds.min, bounds.max);
            let offscreen = actual.distance(center) > 1.0;
            let response = crate::handles::interact(
                ui,
                Rect::from_center_size(center, Vec2::splat(26.0)),
                ui.id().with(("light", id, index)),
            )
            .on_hover_cursor(egui::CursorIcon::Move)
            .on_hover_text(format!(
                "{}{}{} · drag to move X/Y; Z sets depth in the inspector",
                label(index),
                if light.follow && base.is_some() {
                    " · follows the track"
                } else {
                    ""
                },
                if offscreen { " (outside view)" } else { "" }
            ));
            if response.clicked() || response.drag_started() {
                s.selected = index;
            }
            if response.dragged() {
                let delta = crate::handles::drag_delta(&response) / scale;
                light.position[0] = (light.position[0] + delta.x).clamp(-100.0, 100.0);
                light.position[1] = (light.position[1] + delta.y).clamp(-100.0, 100.0);
                changed |= delta != Vec2::ZERO;
            }
            let selected = s.selected == index;
            let colour = if selected {
                Color32::from_rgb(255, 210, 115)
            } else {
                Color32::from_rgba_unmultiplied(190, 195, 210, 150)
            };
            if selected {
                painter.line_segment(
                    [center, origin],
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 210, 115, 65)),
                );
                if !offscreen {
                    let rotate = |[x, y, z]: [f32; 3]| {
                        let (a, b) = rotation[0].to_radians().sin_cos();
                        let (y, z) = (y * b - z * a, y * a + z * b);
                        let (a, b) = rotation[1].to_radians().sin_cos();
                        let (x, _z) = (x * b + z * a, -x * a + z * b);
                        let (a, b) = rotation[2].to_radians().sin_cos();
                        Vec2::new(x * b - y * a, x * a + y * b) * scale
                    };
                    let u = rotate([light.size[0] * 0.5, 0.0, 0.0]);
                    let v = rotate([0.0, light.size[1] * 0.5, 0.0]);
                    painter.add(egui::Shape::closed_line(
                        vec![
                            center - u - v,
                            center + u - v,
                            center + u + v,
                            center - u + v,
                        ],
                        Stroke::new(1.0_f32, colour),
                    ));
                }
                painter.text(
                    center + Vec2::new(16.0, -16.0),
                    egui::Align2::LEFT_BOTTOM,
                    label(index),
                    egui::FontId::proportional(12.0),
                    colour,
                );
            }
            // Vector sun icon, including rays; no font glyph dependencies.
            painter.circle_filled(center, 7.0, Color32::from_rgb(24, 26, 31));
            painter.circle_stroke(center, 5.0, Stroke::new(1.5_f32, colour));
            for ray in 0..8 {
                let a = ray as f32 * std::f32::consts::TAU / 8.0;
                let d = Vec2::new(a.cos(), a.sin());
                painter.line_segment(
                    [center + d * 8.0, center + d * 11.0],
                    Stroke::new(1.2_f32, colour),
                );
            }
            if offscreen {
                let direction = (actual - center).normalized();
                painter.line_segment(
                    [center + direction * 13.0, center + direction * 19.0],
                    Stroke::new(2.0_f32, colour),
                );
            }
        }
        let _ = image;
        save(ui.ctx(), s);
        if changed {
            if id == self.draft.id {
                self.draft = node;
            } else {
                self.text_edits.insert(id, node);
            }
            self.draft_dirty = true;
            self.playing = false;
            self.queue_draft_preview(ui.ctx());
        }
    }
}

/// A named selector for constant emitter choices; animated choices keep their keyframe controls.
pub fn source_selector(ui: &mut egui::Ui, scalar: &mut veylvik_document::Scalar) -> bool {
    let veylvik_document::Scalar::Constant { value } = scalar else {
        return false;
    };
    let mut selected = value.round() as usize;
    let before = selected;
    let name = |index| {
        if index == 0 {
            "All enabled lights".into()
        } else {
            label(index - 1)
        }
    };
    egui::ComboBox::from_id_salt("shadow-light-selection")
        .selected_text(name(selected))
        .show_ui(ui, |ui| {
            for index in 0..=8 {
                ui.selectable_value(&mut selected, index, name(index));
            }
        })
        .response
        .on_hover_text(
            "Use all enabled area lights or a single emitter in the material's light list.",
        );
    if selected != before {
        *value = selected as f32;
        true
    } else {
        false
    }
}
