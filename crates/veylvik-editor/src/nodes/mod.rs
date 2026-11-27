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
    pub unavailable: &'a std::collections::BTreeMap<String, String>,
    /// The node's connected inputs, by port.
    pub wired: &'a std::collections::BTreeMap<String, veylvik_document::NodeId>,
    /// The composition's frame size.
    pub frame: [u32; 2],
    /// The draft changed and needs Apply.
    pub dirty: bool,
    /// The change affects text or media layout and should refresh the text preview.
    pub text_changed: bool,
    /// The node's optional Environment input is connected.
    pub environment: bool,
    /// The node's optional Texture input is connected.
    pub textured: bool,
    /// The node's optional Normal map input is connected.
    pub normal_mapped: bool,
    /// Label of the Light rig connected to the node's Lights input.
    pub lit_by: Option<&'a str>,
    /// The node's optional Track input is connected (a Light rig on a planar track).
    pub tracked: bool,
    /// Refresh the viewer preview soon, for continuous edits such as path dragging.
    pub preview_soon: bool,
    pub error: Option<String>,
    /// What the controls asked the editor to do, in the order asked.
    pub requests: Vec<Request>,
    /// A flock's bake, in a line (None: not a flock).
    pub flock_bake: Option<&'a str>,
    /// The colour field this node is currently sampling with the eyedropper, if any.
    pub picking: Option<&'static str>,
    /// The 3D objects in the composition, for a camera's Focus on.
    pub objects: &'a [(veylvik_document::NodeId, String)],
    /// The particle systems in the composition, for another's Shaded by.
    pub systems: &'a [(veylvik_document::NodeId, String)],
    /// Everything in the scene an object can face: objects, cameras and lights.
    pub things: &'a [(veylvik_document::NodeId, String)],
    /// What a camera can keep in focus: objects, markers and 3D transforms.
    pub targets: &'a [(veylvik_document::NodeId, String)],
    /// The node's Plate input is connected.
    pub plated: bool,
    /// The node on this node's Outline input (the words of a 3D text).
    pub outline: Option<veylvik_document::NodeId>,
}
impl InspectCx<'_> {
    /// Whether `port` is connected.
    pub fn is_wired(&self, port: &str) -> bool {
        self.wired.contains_key(port)
    }
    /// Ask the editor to do something once the controls are drawn.
    pub fn ask(&mut self, request: Request) {
        self.requests.push(request);
    }
}
/// Something a node's controls ask of the editor, beyond changing the node's own settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Request {
    /// Choose a new source file (footage, image, LUT).
    ReplaceImage,
    /// Keep this material in My materials.
    SaveMaterial,
    /// Connect (true) or disconnect (false) the shot as the material's surroundings.
    ReflectShot(bool),
    /// Move the material's lights into a shared rig (true) or go back to its own (false).
    ShareLights(bool),
    /// Light this rig from the shot behind it.
    MatchShot,
    ChooseSplats,
    ChooseModel,
    /// Bake the flock (true) or go back to simulating it (false).
    BakeFlock(bool),
    /// Start (or cancel) the eyedropper for this colour field.
    Pick(&'static str),
    /// Pick the object in focus in the viewer.
    PickFocus,
    /// Add surround lighting to this Render scene, warm or neutral.
    SurroundLights {
        warm: bool,
    },
    /// Measure the plate's grain.
    MeasureGrain,
    /// Select this other node instead.
    Select(veylvik_document::NodeId),
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
mod hidden;
pub(crate) use hidden::hidden;
mod keying;
mod materials;
pub(crate) use materials::{colour_controls, finish_controls, lighting_controls, look_slider};
mod camera_orbit;
mod media;
mod scene;
mod text;
mod tracking;
mod vector;
