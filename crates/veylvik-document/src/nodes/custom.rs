//! A node whose effect is a WGSL function written by the user.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// A custom shader: `code` defines `shade`, and declares its controls in comments;
/// the sixteen values are those controls' slots, in the order declared. The second
/// eight came later: a file without them reads them as 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shader {
    pub code: String,
    pub p1: Scalar,
    pub p2: Scalar,
    pub p3: Scalar,
    pub p4: Scalar,
    pub p5: Scalar,
    pub p6: Scalar,
    pub p7: Scalar,
    pub p8: Scalar,
    #[serde(default = "zero")]
    pub p9: Scalar,
    #[serde(default = "zero")]
    pub p10: Scalar,
    #[serde(default = "zero")]
    pub p11: Scalar,
    #[serde(default = "zero")]
    pub p12: Scalar,
    #[serde(default = "zero")]
    pub p13: Scalar,
    #[serde(default = "zero")]
    pub p14: Scalar,
    #[serde(default = "zero")]
    pub p15: Scalar,
    #[serde(default = "zero")]
    pub p16: Scalar,
    /// A 3D object of the scene on the Scene input: the shader is told where it is
    /// in the frame (`follow`, in pixels) through that scene's first camera.
    #[serde(default)]
    pub follow: Option<NodeId>,
}
fn zero() -> Scalar {
    0.0.into()
}
const fn slot(name: &'static str, default: f32) -> Parameter {
    scalar(name, "control", -1.0e6, 1.0e6, default)
}
pub static SHADER: NodeType = NodeType {
    type_id: "shader",
    version: 1,
    label: "Custom shader",
    description: "Runs your own WGSL on every pixel. code defines fn shade(at: vec2<f32>) -> vec4<f32>, returning premultiplied scene-linear RGBA for the pixel centred at `at` (pixels, y down). Read the inputs with image(at) and second(at) (bilinear, transparent outside the frame or when unconnected); size is the frame's size in pixels and time the frame's time in seconds. Controls are declared in comments and read from u: `// slider NAME MIN MAX DEFAULT` gives u.NAME (f32) and `// colour NAME R G B` gives u.NAME (vec3, linear); their values are p1 to p16 in the order declared, a slider using one and a colour three, and each can be keyed or driven by an expression. To be safe to run on every pixel the code may not use attributes, global variables, while or loop; for loops are allowed only inside shade, written as for (var i = 0; i < 16; i++) with whole-number bounds, at most 1024 iterations each and when nested, and eight loops in all; the source is at most 16000 characters. The code is compiled and validated before it runs, and errors name the line. To make an effect follow something in 3D, connect a scene to the Scene input and set follow to one of its objects: the shader then reads follow (vec2, where that object is in the frame in pixels, through the scene's first camera) and followed (1 when it is in front of the camera, -1 when it is behind it or not in the scene, 0 when nothing is followed).",
    inputs: &["image", "second", "scene"],
    optional: &["image", "second", "scene"],
    output: PortKind::Image,
    parameters: &[
        other("code", ParameterKind::ShaderCode, "WGSL"),
        slot("p1", veylvik_shader::EXAMPLE_DEFAULTS[0]),
        slot("p2", veylvik_shader::EXAMPLE_DEFAULTS[1]),
        slot("p3", veylvik_shader::EXAMPLE_DEFAULTS[2]),
        slot("p4", veylvik_shader::EXAMPLE_DEFAULTS[3]),
        slot("p5", veylvik_shader::EXAMPLE_DEFAULTS[4]),
        slot("p6", veylvik_shader::EXAMPLE_DEFAULTS[5]),
        slot("p7", veylvik_shader::EXAMPLE_DEFAULTS[6]),
        slot("p8", veylvik_shader::EXAMPLE_DEFAULTS[7]),
        slot("p9", veylvik_shader::EXAMPLE_DEFAULTS[8]),
        slot("p10", veylvik_shader::EXAMPLE_DEFAULTS[9]),
        slot("p11", veylvik_shader::EXAMPLE_DEFAULTS[10]),
        slot("p12", veylvik_shader::EXAMPLE_DEFAULTS[11]),
        slot("p13", veylvik_shader::EXAMPLE_DEFAULTS[12]),
        slot("p14", veylvik_shader::EXAMPLE_DEFAULTS[13]),
        slot("p15", veylvik_shader::EXAMPLE_DEFAULTS[14]),
        slot("p16", veylvik_shader::EXAMPLE_DEFAULTS[15]),
        other("follow", ParameterKind::NodeRef, "scene object followed"),
    ],
};
node!(
    Shader,
    SHADER,
    [
        p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16
    ],
    {
        fn validate(&self) -> Result<()> {
            veylvik_shader::check(&self.code)
                .map(|_| ())
                .map_err(|e| Error(format!("Custom shader: {e}")))
        }
    }
);
