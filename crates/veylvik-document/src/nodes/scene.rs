//! 3D scene nodes: objects, structure, camera and lights carried on Scene ports, and the render back to an image.
//!
//! Scene units are composition pixels with x right, y down and z towards the viewer.
//! Positions are offsets from where an object naturally sits (the frame centre at
//! depth 0, or a title's own place), so a fresh object lines up with the 2D layer.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Where an object sits: position offset (pixels), rotation (degrees about X, then Y,
/// then Z) and scale, each keyable as a whole vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub position: Track3,
    pub rotation: Track3,
    pub scale: Track3,
    /// Whether the thing is there at all: below 0.5 it is left out of the scene
    /// (a 3D transform leaves out everything in it). Keyed with step keys it
    /// makes an object appear or vanish on a frame.
    #[serde(default = "one", skip_serializing_if = "always_shown")]
    pub shown: Scalar,
}
impl PartialEq for Placement {
    fn eq(&self, other: &Self) -> bool {
        // A Scalar has no equality of its own: its written form is compared.
        self.position == other.position
            && self.rotation == other.rotation
            && self.scale == other.scale
            && serde_json::to_value(&self.shown).ok() == serde_json::to_value(&other.shown).ok()
    }
}
fn always_shown(shown: &Scalar) -> bool {
    matches!(shown, Scalar::Constant { value } if *value == 1.0)
}
impl Default for Placement {
    fn default() -> Self {
        Self {
            position: [0.0; 3].into(),
            rotation: [0.0; 3].into(),
            scale: [1.0; 3].into(),
            shown: 1.0.into(),
        }
    }
}
impl Placement {
    pub fn validate(&self) -> Result<()> {
        for (track, lo, hi, what) in [
            (&self.position, -1.0e6, 1.0e6, "position"),
            (&self.rotation, -1.0e5, 1.0e5, "rotation"),
            (&self.scale, -1000.0, 1000.0, "scale"),
        ] {
            track.validate()?;
            if !track::values(track).all(|v| v.iter().all(|x| (lo..=hi).contains(x))) {
                return fail(format!("3D {what} is out of range"));
            }
        }
        Ok(())
    }
    pub fn schema() -> serde_json::Value {
        serde_json::json!({"type":"object","additionalProperties":false,"required":["position","rotation","scale"],"properties":{
            "position":track::schema(-1.0e6,1.0e6),"rotation":track::schema(-1.0e5,1.0e5),"scale":track::schema(-1000.0,1000.0),
            "shown":crate::registry::scalar_schema(Some(0.0),Some(1.0))},
            "description":"shown (0 or 1, default 1; keyable, use step keys) leaves the thing out of the scene below 0.5"})
    }
}
fn validate_colour(colour: &Track3) -> Result<()> {
    colour.validate()?;
    if !track::values(colour).all(|v| v.iter().all(|x| (0.0..=1.0).contains(x))) {
        return fail("3D colours are linear RGB 0–1");
    }
    Ok(())
}
/// A look is a whole Text surface definition, checked like one.
fn validate_look(look: &Option<Box<crate::nodes::Material>>) -> Result<()> {
    if let Some(look) = look {
        Operation::Material((**look).clone()).validate()?;
    }
    Ok(())
}
fn ten() -> Scalar {
    10.0.into()
}
fn one() -> Scalar {
    1.0.into()
}
fn zero() -> Scalar {
    0.0.into()
}
fn yes() -> bool {
    true
}
fn grey() -> Track3 {
    [0.6, 0.6, 0.6].into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    #[default]
    Cube,
    Sphere,
    Plane,
    Cylinder,
    Cone,
    Torus,
    /// A flat ring facing +z: size x and y are its outer diameters, size z its
    /// width from the rim inwards (half the diameter or more makes a disc).
    Ring,
}
impl ShapeKind {
    pub const ALL: [&'static str; 7] = [
        "cube", "sphere", "plane", "cylinder", "cone", "torus", "ring",
    ];
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    #[default]
    Point,
    Spot,
    Sun,
    /// A glowing rectangular panel: a soft box or strip light.
    Area,
}
impl LightKind {
    pub const ALL: [&'static str; 4] = ["point", "spot", "sun", "area"];
}

/// A primitive solid centred on the frame at depth 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape3d {
    pub kind: ShapeKind,
    pub size_x: Scalar,
    pub size_y: Scalar,
    pub size_z: Scalar,
    pub detail: Scalar,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default = "grey")]
    pub colour: Track3,
    pub roughness: Scalar,
    pub metallic: Scalar,
    /// Brushed finish: light scatters across the grain, stretching highlights and
    /// reflections that way (0 is an even polish).
    #[serde(default = "zero")]
    pub brushed: Scalar,
    /// The direction the grain runs, in degrees from the object's horizontal.
    #[serde(default = "zero")]
    pub brush_angle: Scalar,
    /// How strongly the grain shows as fine streaks of brighter and duller polish.
    #[serde(default = "zero")]
    pub grain: Scalar,
    /// Long shallow ridges along the grain, as a worked plate has: they tip parts of
    /// the surface towards a light and parts away, breaking reflections into bands.
    #[serde(default = "zero")]
    pub ridges: Scalar,
    /// Wear and handling: the polish varies in soft blotches, with faint scratches
    /// (0 is factory-perfect).
    #[serde(default = "zero")]
    pub imperfections: Scalar,
    /// A library material shading the surface; colour, roughness and metallic then
    /// do not apply.
    #[serde(default)]
    pub look: Option<Box<crate::nodes::Material>>,
    /// Invisible except for the shadows that fall on it: lay one on the ground of
    /// the footage so 3D objects shadow the real floor.
    #[serde(default)]
    pub shadow_catcher: bool,
    /// How dark a shadow catcher's shadows are (1 is their full darkness; real
    /// ground is rarely that dark, with light bouncing in from all round).
    #[serde(default = "one")]
    pub shadow_strength: Scalar,
}
pub static SHAPE_3D: NodeType = NodeType {
    type_id: "shape_3d",
    version: 1,
    label: "3D shape",
    description: "A cube, sphere, plane, cylinder, cone, torus or flat ring (size x and y its outer diameters, size z its width inwards; a planet's rings, or a disc when the width reaches the middle) in a 3D scene, sized in pixels and centred on the frame at depth 0. Detail sets how many segments curved shapes use (3–256). Placement offsets, rotates (degrees, X then Y then Z) and scales it; colour (linear RGB), roughness and metallic shade it, unless a Look (any library material) is set, which shades it like a title. As a Shadow catcher it is invisible except for the shadows falling on it: lay a plane on the ground of the footage so 3D objects shadow the real floor. Outputs a scene for Merge scenes, 3D transform or Render scene. Finish, each 0 to 1 and 0 by default: brushed scatters light across the grain so reflections stretch that way, brush_angle (degrees) turns the grain, grain shows it as fine streaks, ridges add long shallow undulations along the grain that break reflections into bands, imperfections add uneven polish and faint scratches; they shade the plain surface, not a look. Shadow strength (0 to 1) sets how dark a shadow catcher's shadows are. An image on the optional Texture input colours the surface, wrapped once round it (for a sphere, a map of the whole globe: left to right round the equator, top to bottom from pole to pole), multiplied by Colour; a library look takes its place.",
    inputs: &["texture"],
    optional: &["texture"],
    output: PortKind::Scene,
    parameters: &[
        other("kind", ParameterKind::ShapeKind, "shape"),
        scalar("size_x", "pixels", 0.0, 1.0e5, 300.0),
        scalar("size_y", "pixels", 0.0, 1.0e5, 300.0),
        scalar("size_z", "pixels", 0.0, 1.0e5, 300.0),
        scalar("detail", "segments", 3.0, 256.0, 48.0),
        other("placement", ParameterKind::Placement, "transform"),
        other("colour", ParameterKind::Colour3, "linear RGB"),
        scalar("roughness", "factor", 0.0, 1.0, 0.4),
        scalar("metallic", "factor", 0.0, 1.0, 0.0),
        scalar("brushed", "factor", 0.0, 1.0, 0.0),
        scalar("brush_angle", "degrees", -360.0, 360.0, 0.0),
        scalar("grain", "factor", 0.0, 1.0, 0.0),
        scalar("ridges", "factor", 0.0, 1.0, 0.0),
        scalar("imperfections", "factor", 0.0, 1.0, 0.0),
        other("look", ParameterKind::Look, "library material"),
        other("shadow_catcher", ParameterKind::Boolean, "shadows only"),
        scalar("shadow_strength", "factor", 0.0, 1.0, 1.0),
    ],
};
node!(
    Shape3d,
    SHAPE_3D,
    [
        size_x,
        size_y,
        size_z,
        detail,
        roughness,
        metallic,
        brushed,
        brush_angle,
        grain,
        ridges,
        imperfections,
        shadow_strength
    ],
    {
        fn validate(&self) -> Result<()> {
            self.placement.validate()?;
            validate_look(&self.look)?;
            validate_colour(&self.colour)
        }
    }
);

/// How a 3D text's pieces arrive, one after another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LetterMotion {
    /// The text is one solid.
    #[default]
    None,
    /// From `distance` pixels towards the viewer (or away, when negative).
    Fly,
    /// From `distance` pixels above (or below, when negative).
    Drop,
    /// Turning about its own upright axis from edge-on.
    Turn,
    /// Growing from nothing about its own centre.
    Grow,
}
impl LetterMotion {
    pub const ALL: [Self; 5] = [Self::None, Self::Fly, Self::Drop, Self::Turn, Self::Grow];
}
/// Per-piece 3D animation of a 3D text made from a Text node: each character,
/// word or line is its own solid and arrives on its own, `stagger` seconds after
/// the one before. Times are seconds on the timeline, as in a Text's animation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Letters {
    pub motion: LetterMotion,
    pub unit: crate::text::TextUnit,
    pub start: f32,
    pub duration: f32,
    pub stagger: f32,
    /// None holds the text; otherwise the pieces leave from this time, as they came.
    pub exit: Option<f32>,
    pub distance: f32,
    pub ease: bool,
}
impl Default for Letters {
    fn default() -> Self {
        Self {
            motion: LetterMotion::None,
            unit: crate::text::TextUnit::Character,
            start: 0.0,
            duration: 0.6,
            stagger: 0.06,
            exit: None,
            distance: 600.0,
            ease: true,
        }
    }
}
impl Letters {
    pub fn validate(&self) -> Result<()> {
        let time = |t: f32| t.is_finite() && t.abs() <= 86400.0;
        if !time(self.start)
            || !self.exit.is_none_or(time)
            || !(0.001..=3600.0).contains(&self.duration)
            || !(0.0..=60.0).contains(&self.stagger)
            || !self.distance.is_finite()
            || self.distance.abs() > 1.0e5
        {
            return fail(
                "letters: start and exit within a day, duration 0.001..3600 s, stagger 0..60 s, distance within 100000 pixels",
            );
        }
        Ok(())
    }
    /// How far piece `unit` has arrived at `seconds`: 0 not yet, 1 in place.
    pub fn progress(&self, seconds: f64, unit: usize) -> f32 {
        if self.motion == LetterMotion::None {
            return 1.0;
        }
        let delay = unit as f64 * f64::from(self.stagger);
        let span = f64::from(self.duration);
        let arriving = ((seconds - f64::from(self.start) - delay) / span).clamp(0.0, 1.0) as f32;
        let leaving = self.exit.map_or(1.0, |at| {
            1.0 - ((seconds - f64::from(at) - delay) / span).clamp(0.0, 1.0) as f32
        });
        let p = arriving.min(leaving);
        if self.ease {
            p * p * (3.0 - 2.0 * p)
        } else {
            p
        }
    }
    pub fn schema() -> serde_json::Value {
        serde_json::json!({"type":"object","additionalProperties":false,"properties":{
            "motion":{"enum":LetterMotion::ALL},"unit":{"enum":["character","word","line","whole"]},
            "start":{"type":"number","minimum":-86400,"maximum":86400},"duration":{"type":"number","minimum":0.001,"maximum":3600},
            "stagger":{"type":"number","minimum":0,"maximum":60},"exit":{"anyOf":[{"type":"null"},{"type":"number","minimum":-86400,"maximum":86400}]},
            "distance":{"type":"number","minimum":-100000,"maximum":100000},"ease":{"type":"boolean"}},
            "description":"Per-piece 3D animation: motion none, fly (from distance pixels towards the viewer), drop (from distance pixels above), turn (about its upright axis from edge-on) or grow; every other field has a default. Times in seconds."})
    }
}
/// A title (or any vector path) extruded into a solid where the 2D layer sits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text3d {
    pub depth: Scalar,
    /// Rounded or chamfered front and back edges, in pixels (at most half the
    /// depth). Projects from before bevels keep square edges.
    #[serde(default = "zero")]
    pub bevel: Scalar,
    #[serde(default = "yes")]
    pub round_bevel: bool,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default = "grey")]
    pub colour: Track3,
    pub roughness: Scalar,
    pub metallic: Scalar,
    /// Brushed finish: light scatters across the grain, stretching highlights and
    /// reflections that way (0 is an even polish).
    #[serde(default = "zero")]
    pub brushed: Scalar,
    /// The direction the grain runs, in degrees from the object's horizontal.
    #[serde(default = "zero")]
    pub brush_angle: Scalar,
    /// How strongly the grain shows as fine streaks of brighter and duller polish.
    #[serde(default = "zero")]
    pub grain: Scalar,
    /// Long shallow ridges along the grain, as a worked plate has: they tip parts of
    /// the surface towards a light and parts away, breaking reflections into bands.
    #[serde(default = "zero")]
    pub ridges: Scalar,
    /// Wear and handling: the polish varies in soft blotches, with faint scratches
    /// (0 is factory-perfect).
    #[serde(default = "zero")]
    pub imperfections: Scalar,
    /// A library material shading the surface; colour, roughness and metallic then
    /// do not apply.
    #[serde(default)]
    pub look: Option<Box<crate::nodes::Material>>,
    /// The pieces of a Text arriving one by one in 3D; a Path outline stays whole.
    #[serde(default)]
    pub letters: Letters,
}
pub static TEXT_3D: NodeType = NodeType {
    type_id: "text_3d",
    version: 1,
    label: "3D text",
    description: "Extrudes the Text or Path node on its Outline input into a solid Depth pixels deep, front face where the 2D layer sits, centred in depth, with a Bevel (pixels, rounded or chamfered) on its front and back edges so metals catch highlights. Placement turns and moves it about its own centre; colour (linear RGB), roughness and metallic shade it, unless a Look (any library material) is set, which shades it like the 2D title. Outputs a scene. Finish, each 0 to 1 and 0 by default: brushed scatters light across the grain so reflections stretch that way, brush_angle (degrees) turns the grain, grain shows it as fine streaks, ridges add long shallow undulations along the grain that break reflections into bands, imperfections add uneven polish and faint scratches; they shade the plain surface, not a look. Letters brings a Text in piece by piece in 3D (motion fly, drop, turn or grow; unit character, word or line; start, duration and stagger in seconds).",
    inputs: &["outline"],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        scalar("depth", "pixels", 0.0, 1.0e4, 40.0),
        scalar("bevel", "pixels", 0.0, 500.0, 6.0),
        Parameter {
            default: 1.0,
            ..other(
                "round_bevel",
                ParameterKind::Boolean,
                "rounded or chamfered",
            )
        },
        other("placement", ParameterKind::Placement, "transform"),
        other("colour", ParameterKind::Colour3, "linear RGB"),
        scalar("roughness", "factor", 0.0, 1.0, 0.35),
        scalar("metallic", "factor", 0.0, 1.0, 0.0),
        scalar("brushed", "factor", 0.0, 1.0, 0.0),
        scalar("brush_angle", "degrees", -360.0, 360.0, 0.0),
        scalar("grain", "factor", 0.0, 1.0, 0.0),
        scalar("ridges", "factor", 0.0, 1.0, 0.0),
        scalar("imperfections", "factor", 0.0, 1.0, 0.0),
        other("look", ParameterKind::Look, "library material"),
        other("letters", ParameterKind::Letters, "per-letter animation"),
    ],
};
node!(
    Text3d,
    TEXT_3D,
    [
        depth,
        bevel,
        roughness,
        metallic,
        brushed,
        brush_angle,
        grain,
        ridges,
        imperfections
    ],
    {
        fn validate(&self) -> Result<()> {
            self.placement.validate()?;
            self.letters.validate()?;
            validate_look(&self.look)?;
            validate_colour(&self.colour)
        }
    }
);

/// Any image as a flat card in the scene: footage, stills, titles, whole comps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCard {
    #[serde(default)]
    pub placement: Placement,
}
pub static IMAGE_CARD: NodeType = NodeType {
    type_id: "image_card",
    version: 1,
    label: "Image card",
    description: "Puts the image on its Image input on a flat card in a 3D scene. The card covers the frame at depth 0, so an untouched card renders like the 2D image; Placement moves, turns and scales it about the frame centre. It shows the image's own colours, unlit, with its alpha. Outputs a scene.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[other("placement", ParameterKind::Placement, "transform")],
};
node!(ImageCard, IMAGE_CARD, [], {
    fn validate(&self) -> Result<()> {
        self.placement.validate()
    }
});

/// Moves, turns or scales a whole scene about the frame centre at depth 0.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform3d {
    #[serde(default)]
    pub placement: Placement,
}
pub static TRANSFORM_3D: NodeType = NodeType {
    type_id: "transform_3d",
    version: 1,
    label: "3D transform",
    description: "Moves, turns (degrees, X then Y then Z) and scales everything on its Scene input, including cameras and lights, about the frame centre at depth 0.",
    inputs: &["scene"],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[other("placement", ParameterKind::Placement, "transform")],
};
node!(Transform3d, TRANSFORM_3D, [], {
    fn validate(&self) -> Result<()> {
        self.placement.validate()
    }
});

/// Combines up to eight scenes into one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeScenes {}
pub const MERGE_PORTS: [&str; 8] = [
    "scene", "scene_2", "scene_3", "scene_4", "scene_5", "scene_6", "scene_7", "scene_8",
];
pub static MERGE_SCENES: NodeType = NodeType {
    type_id: "merge_scenes",
    version: 1,
    label: "Merge scenes",
    description: "Combines the objects, cameras and lights of up to eight scenes into one. Order does not matter: depth decides what is in front.",
    inputs: &MERGE_PORTS,
    optional: &[
        "scene_2", "scene_3", "scene_4", "scene_5", "scene_6", "scene_7", "scene_8",
    ],
    output: PortKind::Scene,
    parameters: &[],
};
node!(MergeScenes, MERGE_SCENES, []);

/// A camera; untouched it sees exactly the 2D frame at depth 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera3d {
    #[serde(default)]
    pub placement: Placement,
    pub focal_length: Scalar,
    pub sensor_width: Scalar,
    /// Depth of field: the lens's f-number (0 = everything sharp).
    #[serde(default = "zero")]
    pub f_stop: Scalar,
    /// Distance in focus, in scene units (0 = autofocus on the frame centre).
    #[serde(default = "zero")]
    pub focus_distance: Scalar,
    /// A 3D object kept in focus wherever it and the camera move; overrides Focus distance.
    #[serde(default)]
    pub focus_on: Option<NodeId>,
    /// A 3D object the camera keeps looking at, wherever both move; replaces the
    /// placement's rotation while the object is in the scene.
    #[serde(default)]
    pub aim_at: Option<NodeId>,
}
pub static CAMERA_3D: NodeType = NodeType {
    type_id: "camera_3d",
    version: 1,
    label: "Camera",
    description: "A perspective camera. Untouched, it frames the composition exactly at depth 0, at the distance its focal length and sensor width (millimetres, horizontal) imply, so 3D layers there match 2D. Placement moves and turns it from that home view; keys animate it. F-stop above 0 adds depth of field (lower = blurrier, like a real lens with one scene unit as a millimetre), focused on the Focus on object (kept sharp as it and the camera move), else at Focus distance or, at 0, on whatever is at the frame centre. Aim at names an object the camera keeps looking at from wherever it is placed, in place of its rotation. Outputs a scene to merge with the objects it films.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("placement", ParameterKind::Placement, "transform"),
        scalar("focal_length", "millimetres", 1.0, 2000.0, 50.0),
        scalar("sensor_width", "millimetres", 1.0, 100.0, 36.0),
        scalar("f_stop", "f-number", 0.0, 64.0, 0.0),
        scalar("focus_distance", "pixels", 0.0, 1_000_000.0, 0.0),
        other("focus_on", ParameterKind::NodeRef, "object in focus"),
        other("aim_at", ParameterKind::NodeRef, "object looked at"),
    ],
};
node!(
    Camera3d,
    CAMERA_3D,
    [focal_length, sensor_width, f_stop, focus_distance],
    {
        fn validate(&self) -> Result<()> {
            self.placement.validate()
        }
    }
);

/// A point, spot, sun or area light.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Light3d {
    pub kind: LightKind,
    /// Casts shadows (up to four lights per scene; area lights after the others).
    #[serde(default = "yes")]
    pub shadows: bool,
    #[serde(default)]
    pub placement: Placement,
    pub colour: Track3,
    pub intensity: Scalar,
    pub cone: Scalar,
    pub softness: Scalar,
    /// An area light's panel size in pixels.
    #[serde(default = "panel_width")]
    pub width: Scalar,
    #[serde(default = "panel_height")]
    pub height: Scalar,
    /// An area light split along its width into this many uneven strips (1 is one
    /// whole panel): a row of studio strip lights from one node.
    #[serde(default = "one_strip")]
    pub strips: Scalar,
    /// How uneven the strips are: their lengths, gaps, heights, tilt and brightness.
    #[serde(default = "zero")]
    pub variation: Scalar,
    /// Which arrangement of strips; the same seed always gives the same row.
    #[serde(default)]
    pub seed: u32,
    /// A 3D object the light keeps pointing at, wherever both move; replaces the
    /// placement's rotation while the object is in the scene.
    #[serde(default)]
    pub aim_at: Option<NodeId>,
}
fn one_strip() -> Scalar {
    1.0.into()
}
fn panel_width() -> Scalar {
    600.0.into()
}
fn panel_height() -> Scalar {
    150.0.into()
}
pub static LIGHT_3D: NodeType = NodeType {
    type_id: "light_3d",
    version: 1,
    label: "Light",
    description: "A point light, a spot (cone in degrees, softness of its edge), a sun (parallel light; only its rotation matters) or an area light: a glowing panel of width by height pixels facing along its direction, like a soft box or strip light. An area light near a surface lights it unevenly, brightest where it is close, and glossy surfaces show its reflection as a soft-edged shape whose blur follows the surface's roughness and brushing; its intensity is the panel's brightness, so a larger panel gives more light. With Strips above 1 the panel is split along its width into that many strip lights, a row of studio strips from one node; Variation (0 to 1) makes their lengths, gaps, heights, tilt and brightness uneven, as a hand-set studio is, and Seed picks the arrangement (the same seed always gives the same row). An area light casts a soft shadow from the middle of its panel, a row of strips as one light. Lights sit at the frame centre plus their placement and point along -z (towards the scene) before rotation; Aim at names an object the light keeps pointing at instead, from wherever it is placed. Colour is linear RGB; intensity is relative to the studio. With Shadows on (the default) it casts shadows from a shadow map, softened by Softness; up to four lights per scene cast them, sun, spot and point lights first. Outputs a scene.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("kind", ParameterKind::LightKind, "light"),
        Parameter {
            default: 1.0,
            ..other("shadows", ParameterKind::Boolean, "casts shadows")
        },
        other("placement", ParameterKind::Placement, "transform"),
        other("colour", ParameterKind::Colour3, "linear RGB"),
        scalar("intensity", "factor", 0.0, 1000.0, 1.0),
        scalar("cone", "degrees", 1.0, 179.0, 40.0),
        scalar("softness", "factor", 0.0, 1.0, 0.2),
        scalar("width", "pixels", 1.0, 100000.0, 600.0),
        scalar("height", "pixels", 1.0, 100000.0, 150.0),
        scalar("strips", "count", 1.0, 32.0, 1.0),
        scalar("variation", "factor", 0.0, 1.0, 0.0),
        other("seed", ParameterKind::Seed, "integer"),
        other("aim_at", ParameterKind::NodeRef, "object pointed at"),
    ],
};
node!(
    Light3d,
    LIGHT_3D,
    [intensity, cone, softness, width, height, strips, variation],
    {
        fn validate(&self) -> Result<()> {
            self.placement.validate()?;
            validate_colour(&self.colour)
        }
    }
);

/// What surrounds a scene: lighting, reflections and an optional backdrop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment3d {
    pub intensity: Scalar,
    pub rotation: Scalar,
    pub background: bool,
    pub blur: Scalar,
    /// The image is an ordinary frame of the shot, not a panorama: it surrounds
    /// the scene as the camera saw it.
    #[serde(default)]
    pub frame: bool,
}
pub static ENVIRONMENT_3D: NodeType = NodeType {
    type_id: "environment_3d",
    version: 1,
    label: "Environment",
    description: "Lights a 3D scene and fills its reflections from all around: the image on the optional Image input as a panorama (equirectangular, for example an HDR or EXR sky; any image or footage works), or the built-in studio without one. Intensity scales it and Rotation turns it (degrees about the vertical). With Background on it also fills the sky behind the objects, softened by Blur. With Frame on, the image is taken as an ordinary frame of the shot instead of a panorama: what is at the top of the picture lights from above, the bottom from below, its left and right from those sides and its middle from straight ahead, the same again behind, so footage gives 3D put into it the shot's own light. The first Environment in a scene is used.",
    inputs: &["image"],
    optional: &["image"],
    output: PortKind::Scene,
    parameters: &[
        scalar("intensity", "factor", 0.0, 64.0, 1.0),
        scalar("rotation", "degrees", -3600.0, 3600.0, 0.0),
        other("background", ParameterKind::Boolean, "show as backdrop"),
        scalar("blur", "factor", 0.0, 1.0, 0.0),
        other("frame", ParameterKind::Boolean, "a frame of the shot"),
    ],
};
node!(Environment3d, ENVIRONMENT_3D, [intensity, rotation, blur]);

/// An imported glTF model (.glb or .gltf).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model3d {
    /// The model file's absolute path; empty shows nothing.
    pub model: String,
    /// The model's largest side, in pixels.
    pub size: Scalar,
    /// The model's proportions (largest side 1), measured on import, so the scene
    /// knows its box without reading the file.
    pub extent: [f32; 3],
    #[serde(default)]
    pub placement: Placement,
    /// A library material instead of the model's own.
    #[serde(default)]
    pub look: Option<Box<crate::nodes::Material>>,
}
pub static MODEL_3D: NodeType = NodeType {
    type_id: "model_3d",
    version: 1,
    label: "3D model",
    description: "An imported glTF 2.0 model (.glb or .gltf with its files): every part with its own base colour, texture, metallic and roughness, centred on its box and scaled so its largest side is Size pixels. Placement moves, turns and scales it about the frame centre; a library Look replaces its materials. Outputs a scene.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("model", ParameterKind::FilePath, "glTF file"),
        scalar("size", "pixels", 1.0, 100_000.0, 400.0),
        other("extent", ParameterKind::Vector3, "proportions"),
        other("placement", ParameterKind::Placement, "transform"),
        other("look", ParameterKind::Look, "library material"),
    ],
};
node!(Model3d, MODEL_3D, [size], {
    fn validate(&self) -> Result<()> {
        if self.model.len() > 4096 || self.model.contains('\0') {
            return fail("The model path is too long or invalid");
        }
        if self
            .extent
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return fail("Model proportions must be 0 to 1");
        }
        validate_look(&self.look)?;
        self.placement.validate()
    }
});

/// Points in 3D, such as those a camera solve finds in footage: guides in the viewer for
/// placing objects, drawing nothing in renders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointCloud {
    #[serde(default)]
    pub points: Vec<[f32; 3]>,
}
pub const MAX_CLOUD_POINTS: usize = 20_000;
pub static POINT_CLOUD: NodeType = NodeType {
    type_id: "point_cloud",
    version: 1,
    label: "Point cloud",
    description: "Points in 3D, offsets from the frame centre at depth 0 like other 3D placements, such as the features a camera solve finds in the footage. They draw nothing in renders; the viewer shows them as guides to put objects on. At most 20000. Outputs a scene.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[other("points", ParameterKind::Points3, "3D points")],
};
node!(PointCloud, POINT_CLOUD, [], {
    fn validate(&self) -> Result<()> {
        if self.points.len() > MAX_CLOUD_POINTS {
            return fail("A point cloud holds at most 20000 points");
        }
        if self
            .points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1.0e7)
        {
            return fail("Point cloud points must be finite and within 10 million units");
        }
        Ok(())
    }
});

/// Renders a scene back into the 2D graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderScene {
    pub camera: Scalar,
    /// How much nearby surfaces shade the environment's light (contact shadows).
    #[serde(default = "one")]
    pub occlusion: Scalar,
    /// Shutter angle in degrees (0 = no motion blur, 180 = the film look).
    #[serde(default = "zero")]
    pub motion_blur: Scalar,
    /// Moments across the shutter that are rendered and averaged.
    #[serde(default = "ten")]
    pub blur_samples: Scalar,
    /// Output the scene's depth instead of its picture: each pixel's distance from
    /// the camera in scene units, opaque where an object is.
    #[serde(default)]
    pub depth: bool,
}
pub static RENDER_SCENE: NodeType = NodeType {
    type_id: "render_scene",
    version: 1,
    label: "Render scene",
    description: "Renders the scene on its Scene input as an image at the composition size, with transparent background. Camera picks which camera films it (1 = the first, in node order; 0 = the home view that matches 2D). Occlusion sets how much nearby surfaces shade the environment's light, so objects sit on the floor and gaps darken (0 = off). Motion blur is the shutter angle (180 = the film look): the scene is rendered at Blur samples moments across the shutter, centred on the frame, and averaged. With Depth on it outputs the scene's depth instead of its picture: every pixel's distance from the camera in scene units (red, green and blue alike), opaque where an object is and empty elsewhere, without lighting or lens blur. Connect that to the Depth input of Particles filmed by the same camera so objects hide the particles behind them, or to a Lens blur's depth input.",
    inputs: &["scene"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("camera", "index", 0.0, 64.0, 1.0),
        scalar("occlusion", "factor", 0.0, 1.0, 1.0),
        scalar("motion_blur", "degrees", 0.0, 360.0, 0.0),
        scalar("blur_samples", "count", 2.0, 32.0, 10.0),
        other("depth", ParameterKind::Boolean, "output depth"),
    ],
};
node!(
    RenderScene,
    RENDER_SCENE,
    [camera, occlusion, motion_blur, blur_samples]
);
