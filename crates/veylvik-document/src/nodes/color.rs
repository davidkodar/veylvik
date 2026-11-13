//! Colour adjustments applied per pixel to one image.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exposure {
    pub stops: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colorize {
    pub low: [f32; 4],
    pub high: [f32; 4],
}

pub static EXPOSURE: NodeType = NodeType {
    type_id: "exposure",
    version: 1,
    label: "Exposure",
    description: "Multiply RGB by two to the power of stops; preserve alpha.",
    inputs: &["image"],
    parameters: &[scalar("stops", "stops", -32.0, 32.0, 0.0)],
};
pub static COLORIZE: NodeType = NodeType {
    type_id: "colorize",
    version: 1,
    label: "Colorize",
    description: "Map clamped straight Rec.709 luminance to two scene-linear RGBA colors. Interpolate premultiplied endpoints, then multiply by input coverage. Transparent input stays transparent; HDR and negative endpoint colors are preserved.",
    inputs: &["image"],
    parameters: &[
        other("low", ParameterKind::Color, "linear RGBA"),
        Parameter {
            default: 1.0,
            ..other("high", ParameterKind::Color, "linear RGBA")
        },
    ],
};

node!(Exposure, EXPOSURE, [stops]);
node!(Colorize, COLORIZE, [], {
    fn validate(&self) -> Result<()> {
        if [self.low, self.high]
            .iter()
            .any(|color| color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]))
        {
            return fail("colorize needs finite RGB and alpha in [0,1] for both endpoints");
        }
        Ok(())
    }
});
