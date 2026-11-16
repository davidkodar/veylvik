//! Per-node inspector UI, grouped like `veylvik_document::nodes`. Each node edits its own
//! fields and reports what changed through `InspectCx`; it never sees the whole editor.
use eframe::egui;
use veylvik_document::Time;

/// What a node's inspector may read, and what it reports back to the editor.
pub(crate) struct InspectCx<'a> {
    pub node_id: veylvik_document::NodeId,
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
    /// The user asked to save this material to My materials.
    pub save_material: bool,
    /// The node's optional Environment input is connected.
    pub environment: bool,
    /// The node's optional Texture input is connected.
    pub textured: bool,
    /// Connect (true) or disconnect (false) the shot as the material's surroundings.
    pub reflect_shot: Option<bool>,
    /// Label of the Light rig connected to the node's Lights input.
    pub lit_by: Option<&'a str>,
    /// Move the material's lights into a shared rig (true) or go back to its own (false).
    pub share_lights: Option<bool>,
    /// Light this rig from the shot behind it.
    pub match_shot: bool,
    /// The node's optional Track input is connected (a Light rig on a planar track).
    pub tracked: bool,
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
pub(crate) use color::ocio_controls;
mod composite;
mod filters;
mod generators;
mod keying;
mod materials;
pub(crate) use materials::{finish_controls, lighting_controls, look_slider};
mod media;
mod text;
mod tracking;
mod vector;
