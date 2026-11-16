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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub position: Track3,
    pub rotation: Track3,
    pub scale: Track3,
}
impl Default for Placement {
    fn default() -> Self {
        Self {
            position: [0.0; 3].into(),
            rotation: [0.0; 3].into(),
            scale: [1.0; 3].into(),
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
            "position":track::schema(-1.0e6,1.0e6),"rotation":track::schema(-1.0e5,1.0e5),"scale":track::schema(-1000.0,1000.0)}})
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
}
impl ShapeKind {
    pub const ALL: [&'static str; 6] = ["cube", "sphere", "plane", "cylinder", "cone", "torus"];
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    #[default]
    Point,
    Spot,
    Sun,
}
impl LightKind {
    pub const ALL: [&'static str; 3] = ["point", "spot", "sun"];
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
    /// A library material shading the surface; colour, roughness and metallic then
    /// do not apply.
    #[serde(default)]
    pub look: Option<Box<crate::nodes::Material>>,
    /// Invisible except for the shadows that fall on it: lay one on the ground of
    /// the footage so 3D objects shadow the real floor.
    #[serde(default)]
    pub shadow_catcher: bool,
}
pub static SHAPE_3D: NodeType = NodeType {
    type_id: "shape_3d",
    version: 1,
    label: "3D shape",
    description: "A cube, sphere, plane, cylinder, cone or torus in a 3D scene, sized in pixels and centred on the frame at depth 0. Detail sets how many segments curved shapes use (3–256). Placement offsets, rotates (degrees, X then Y then Z) and scales it; colour (linear RGB), roughness and metallic shade it, unless a Look (any library material) is set, which shades it like a title. As a Shadow catcher it is invisible except for the shadows falling on it: lay a plane on the ground of the footage so 3D objects shadow the real floor. Outputs a scene for Merge scenes, 3D transform or Render scene.",
    inputs: &[],
    optional: &[],
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
        other("look", ParameterKind::Look, "library material"),
        other("shadow_catcher", ParameterKind::Boolean, "shadows only"),
    ],
};
node!(
    Shape3d,
    SHAPE_3D,
    [size_x, size_y, size_z, detail, roughness, metallic],
    {
        fn validate(&self) -> Result<()> {
            self.placement.validate()?;
            validate_look(&self.look)?;
            validate_colour(&self.colour)
        }
    }
);

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
    /// A library material shading the surface; colour, roughness and metallic then
    /// do not apply.
    #[serde(default)]
    pub look: Option<Box<crate::nodes::Material>>,
}
pub static TEXT_3D: NodeType = NodeType {
    type_id: "text_3d",
    version: 1,
    label: "3D text",
    description: "Extrudes the Text or Path node on its Outline input into a solid Depth pixels deep, front face where the 2D layer sits, centred in depth, with a Bevel (pixels, rounded or chamfered) on its front and back edges so metals catch highlights. Placement turns and moves it about its own centre; colour (linear RGB), roughness and metallic shade it, unless a Look (any library material) is set, which shades it like the 2D title. Outputs a scene.",
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
        other("look", ParameterKind::Look, "library material"),
    ],
};
node!(Text3d, TEXT_3D, [depth, bevel, roughness, metallic], {
    fn validate(&self) -> Result<()> {
        self.placement.validate()?;
        validate_look(&self.look)?;
        validate_colour(&self.colour)
    }
});

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
}
pub static CAMERA_3D: NodeType = NodeType {
    type_id: "camera_3d",
    version: 1,
    label: "Camera",
    description: "A perspective camera. Untouched, it frames the composition exactly at depth 0, at the distance its focal length and sensor width (millimetres, horizontal) imply, so 3D layers there match 2D. Placement moves and turns it from that home view; keys animate it. Outputs a scene to merge with the objects it films.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("placement", ParameterKind::Placement, "transform"),
        scalar("focal_length", "millimetres", 1.0, 2000.0, 50.0),
        scalar("sensor_width", "millimetres", 1.0, 100.0, 36.0),
    ],
};
node!(Camera3d, CAMERA_3D, [focal_length, sensor_width], {
    fn validate(&self) -> Result<()> {
        self.placement.validate()
    }
});

/// A point, spot or sun light.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Light3d {
    pub kind: LightKind,
    /// Casts shadows (sun, spot and point lights; up to four per scene).
    #[serde(default = "yes")]
    pub shadows: bool,
    #[serde(default)]
    pub placement: Placement,
    pub colour: Track3,
    pub intensity: Scalar,
    pub cone: Scalar,
    pub softness: Scalar,
}
pub static LIGHT_3D: NodeType = NodeType {
    type_id: "light_3d",
    version: 1,
    label: "Light",
    description: "A point light, a spot (cone in degrees, softness of its edge) or a sun (parallel light; only its rotation matters). Lights sit at the frame centre plus their placement and point along -z (towards the scene) before rotation. Colour is linear RGB; intensity is relative to the studio. With Shadows on (the default) it casts shadows from a shadow map, softened by Softness; up to four lights per scene cast them. Outputs a scene.",
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
    ],
};
node!(Light3d, LIGHT_3D, [intensity, cone, softness], {
    fn validate(&self) -> Result<()> {
        self.placement.validate()?;
        validate_colour(&self.colour)
    }
});

/// What surrounds a scene: lighting, reflections and an optional backdrop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment3d {
    pub intensity: Scalar,
    pub rotation: Scalar,
    pub background: bool,
    pub blur: Scalar,
}
pub static ENVIRONMENT_3D: NodeType = NodeType {
    type_id: "environment_3d",
    version: 1,
    label: "Environment",
    description: "Lights a 3D scene and fills its reflections from all around: the image on the optional Image input as a panorama (equirectangular, for example an HDR or EXR sky; any image or footage works), or the built-in studio without one. Intensity scales it and Rotation turns it (degrees about the vertical). With Background on it also fills the sky behind the objects, softened by Blur. The first Environment in a scene is used.",
    inputs: &["image"],
    optional: &["image"],
    output: PortKind::Scene,
    parameters: &[
        scalar("intensity", "factor", 0.0, 64.0, 1.0),
        scalar("rotation", "degrees", -3600.0, 3600.0, 0.0),
        other("background", ParameterKind::Boolean, "show as backdrop"),
        scalar("blur", "factor", 0.0, 1.0, 0.0),
    ],
};
node!(Environment3d, ENVIRONMENT_3D, [intensity, rotation, blur]);

/// Renders a scene back into the 2D graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderScene {
    pub camera: Scalar,
    /// How much nearby surfaces shade the environment's light (contact shadows).
    #[serde(default = "one")]
    pub occlusion: Scalar,
}
pub static RENDER_SCENE: NodeType = NodeType {
    type_id: "render_scene",
    version: 1,
    label: "Render scene",
    description: "Renders the scene on its Scene input as an image at the composition size, with transparent background. Camera picks which camera films it (1 = the first, in node order; 0 = the home view that matches 2D). Occlusion sets how much nearby surfaces shade the environment's light, so objects sit on the floor and gaps darken (0 = off).",
    inputs: &["scene"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("camera", "index", 0.0, 64.0, 1.0),
        scalar("occlusion", "factor", 0.0, 1.0, 1.0),
    ],
};
node!(RenderScene, RENDER_SCENE, [camera, occlusion]);
