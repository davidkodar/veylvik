//! Shine here: click a surface in the 3D viewer and the selected light moves to where its reflection shows at that spot, facing it. One undo step per click.
use crate::Studio;
use crate::scene_guides::Projector;
use eframe::egui;
use veylvik_document::Operation;
use veylvik_scene::{Scene, pick};

fn id() -> egui::Id {
    egui::Id::new("scene-shine")
}
impl Studio {
    fn light_selected(&self) -> bool {
        matches!(self.draft.operation, Operation::Light3d(_))
    }
    /// The Shine here button in the 3D toolbar.
    pub(crate) fn shine_button(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let light = self.light_selected();
        let on = light && ctx.data(|d| d.get_temp::<bool>(id()).unwrap_or(false));
        if ui
            .add_enabled(light, egui::Button::selectable(on, "Shine here"))
            .on_hover_text("Click a surface in the viewer and the selected light moves to where its reflection shows at that spot, keeping its distance. Click again to try another spot")
            .on_disabled_hover_text("Select a light in the scene first")
            .clicked()
        {
            ctx.data_mut(|d| d.insert_temp(id(), !on));
        }
    }
    /// While Shine here is on: a crosshair and, on a click on a surface, the light
    /// moved. Returns true when it took the pointer, so guides do not select.
    pub(crate) fn scene_shine(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        p: &Projector,
        scene: &Scene,
        response: &egui::Response,
    ) -> bool {
        if !ctx.data(|d| d.get_temp::<bool>(id()).unwrap_or(false)) {
            return false;
        }
        if !self.light_selected() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.data_mut(|d| d.insert_temp(id(), false));
            return false;
        }
        let Some(at) = response.hover_pos() else {
            return true;
        };
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let colour = egui::Color32::from_rgb(255, 220, 120);
        painter.circle_stroke(at, 10.0, egui::Stroke::new(1.5_f32, colour));
        painter.text(
            at + egui::vec2(14.0, 14.0),
            egui::Align2::LEFT_TOP,
            "Click a surface to see this light reflected there (Esc to stop)",
            egui::FontId::proportional(12.0),
            colour,
        );
        if response.clicked() {
            let doc = self.project.document.snapshot();
            let (eye, direction) = pick::ray(&p.camera, p.to_pixel(at), doc.width, doc.height);
            let result = match pick::hit(scene, eye, direction) {
                Ok(Some(hit)) => self.shine_at(scene, eye, &hit),
                Ok(None) => Err("There is no surface there to reflect the light".to_owned()),
                Err(e) => Err(e.to_string()),
            };
            if let Err(e) = result {
                self.fail(e);
            }
        }
        true
    }
    fn shine_at(&mut self, scene: &Scene, eye: [f32; 3], hit: &pick::Hit) -> Result<(), String> {
        let time = self.project.time().map_err(|e| e.to_string())?;
        // Where the light is now: a row of strips counts from its middle.
        let row: Vec<[f32; 3]> = scene
            .lights
            .iter()
            .filter(|l| l.node == self.draft.id)
            .map(|l| l.position())
            .collect();
        if row.is_empty() {
            return Err("This light is not in the scene being viewed".into());
        }
        let now = [0, 1, 2].map(|k| row.iter().map(|p| p[k]).sum::<f32>() / row.len() as f32);
        let away = [0, 1, 2].map(|k| now[k] - hit.point[k]);
        let distance = (away[0] * away[0] + away[1] * away[1] + away[2] * away[2])
            .sqrt()
            .clamp(50.0, 50_000.0);
        let (place, travel) = pick::mirrored(eye, hit, distance);
        let Operation::Light3d(light) = &mut self.draft.operation else {
            return Err("Select a light first".into());
        };
        // Move by the difference in the world; exact for a light that is not inside a
        // turned or scaled 3D transform.
        let local = light
            .placement
            .position
            .at(time)
            .map_err(|e| e.to_string())?;
        light
            .placement
            .position
            .set_at(time, [0, 1, 2].map(|k| local[k] + place[k] - now[k]))
            .map_err(|e| e.to_string())?;
        light
            .placement
            .rotation
            .set_at(time, veylvik_scene::shining(travel))
            .map_err(|e| e.to_string())?;
        self.draft_dirty = true;
        self.apply_draft();
        Ok(())
    }
}
