//! Camera projection: footage cast through a camera onto 3D objects, so a shot can be rebuilt in 3D and filmed again from elsewhere.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// The objects at `scene`, coloured by the picture at `image` as the camera at `camera`
/// saw it; a 3D scene part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraProjection {
    /// The projected picture takes the scene's light (as a surface colour), rather than
    /// showing as it is.
    #[serde(default)]
    pub lit: bool,
    /// The projecting camera stays where it was at hold_time, rather than moving.
    #[serde(default)]
    pub hold: bool,
    pub hold_time: Scalar,
    /// Parts of the objects the camera did not see are left out, rather than keeping
    /// their own surface.
    #[serde(default)]
    pub hide_unseen: bool,
}

pub static CAMERA_PROJECTION: NodeType = NodeType {
    type_id: "camera_projection",
    version: 1,
    label: "Camera projection",
    description: "Casts the picture connected to image through the camera connected to camera (a Camera node, such as a solved one) onto the shapes, 3D texts, image cards and models connected to scene, as a part of a 3D scene: each point of their surfaces takes the colour the camera saw there, so footage can be laid back onto rough geometry of the place it shows and filmed again by another camera (parallax for a still, set extension, a small camera move). Unlit, the picture shows on the surfaces as it is; lit, it becomes their colour and takes the scene's lights. With hold the projecting camera stays where it was at hold_time seconds while the picture is whatever image gives at each frame: connect a still or a held frame to project one moment. Surfaces the camera did not see (outside its view, turned away from it, or hidden behind other objects of the scene as it saw them) keep their own look, or with hide_unseen are left out. What hides what is found from a depth map made from the projecting camera, as for a light's shadow: a scene has four such maps, lights with shadows taking theirs first, so a fifth projecting camera (or one past three shadowing lights) casts through what is in front.",
    inputs: &["scene", "camera", "image"],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("lit", ParameterKind::Boolean, "lit by the scene"),
        other("hold", ParameterKind::Boolean, "camera held"),
        scalar("hold_time", "seconds", -3600.0, 36_000.0, 0.0),
        other(
            "hide_unseen",
            ParameterKind::Boolean,
            "leave out what the camera did not see",
        ),
    ],
};

node!(CameraProjection, CAMERA_PROJECTION, [hold_time]);
