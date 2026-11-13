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

/// Primary grading of scene-linear colour; defaults are an exact identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorCorrect {
    pub temperature: Scalar,
    pub tint: Scalar,
    pub lift: Scalar,
    pub gain: Scalar,
    pub contrast: Scalar,
    pub gamma: Scalar,
    pub saturation: Scalar,
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

pub static COLOR_CORRECT: NodeType = NodeType {
    type_id: "color_correct",
    version: 1,
    label: "Colour correct",
    description: "Grade scene-linear colour in order: white balance (temperature warms or cools, tint shifts magenta or green), lift and gain (offset and multiply), contrast around 18% grey, gamma, then saturation with Rec.709 luminance. Works on unpremultiplied colour, keeps alpha, and does not clamp HDR values; negative values keep their sign through contrast and gamma. Defaults leave the image unchanged.",
    inputs: &["image"],
    parameters: &[
        scalar("temperature", "amount", -1.0, 1.0, 0.0),
        scalar("tint", "amount", -1.0, 1.0, 0.0),
        scalar("lift", "offset", -1.0, 1.0, 0.0),
        scalar("gain", "factor", 0.0, 16.0, 1.0),
        scalar("contrast", "factor", 0.0, 4.0, 1.0),
        scalar("gamma", "factor", 0.1, 10.0, 1.0),
        scalar("saturation", "factor", 0.0, 4.0, 1.0),
    ],
};

node!(Exposure, EXPOSURE, [stops]);
node!(
    ColorCorrect,
    COLOR_CORRECT,
    [temperature, tint, lift, gain, contrast, gamma, saturation]
);
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
