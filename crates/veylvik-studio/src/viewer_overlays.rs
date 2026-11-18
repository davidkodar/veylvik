//! Viewer aids that only help the eye: safe areas, centre, thirds, a pixel grid with rulers, and the colour under the pointer, read back one pixel at a time on a worker. None of them changes the picture, the project or an export.
use crate::{Studio, display::Channel, viewer::ViewTransform};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use veylvik_render::GpuFrame;

/// The value last read and where, in composition pixels.
#[derive(Clone, Copy)]
struct Reading {
    at: [u32; 2],
    value: [f32; 4],
}
#[derive(Default)]
pub struct Overlays {
    pub safe: bool,
    pub centre: bool,
    pub thirds: bool,
    pub grid: bool,
    pub rulers: bool,
    pub readout: bool,
    busy: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<Reading>>>,
    asked: Option<([u32; 2], Instant)>,
}
/// The grid's spacing in composition pixels: the smallest round step whose lines
/// stay at least `apart` screen points from each other.
pub(crate) fn grid_step(points_per_pixel: f32, apart: f32) -> u32 {
    const STEPS: [u32; 15] = [
        1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000, 10000,
    ];
    STEPS
        .into_iter()
        .find(|step| *step as f32 * points_per_pixel >= apart)
        .unwrap_or(10000)
}
/// The straight colour of a premultiplied value; transparent reads as black.
pub(crate) fn straight(value: [f32; 4]) -> [f32; 3] {
    if value[3] > 1e-6 {
        [
            value[0] / value[3],
            value[1] / value[3],
            value[2] / value[3],
        ]
    } else {
        [0.0; 3]
    }
}
fn swatch(rgb: [f32; 3]) -> Color32 {
    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    Color32::from_rgb(encode(rgb[0]), encode(rgb[1]), encode(rgb[2]))
}
impl Studio {
    /// What the viewer shows: the colour, one channel, the luminance or the coverage.
    pub(crate) fn channel_choice(&mut self, ui: &mut egui::Ui) {
        let shown = if self.view.alpha {
            "Alpha"
        } else {
            self.view.channel.label()
        };
        egui::ComboBox::from_id_salt("viewer-channel")
            .selected_text(shown)
            .width(104.0)
            .show_ui(ui, |ui| {
                for channel in Channel::ALL {
                    if ui
                        .selectable_label(!self.view.alpha && self.view.channel == channel, channel.label())
                        .clicked()
                    {
                        self.view.alpha = false;
                        self.view.channel = channel;
                    }
                }
                if ui.selectable_label(self.view.alpha, "Alpha").clicked() {
                    self.view.alpha = true;
                }
            })
            .response
            .on_hover_text(
                "Look at the colour, a single channel, the luminance or the coverage. Only the view changes, never the composition",
            );
    }
    /// The menu of viewer aids.
    pub(crate) fn overlays_menu(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.overlays;
        let on = [o.safe, o.centre, o.thirds, o.grid, o.rulers, o.readout]
            .into_iter()
            .filter(|v| *v)
            .count();
        let title = if on == 0 {
            "Overlays".to_owned()
        } else {
            format!("Overlays ({on})")
        };
        ui.menu_button(title, |ui| {
            ui.checkbox(&mut o.safe, "Safe areas")
                .on_hover_text("Action safe (90%) and title safe (80%) of the frame");
            ui.checkbox(&mut o.centre, "Centre");
            ui.checkbox(&mut o.thirds, "Thirds");
            ui.checkbox(&mut o.grid, "Grid")
                .on_hover_text("A pixel grid that keeps round spacing as you zoom");
            ui.checkbox(&mut o.rulers, "Rulers")
                .on_hover_text("Composition pixels along the top and left of the viewer");
            ui.checkbox(&mut o.readout, "Colour under the pointer")
                .on_hover_text("Position and scene-linear colour of the pixel under the pointer");
            ui.separator();
            if ui.button("Hide all").clicked() {
                (o.safe, o.centre, o.thirds, o.grid, o.rulers, o.readout) =
                    (false, false, false, false, false, false);
                ui.close();
            }
        })
        .response
        .on_hover_text("Guides drawn over the viewer; they are never part of the picture");
    }
    /// Draw the aids that are on. `pointer` is the pointer when it is over the picture
    /// with nothing above the viewer.
    pub(crate) fn viewer_overlays(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        transform: ViewTransform,
        image: Vec2,
        frame: &Arc<GpuFrame>,
        pointer: Option<Pos2>,
    ) {
        let [ox, oy, sx, sy] = transform.0;
        let scale = egui::vec2(
            canvas.width() * sx / image.x,
            canvas.height() * sy / image.y,
        );
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let screen = |x: f32, y: f32| origin + egui::vec2(x, y) * scale;
        let picture = Rect::from_min_max(origin, screen(image.x, image.y));
        let painter = ui.painter().with_clip_rect(canvas);
        let o = &self.overlays;
        let line = Stroke::new(1.0_f32, Color32::from_white_alpha(120));
        let faint = Stroke::new(1.0_f32, Color32::from_white_alpha(38));
        let step = grid_step(scale.x.min(scale.y), 48.0);
        let visible = picture.intersect(canvas);
        if o.grid && visible.is_positive() {
            let first = |from: f32, per: f32| ((from / per / step as f32).floor().max(0.0)) as u32;
            let mut x = first(visible.min.x - origin.x, scale.x) * step;
            while (x as f32) <= image.x && screen(x as f32, 0.0).x <= visible.max.x {
                let at = screen(x as f32, 0.0).x;
                painter.line_segment(
                    [egui::pos2(at, visible.min.y), egui::pos2(at, visible.max.y)],
                    faint,
                );
                x += step;
            }
            let mut y = first(visible.min.y - origin.y, scale.y) * step;
            while (y as f32) <= image.y && screen(0.0, y as f32).y <= visible.max.y {
                let at = screen(0.0, y as f32).y;
                painter.line_segment(
                    [egui::pos2(visible.min.x, at), egui::pos2(visible.max.x, at)],
                    faint,
                );
                y += step;
            }
        }
        if o.thirds {
            for third in [1.0 / 3.0, 2.0 / 3.0] {
                painter.line_segment(
                    [
                        screen(image.x * third, 0.0),
                        screen(image.x * third, image.y),
                    ],
                    line,
                );
                painter.line_segment(
                    [
                        screen(0.0, image.y * third),
                        screen(image.x, image.y * third),
                    ],
                    line,
                );
            }
        }
        if o.safe {
            for inset in [0.05, 0.1] {
                painter.rect_stroke(
                    Rect::from_min_max(
                        screen(image.x * inset, image.y * inset),
                        screen(image.x * (1.0 - inset), image.y * (1.0 - inset)),
                    ),
                    0.0,
                    line,
                    egui::StrokeKind::Inside,
                );
            }
        }
        if o.centre {
            let middle = screen(image.x * 0.5, image.y * 0.5);
            let arm = 14.0;
            painter.line_segment(
                [middle - egui::vec2(arm, 0.0), middle + egui::vec2(arm, 0.0)],
                line,
            );
            painter.line_segment(
                [middle - egui::vec2(0.0, arm), middle + egui::vec2(0.0, arm)],
                line,
            );
        }
        if o.rulers {
            let band = 16.0;
            let back = Color32::from_black_alpha(170);
            let ink = Color32::from_gray(190);
            let font = FontId::monospace(10.0);
            // Labels need more room than grid lines.
            let step = grid_step(scale.x.min(scale.y), 64.0);
            let top = Rect::from_min_size(canvas.min, egui::vec2(canvas.width(), band));
            let left = Rect::from_min_size(canvas.min, egui::vec2(band, canvas.height()));
            painter.rect_filled(top, 0.0, back);
            painter.rect_filled(left, 0.0, back);
            let mut x = (((canvas.min.x + band - origin.x) / scale.x / step as f32)
                .floor()
                .max(0.0)) as u32
                * step;
            while (x as f32) <= image.x && screen(x as f32, 0.0).x <= canvas.max.x {
                let at = screen(x as f32, 0.0).x;
                if at >= canvas.min.x + band {
                    painter.line_segment(
                        [
                            egui::pos2(at, canvas.min.y + band - 5.0),
                            egui::pos2(at, canvas.min.y + band),
                        ],
                        Stroke::new(1.0_f32, ink),
                    );
                    painter.text(
                        egui::pos2(at + 3.0, canvas.min.y + 1.0),
                        Align2::LEFT_TOP,
                        x.to_string(),
                        font.clone(),
                        ink,
                    );
                }
                x += step;
            }
            let mut y = (((canvas.min.y + band - origin.y) / scale.y / step as f32)
                .floor()
                .max(0.0)) as u32
                * step;
            while (y as f32) <= image.y && screen(0.0, y as f32).y <= canvas.max.y {
                let at = screen(0.0, y as f32).y;
                if at >= canvas.min.y + band {
                    painter.line_segment(
                        [
                            egui::pos2(canvas.min.x + band - 5.0, at),
                            egui::pos2(canvas.min.x + band, at),
                        ],
                        Stroke::new(1.0_f32, ink),
                    );
                    // Upright digits, one under the other, keep the band narrow.
                    for (row, digit) in y.to_string().chars().enumerate() {
                        painter.text(
                            egui::pos2(canvas.min.x + 4.0, at + 3.0 + row as f32 * 9.0),
                            Align2::LEFT_TOP,
                            digit,
                            font.clone(),
                            ink,
                        );
                    }
                }
                y += step;
            }
            painter.rect_filled(
                Rect::from_min_size(canvas.min, egui::vec2(band, band)),
                0.0,
                Color32::from_black_alpha(220),
            );
        }
        if !self.overlays.readout {
            return;
        }
        // The pixel under the pointer, in composition pixels and in the frame's own
        // (a preview may be rendered smaller, or cropped).
        let under = pointer.and_then(|pos| {
            let p = (pos - origin) / scale;
            (p.x >= 0.0 && p.y >= 0.0 && p.x < image.x && p.y < image.y).then_some(p)
        });
        if let Some(p) = under {
            let at = [p.x as u32, p.y as u32];
            let region = frame.region();
            let x =
                ((p.x / image.x * region.width as f32) as u32).min(region.width.saturating_sub(1));
            let y = ((p.y / image.y * region.height as f32) as u32)
                .min(region.height.saturating_sub(1));
            // Ask again when the pointer moved, and now and then for a changing picture.
            let due = self.overlays.asked.is_none_or(|(asked, when)| {
                asked != at || when.elapsed() > Duration::from_millis(120)
            });
            if due
                && region.width > 0
                && region.height > 0
                && !self.overlays.busy.swap(true, Ordering::AcqRel)
            {
                self.overlays.asked = Some((at, Instant::now()));
                let (renderer, frame, ctx) =
                    (self.renderer.clone(), frame.clone(), ui.ctx().clone());
                let (busy, latest) = (self.overlays.busy.clone(), self.overlays.latest.clone());
                std::thread::spawn(move || {
                    if let Ok(value) = renderer.read_pixel(&frame, x, y)
                        && let Ok(mut latest) = latest.lock()
                    {
                        *latest = Some(Reading { at, value });
                    }
                    busy.store(false, Ordering::Release);
                    ctx.request_repaint();
                });
            }
        }
        let reading = self.overlays.latest.lock().ok().and_then(|r| *r);
        let (Some(_), Some(reading)) = (under, reading) else {
            return;
        };
        let rgb = straight(reading.value);
        let text = format!(
            "X {:>5} Y {:>5}   R {:.4}  G {:.4}  B {:.4}  A {:.4}",
            reading.at[0], reading.at[1], rgb[0], rgb[1], rgb[2], reading.value[3]
        );
        let galley = painter.layout_no_wrap(text, FontId::monospace(12.0), Color32::WHITE);
        let chip = galley.size().y;
        let position = canvas.left_bottom() + egui::vec2(8.0 + chip + 6.0, -8.0 - galley.size().y);
        painter.rect_filled(
            Rect::from_min_max(
                position - egui::vec2(chip + 6.0, 0.0),
                position + galley.size(),
            )
            .expand(3.0),
            2.0,
            Color32::from_black_alpha(190),
        );
        painter.rect_filled(
            Rect::from_min_size(
                position - egui::vec2(chip + 6.0, 0.0),
                egui::vec2(chip, chip),
            ),
            2.0,
            swatch(rgb),
        );
        painter.galley(position, galley, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_spacing_stays_round_and_readable_and_colour_is_unpremultiplied() {
        // Zoomed far out, lines are far apart in pixels; zoomed in, down to single pixels.
        assert_eq!(grid_step(0.5, 48.0), 100);
        assert_eq!(grid_step(1.0, 48.0), 50);
        assert_eq!(grid_step(2.5, 48.0), 20);
        assert_eq!(grid_step(64.0, 48.0), 1);
        assert_eq!(grid_step(0.000_1, 48.0), 10000);
        for zoom in [0.03_f32, 0.2, 0.9, 3.0, 17.0] {
            let step = grid_step(zoom, 48.0);
            assert!(step as f32 * zoom >= 48.0 || step == 10000, "{zoom}");
        }
        assert_eq!(straight([0.2, 0.1, 0.05, 0.5]), [0.4, 0.2, 0.1]);
        assert_eq!(straight([4.0, 2.0, 1.0, 1.0]), [4.0, 2.0, 1.0]);
        assert_eq!(straight([0.3, 0.3, 0.3, 0.0]), [0.0; 3]);
        assert_eq!(swatch([1.0, 0.0, 9.0]), Color32::from_rgb(255, 0, 255));
    }
}
