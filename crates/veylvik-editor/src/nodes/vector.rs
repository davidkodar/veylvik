//! Inspector controls for vector path masks.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;
use std::time::Duration;

impl Inspect for veylvik_document::nodes::Path {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Path { geometry } = self;
        {
            match shape_animation::edit(ui, geometry, cx.time) {
                Ok(true) => {
                    cx.dirty = true;
                    cx.preview_soon = true;
                    ui.ctx().request_repaint_after(Duration::from_millis(32));
                }
                Err(error) => cx.error = Some(error.to_string()),
                _ => {}
            }
        }
    }
}
