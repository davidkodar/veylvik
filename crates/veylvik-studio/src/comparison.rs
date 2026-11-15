//! Session-only GPU reference comparison. Never modifies the composition.
use crate::{
    Studio,
    display::{DisplayFrame, Paint},
    viewer::Navigation,
};
use eframe::{
    egui::{self, Rect},
    egui_wgpu,
};
use std::sync::Arc;

#[derive(Default, PartialEq, Clone, Copy)]
enum Mode {
    #[default]
    Off,
    Wipe,
    SideBySide,
}
#[derive(Default)]
pub struct Comparison {
    reference: Option<Arc<DisplayFrame>>,
    label: String,
    size: [u32; 2],
    mode: Mode,
    swapped: bool,
    divider: Option<f32>,
    dragging: bool,
    navigation: Navigation,
}
impl Comparison {
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"captured":self.reference.is_some(),"mode":match self.mode {Mode::Off=>"off",Mode::Wipe=>"wipe",Mode::SideBySide=>"side_by_side"},"reference":self.label,"size":self.size,"swapped":self.swapped,"divider":self.divider.unwrap_or(0.5),"scale":self.navigation.manual_scale(),"pan":self.navigation.pan()})
    }
    pub fn fit(&mut self) {
        self.navigation.fit();
    }
    pub fn actual_size(&mut self) {
        self.navigation.actual_size();
    }
    pub fn active(&self) -> bool {
        self.reference.is_some() && self.mode != Mode::Off
    }
}
impl Studio {
    pub(super) fn comparison_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let ready = self.displayed.is_some() && self.displayed_ticket == self.wanted && !self.preview_pending && !self.draft_dirty;
            if ui.add_enabled(ready, egui::Button::new(if self.comparison.reference.is_some() {"Replace A"} else {"Capture A"}))
                .on_hover_text("Keep this completed preview as a GPU reference. B remains live. Reference includes its preview colour transform; it is not saved in the project.").clicked() {
                self.comparison.reference = self.displayed.clone();
                let doc = self.project.document.snapshot();
                self.comparison.size = [doc.width, doc.height];
                let node = self.project.preview.unwrap_or(doc.output);
                self.comparison.label = format!("Frame {} · revision {} · {}", self.project.frame, self.project.document.revision(), doc.nodes[&node].label);
                self.comparison.mode = Mode::Wipe;
            }
            ui.add_enabled_ui(self.comparison.reference.is_some(), |ui| {
                egui::ComboBox::from_id_salt("comparison-mode")
                    .selected_text(match self.comparison.mode { Mode::Off => "Compare: Off", Mode::Wipe => "Compare: Wipe", Mode::SideBySide => "Compare: Side by side" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.comparison.mode, Mode::Off, "Off");
                        ui.selectable_value(&mut self.comparison.mode, Mode::Wipe, "Wipe");
                        ui.selectable_value(&mut self.comparison.mode, Mode::SideBySide, "Side by side");
                    }).response.on_hover_text("Compare captured A with live B. Comparison is view-only; turn it off to manipulate objects.");
                if ui.button("Swap A/B").on_hover_text("Swap the reference and live sides").clicked() { self.comparison.swapped = !self.comparison.swapped; }
                if ui.button("Clear A").on_hover_text("Release the captured reference and its GPU memory").clicked() { self.comparison = Comparison::default(); }
            });
            if self.comparison.active() {
                ui.weak("A: reference · B: live").on_hover_text(&self.comparison.label);
            }
        });
    }
    pub(super) fn comparison_view(&mut self, ui: &mut egui::Ui) {
        let Some(live) = self.displayed.clone() else {
            return;
        };
        let Some(reference) = self.comparison.reference.clone() else {
            return;
        };
        let (canvas, _) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1., 1.)),
            egui::Sense::hover(),
        );
        let side = self.comparison.mode == Mode::SideBySide;
        let center = canvas.center().x;
        let panes = if side {
            [
                Rect::from_min_max(canvas.min, egui::pos2(center - 2., canvas.max.y)),
                Rect::from_min_max(egui::pos2(center + 2., canvas.min.y), canvas.max),
            ]
        } else {
            [canvas, canvas]
        };
        let fraction = self.comparison.divider.unwrap_or(0.5);
        let split = canvas.left() + canvas.width() * fraction;
        let handle = Rect::from_min_max(
            egui::pos2(split - 7., canvas.top()),
            egui::pos2(split + 7., canvas.bottom()),
        );
        let doc = self.project.document.snapshot();
        let image = egui::vec2(doc.width as f32, doc.height as f32);
        let view = self.effective_view();
        for (i, pane) in panes.iter().copied().enumerate() {
            let hit_rect = if side {
                pane
            } else if i == 0 {
                Rect::from_min_max(canvas.min, egui::pos2(handle.left(), canvas.bottom()))
            } else {
                Rect::from_min_max(egui::pos2(handle.right(), canvas.top()), canvas.max)
            };
            let response = ui.interact(
                hit_rect,
                ui.id().with(("compare-pan", i)),
                egui::Sense::click_and_drag(),
            );
            if !self.comparison.dragging
                && (side
                    || !ui.input(|input| {
                        input
                            .pointer
                            .hover_pos()
                            .is_some_and(|p| handle.contains(p))
                            || input
                                .pointer
                                .press_origin()
                                .is_some_and(|p| handle.contains(p))
                    }))
            {
                self.comparison
                    .navigation
                    .interact(ui, &response, pane, image);
            }
        }
        if !side {
            let response = ui
                .interact(
                    handle,
                    ui.id().with("wipe-handle"),
                    egui::Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text("Drag to reveal A or B. Double-click to centre the wipe.");
            let pointer = ui.input(|i| {
                (
                    i.pointer.primary_pressed(),
                    i.pointer.primary_down(),
                    i.pointer.interact_pos(),
                    i.pointer.press_origin(),
                )
            });
            if pointer.0
                && ui.is_enabled()
                && pointer.3.is_some_and(|p| {
                    handle.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
                })
            {
                self.comparison.dragging = true;
            }
            if !pointer.1 {
                self.comparison.dragging = false;
            }
            if self.comparison.dragging
                && let Some(p) = pointer.2
            {
                self.comparison.divider =
                    Some(((p.x - canvas.left()) / canvas.width()).clamp(0.02, 0.98));
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if response.double_clicked() {
                self.comparison.divider = Some(0.5);
            }
        }
        for (i, pane) in panes.iter().copied().enumerate() {
            let is_reference = (i == 0) != self.comparison.swapped;
            let source = if is_reference { &reference } else { &live };
            let dimensions = if is_reference {
                self.comparison.size
            } else {
                [doc.width, doc.height]
            };
            let size = egui::vec2(dimensions[0] as f32, dimensions[1] as f32);
            // The same navigation and canvas dimensions align A/B at identical project coordinates.
            let transform =
                self.comparison
                    .navigation
                    .transform(pane, image, ui.ctx().pixels_per_point());
            let mut settings = view;
            settings.filmic = source.settings.filmic;
            let mut transform = transform;
            transform.0[2] *= size.x / image.x;
            transform.0[3] *= size.y / image.y;
            let frame =
                self.display
                    .frame(&self.renderer, source.frame.clone(), settings, transform);
            let clip = if side {
                pane
            } else if i == 0 {
                Rect::from_min_max(canvas.min, egui::pos2(split, canvas.bottom()))
            } else {
                Rect::from_min_max(egui::pos2(split, canvas.top()), canvas.max)
            };
            let painter = ui.painter().with_clip_rect(clip);
            painter.add(egui_wgpu::Callback::new_paint_callback(pane, Paint(frame)));
            let label = if is_reference {
                format!("A · {}", self.comparison.label)
            } else {
                format!(
                    "B · Live · frame {}{}",
                    self.project.frame,
                    if self.displayed_ticket != self.wanted {
                        " · updating"
                    } else {
                        ""
                    }
                )
            };
            let pos = clip.left_top() + egui::vec2(10., 10.);
            let galley = painter.layout_no_wrap(
                label,
                egui::FontId::proportional(13.),
                egui::Color32::WHITE,
            );
            painter.rect_filled(
                Rect::from_min_size(pos, galley.size()).expand(4.),
                3.,
                egui::Color32::from_black_alpha(210),
            );
            painter.galley(pos, galley, egui::Color32::WHITE);
        }
        if !side {
            let painter = ui.painter().with_clip_rect(canvas);
            painter.line_segment(
                [
                    egui::pos2(split, canvas.top()),
                    egui::pos2(split, canvas.bottom()),
                ],
                egui::Stroke::new(2.0_f32, ui.visuals().selection.bg_fill),
            );
            painter.circle_filled(
                egui::pos2(split, canvas.center().y),
                6.,
                ui.visuals().selection.bg_fill,
            );
        }
    }
}
