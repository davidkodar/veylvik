//! Eyedropper: sample exact scene-linear colour from the viewer into a colour parameter.
//! The sampled area is rendered and read back on the file worker, never on the UI thread.
use crate::viewer::ViewTransform;
use eframe::egui;
use veylvik_document::{NodeId, Operation, Time};
use veylvik_render::{ImageInputs, Plan, Region, RenderRequest, Renderer};

/// Side of the square averaged by a single click, to see past noise and grain.
const CLICK_SIZE: u32 = 5;

/// An eyedropper button for colour parameter `field`; highlighted while that field is being picked.
pub fn button(ui: &mut egui::Ui, cx: &mut crate::nodes::InspectCx<'_>, field: &'static str) {
    let active = cx.picking == Some(field);
    let response = ui
        .selectable_label(active, "Pick")
        .on_hover_text("Eyedropper: click in the viewer to sample a 5 × 5 area, or drag to average a rectangle. Samples exact scene-linear values from this node's input (or the viewed image for generators). Esc cancels.");
    if response.clicked() {
        cx.pick = Some(field);
    }
}
/// An eyedropper waiting for a click or drag in the viewer.
pub struct Picking {
    pub node: NodeId,
    pub field: &'static str,
    drag: Option<egui::Pos2>,
}
/// A finished sample, applied to the draft if it still edits the same node.
pub struct Sampled {
    pub session: NodeId,
    pub node: NodeId,
    pub field: &'static str,
    pub rgb: [f32; 3],
    pub pixels: u32,
}
impl Picking {
    pub fn new(node: NodeId, field: &'static str) -> Self {
        Self {
            node,
            field,
            drag: None,
        }
    }
    /// Draws the pick cursor and rectangle. Returns the composition-pixel area once the user
    /// clicks (a small square) or finishes a drag (that rectangle); `frame` is the displayed
    /// region of the composition.
    pub fn overlay(
        &mut self,
        ui: &egui::Ui,
        canvas: egui::Rect,
        view: ViewTransform,
        frame: Region,
    ) -> Option<Region> {
        let [ox, oy, sx, sy] = view.0;
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let scale = egui::vec2(canvas.width() * sx, canvas.height() * sy);
        let to_pixel = |p: egui::Pos2| {
            let n = (p - origin) / scale;
            [
                f64::from(frame.x) + f64::from(n.x) * f64::from(frame.width),
                f64::from(frame.y) + f64::from(n.y) * f64::from(frame.height),
            ]
        };
        let painter = ui.painter().with_clip_rect(canvas);
        let stroke = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(250, 190, 80));
        let (pointer, pressed, down, released) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
            )
        });
        // Windows over the viewer keep their own clicks.
        if let Some(p) = pointer
            .filter(|p| canvas.contains(*p) && ui.ctx().layer_id_at(*p) == Some(ui.layer_id()))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            for (a, b) in [
                (egui::vec2(-9., 0.), egui::vec2(-3., 0.)),
                (egui::vec2(3., 0.), egui::vec2(9., 0.)),
                (egui::vec2(0., -9.), egui::vec2(0., -3.)),
                (egui::vec2(0., 3.), egui::vec2(0., 9.)),
            ] {
                painter.line_segment([p + a, p + b], stroke);
            }
            if pressed {
                self.drag = Some(p);
            }
        }
        let start = self.drag?;
        let end = pointer.unwrap_or(start);
        if down || !released {
            painter.rect_stroke(
                egui::Rect::from_two_pos(start, end),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            return None;
        }
        self.drag = None;
        let (a, b) = (to_pixel(start), to_pixel(end));
        let (lo, hi) = (
            [a[0].min(b[0]), a[1].min(b[1])],
            [a[0].max(b[0]), a[1].max(b[1])],
        );
        let clamp_x =
            |v: f64| v.clamp(f64::from(frame.x), f64::from(frame.x + frame.width - 1)) as u32;
        let clamp_y =
            |v: f64| v.clamp(f64::from(frame.y), f64::from(frame.y + frame.height - 1)) as u32;
        let dragged = (end - start).length() > 4.0;
        let (x0, y0, x1, y1) = if dragged {
            (
                clamp_x(lo[0]),
                clamp_y(lo[1]),
                clamp_x(hi[0]),
                clamp_y(hi[1]),
            )
        } else {
            let half = f64::from(CLICK_SIZE / 2);
            (
                clamp_x(a[0] - half),
                clamp_y(a[1] - half),
                clamp_x(a[0] + half),
                clamp_y(a[1] + half),
            )
        };
        Some(Region {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
        })
    }
}
/// Renders `region` of `source` at `time` and returns its straight colour: the coverage-weighted
/// mean of premultiplied pixels, so transparent pixels do not pull it towards black.
pub fn sample(
    renderer: &Renderer,
    plan: &Plan,
    inputs: &ImageInputs,
    time: Time,
    region: Region,
) -> Result<([f32; 3], u32), String> {
    let frame = renderer
        .render_frame(
            plan,
            RenderRequest {
                time,
                region: Some(region),
                scene_view: None,
            },
            inputs,
        )
        .map_err(|e| e.to_string())?;
    let (mut rgb, mut alpha) = ([0f64; 3], 0f64);
    for pixel in &frame.pixels {
        for c in 0..3 {
            rgb[c] += f64::from(pixel[c]);
        }
        alpha += f64::from(pixel[3]);
    }
    if alpha <= 1e-9 {
        return Err(
            "The sampled area is fully transparent; sample where the image is visible.".into(),
        );
    }
    Ok((rgb.map(|v| (v / alpha) as f32), frame.pixels.len() as u32))
}
/// Writes `rgb` into the colour parameter `field`, keeping its alpha. False if the node has none.
pub fn apply(operation: &mut Operation, field: &str, rgb: [f32; 3]) -> bool {
    let Ok(mut value) = serde_json::to_value(&*operation) else {
        return false;
    };
    let Some(slot) = value
        .get_mut(field)
        .and_then(|v| v.as_array_mut())
        .filter(|a| a.len() == 4)
    else {
        return false;
    };
    for (c, v) in rgb.iter().enumerate() {
        slot[c] = serde_json::json!(v);
    }
    match serde_json::from_value(value) {
        Ok(next) => {
            *operation = next;
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Colour fields with eyedropper buttons, by node.
    fn fields(operation: &Operation) -> &'static [&'static str] {
        match operation {
            Operation::ChromaKey(_) => &["key_color"],
            Operation::Solid(_) => &["color"],
            Operation::Colorize(_) => &["low", "high"],
            _ => &[],
        }
    }

    #[test]
    #[ignore = "requires Vulkan"]
    fn samples_exact_straight_colour_and_refuses_transparent_areas() {
        let renderer = Renderer::new().unwrap();
        let rate = Time { ticks: 24, rate: 1 };
        let mut doc = veylvik_document::blank(16, 16, rate).unwrap();
        let solid = doc.output;
        doc.nodes.get_mut(&solid).unwrap().operation =
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [0.08, 0.45, 2.5, 0.5],
            });
        let plan = Plan::compile(&doc).unwrap();
        let region = Region {
            x: 3,
            y: 4,
            width: 5,
            height: 5,
        };
        let (rgb, pixels) = sample(
            &renderer,
            &plan,
            &ImageInputs::default(),
            Time { ticks: 0, rate: 1 },
            region,
        )
        .unwrap();
        assert_eq!(pixels, 25);
        for (a, b) in rgb.iter().zip([0.08, 0.45, 2.5]) {
            assert!((a - b).abs() < 1e-6, "{rgb:?}");
        }
        doc.nodes.get_mut(&solid).unwrap().operation =
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [1.0, 1.0, 1.0, 0.0],
            });
        let plan = Plan::compile(&doc).unwrap();
        assert!(
            sample(
                &renderer,
                &plan,
                &ImageInputs::default(),
                Time { ticks: 0, rate: 1 },
                region
            )
            .is_err()
        );
    }
    #[test]
    fn sampled_colour_lands_in_the_named_field_and_keeps_alpha() {
        let mut op: Operation =
            serde_json::from_value(veylvik_document::registry::COLORIZE.default_operation())
                .unwrap();
        assert!(apply(&mut op, "high", [2.5, 0.25, -0.5]));
        let Operation::Colorize(c) = &op else {
            panic!()
        };
        assert_eq!(c.high, [2.5, 0.25, -0.5, 1.0]);
        assert_eq!(c.low, [0.0, 0.0, 0.0, 1.0]);
        assert!(!apply(&mut op, "missing", [0.0; 3]));
        for operation in [
            &veylvik_document::registry::CHROMA_KEY,
            &veylvik_document::registry::SOLID,
            &veylvik_document::registry::COLORIZE,
        ] {
            let mut op: Operation = serde_json::from_value(operation.default_operation()).unwrap();
            for field in fields(&op) {
                assert!(
                    apply(&mut op, field, [0.1, 0.2, 0.3]),
                    "{} {field}",
                    operation.type_id
                );
            }
        }
    }
}
