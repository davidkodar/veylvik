//! Viewport navigation in logical UI points, independent of document edits.
use eframe::egui::{self, Pos2, Rect, Vec2};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform(pub [f32; 4]);
impl Default for ViewTransform {
    fn default() -> Self {
        Self([0.0, 0.0, 1.0, 1.0])
    }
}
#[derive(Default)]
pub struct Navigation {
    scale: Option<f32>,
    pan: Vec2,
}
impl Navigation {
    pub fn manual_scale(&self) -> Option<f32> {
        self.scale
    }
    pub fn pan(&self) -> [f32; 2] {
        [self.pan.x, self.pan.y]
    }
    pub fn fit(&mut self) {
        *self = Self::default();
    }
    pub fn actual_size(&mut self) {
        self.scale = Some(1.0);
        self.pan = Vec2::ZERO;
    }
    pub fn scale(&self, canvas: Rect, image: Vec2, ppp: f32) -> f32 {
        self.scale.unwrap_or_else(|| {
            (canvas.width() * ppp / image.x)
                .min(canvas.height() * ppp / image.y)
                .max(0.0001)
        })
    }
    fn image_size(&self, canvas: Rect, image: Vec2, ppp: f32) -> Vec2 {
        image * self.scale(canvas, image, ppp) / ppp
    }
    pub fn transform(&self, canvas: Rect, image: Vec2, ppp: f32) -> ViewTransform {
        let size = self.image_size(canvas, image, ppp);
        let mut origin = canvas.center() + self.pan - size * 0.5;
        let scale = self.scale(canvas, image, ppp);
        if scale >= 1.0 && (scale - scale.round()).abs() < 0.00001 {
            origin = egui::pos2(
                (origin.x * ppp).round() / ppp,
                (origin.y * ppp).round() / ppp,
            );
        }
        let origin = origin - canvas.min;
        ViewTransform([
            origin.x / canvas.width(),
            origin.y / canvas.height(),
            size.x / canvas.width(),
            size.y / canvas.height(),
        ])
    }
    fn zoom_at(&mut self, factor: f32, point: Pos2, canvas: Rect, image: Vec2, ppp: f32) {
        let old = self.scale(canvas, image, ppp);
        let new = (old * factor).clamp(0.005, 32.0);
        let relative = point - canvas.center();
        self.pan = relative - (relative - self.pan) * (new / old);
        self.scale = Some(new);
        self.limit_pan();
    }
    fn limit_pan(&mut self) {
        self.pan = self
            .pan
            .clamp(Vec2::splat(-1_000_000.0), Vec2::splat(1_000_000.0));
    }
    pub fn interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        canvas: Rect,
        image: Vec2,
    ) {
        if !ui.is_enabled() {
            return;
        }
        if response.hovered() {
            let (scroll, gesture, point) = ui.input(|input| {
                (
                    input.smooth_scroll_delta,
                    input.zoom_delta(),
                    input.pointer.hover_pos().unwrap_or(canvas.center()),
                )
            });
            let factor = if gesture != 1.0 {
                gesture
            } else {
                (scroll.y * 0.005).exp()
            };
            if factor != 1.0 {
                self.zoom_at(factor, point, canvas, image, ui.ctx().pixels_per_point());
            }
        }
        // Let small authoring handles claim a coalesced press/motion first.
        // Their press-origin hit test runs later in the same UI pass.
        if response.dragged()
            && ui.ctx().dragged_id() == Some(response.id)
            && !ui.input(|input| input.pointer.primary_pressed())
        {
            self.scale = Some(self.scale(canvas, image, ui.ctx().pixels_per_point()));
            self.pan += response.drag_delta();
            self.limit_pan();
        }
        if response.double_clicked() {
            self.fit();
        }
    }
}
/// Paint feedback inside the viewport without taking space from the image.
pub fn paint_status(ui: &egui::Ui, canvas: Rect, text: &str) {
    let painter = ui.painter().with_clip_rect(canvas);
    let galley = painter.layout_no_wrap(
        text.into(),
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().weak_text_color(),
    );
    let position = canvas.min + egui::vec2(8.0, 8.0);
    painter.rect_filled(
        Rect::from_min_size(position, galley.size()).expand(4.0),
        3.0,
        ui.visuals().panel_fill,
    );
    painter.galley(position, galley, ui.visuals().weak_text_color());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_size_aligns_to_physical_pixels_on_odd_canvases() {
        for ppp in [1.0, 1.5, 2.0] {
            let canvas = Rect::from_min_size(egui::pos2(13.25, 21.5), egui::vec2(801.0, 403.0));
            let mut nav = Navigation::default();
            nav.actual_size();
            let t = nav.transform(canvas, egui::vec2(400.0, 200.0), ppp).0;
            for origin in [
                (canvas.min.x + t[0] * canvas.width()) * ppp,
                (canvas.min.y + t[1] * canvas.height()) * ppp,
            ] {
                assert!((origin - origin.round()).abs() < 0.0001);
            }
            assert!((t[2] * canvas.width() * ppp - 400.0).abs() < 0.0001);
        }
    }
    #[test]
    fn pending_preview_feedback_keeps_canvas_and_image_geometry_stable() {
        let ctx = egui::Context::default();
        let mut geometries = Vec::new();
        for status in [
            None,
            Some("Showing previous frame · rendering"),
            None,
            Some("Showing previous frame · preview failed"),
        ] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(960.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        ui.label("Viewer");
                        let (canvas, _) =
                            ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                        let cursor = ui.cursor();
                        if let Some(text) = status {
                            paint_status(ui, canvas, text);
                        }
                        assert_eq!(ui.cursor(), cursor);
                        let transform =
                            Navigation::default().transform(canvas, egui::vec2(960.0, 540.0), 1.0);
                        geometries.push((canvas, transform));
                    });
                },
            );
        }
        assert!(geometries.windows(2).all(|pair| pair[0] == pair[1]));
    }
    #[test]
    fn fit_actual_size_and_cursor_zoom_respect_pixel_density() {
        let mut nav = Navigation::default();
        let canvas = Rect::from_min_size(egui::pos2(20.0, 40.0), egui::vec2(800.0, 400.0));
        let image = egui::vec2(400.0, 400.0);
        assert_eq!(nav.scale(canvas, image, 2.0), 2.0);
        assert_eq!(nav.transform(canvas, image, 2.0).0, [0.25, 0.0, 0.5, 1.0]);
        nav.actual_size();
        assert_eq!(
            nav.transform(canvas, image, 2.0).0,
            [0.375, 0.25, 0.25, 0.5]
        );
        let point = egui::pos2(480.0, 260.0);
        let pixel_at = |nav: &Navigation| {
            (point - canvas.center() - nav.pan) / (nav.scale(canvas, image, 2.0) / 2.0)
        };
        let pixel = pixel_at(&nav);
        nav.zoom_at(3.0, point, canvas, image, 2.0);
        assert!((pixel_at(&nav) - pixel).length() < 0.001);
        assert_eq!(nav.scale(canvas, image, 2.0), 3.0);
        nav.zoom_at(1.0e10, point, canvas, image, 2.0);
        assert_eq!(nav.scale(canvas, image, 2.0), 32.0);
        nav.fit();
        assert_eq!(nav.transform(canvas, image, 2.0).0, [0.25, 0.0, 0.5, 1.0]);
    }
}
