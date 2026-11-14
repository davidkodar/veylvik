//! Per-node inspector UI, grouped like `veylvik_document::nodes`. Each node edits its own
//! fields and reports what changed through `InspectCx`; it never sees the whole editor.
use eframe::egui;
use veylvik_document::Time;

/// What a node's inspector may read, and what it reports back to the editor.
pub(crate) struct InspectCx<'a> {
    pub time: Time,
    pub frame_rate: Time,
    pub inputs: &'a veylvik_render::ImageInputs,
    pub unavailable: &'a std::collections::BTreeMap<&'a str, &'a str>,
    /// The draft changed and needs Apply.
    pub dirty: bool,
    /// The change affects text or media layout and should refresh the text preview.
    pub text_changed: bool,
    /// The user asked to choose a new source file.
    pub replace_image: bool,
    /// Refresh the viewer preview soon, for continuous edits such as path dragging.
    pub preview_soon: bool,
    pub error: Option<String>,
    /// The colour field this node is currently sampling with the eyedropper, if any.
    pub picking: Option<&'static str>,
    /// An eyedropper request for this colour field.
    pub pick: Option<&'static str>,
}
pub(crate) trait Inspect {
    /// Node-specific controls above the registered parameters; most nodes have none.
    fn inspect(&mut self, _ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {}
}

mod color;
mod composite;
mod filters;
mod generators;
mod keying;
mod materials;
pub(crate) use materials::lighting_controls;
mod media;
mod text;
mod tracking;
mod vector;
