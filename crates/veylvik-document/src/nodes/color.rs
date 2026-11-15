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
    optional: &[],
    parameters: &[scalar("stops", "stops", -32.0, 32.0, 0.0)],
};
pub static COLORIZE: NodeType = NodeType {
    type_id: "colorize",
    version: 1,
    label: "Colorize",
    description: "Map clamped straight Rec.709 luminance to two scene-linear RGBA colors. Interpolate premultiplied endpoints, then multiply by input coverage. Transparent input stays transparent; HDR and negative endpoint colors are preserved.",
    inputs: &["image"],
    optional: &[],
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
    optional: &[],
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

/// Applies a 3D `.cube` LUT, converting scene-linear colour into the LUT's input encoding and
/// its output back, so camera and creative LUTs work in a scene-linear graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lut {
    pub asset: String,
    pub mix: Scalar,
    /// What the LUT expects; Auto means Rec.709 video.
    pub input: media::InputColour,
    /// What the LUT produces; Auto means Rec.709 video.
    pub output: media::InputColour,
}
pub static LUT: NodeType = NodeType {
    type_id: "lut",
    version: 1,
    label: "LUT",
    description: "Apply a 3D .cube LUT (up to 65³, with DOMAIN_MIN/MAX) to scene-linear colour. Input names the encoding the LUT expects and Output what it produces, each a curve (rec709, srgb, linear, pq, hlg, apple_log, slog3, logc3, logc4, vlog) and gamut (rec709, rec2020, p3_d65, s_gamut3, s_gamut3_cine, arri_wide_gamut3, arri_wide_gamut4, v_gamut); auto means Rec.709. Colour is encoded into the input space, looked up with trilinear interpolation and decoded from the output space back to scene-linear Rec.709. A camera LUT such as S-Log3/S-Gamut3.Cine to Rec.709 takes input slog3 with s_gamut3_cine and output rec709; a creative look made for Rec.709 video takes rec709 both ways. Mix blends with the original. Works on unpremultiplied colour and keeps alpha.",
    inputs: &["image"],
    optional: &[],
    parameters: &[
        other("asset", ParameterKind::Asset, ".cube file"),
        scalar("mix", "amount", 0.0, 1.0, 1.0),
        other("input", ParameterKind::MediaColour, ""),
        other("output", ParameterKind::MediaColour, ""),
    ],
};
node!(Lut, LUT, [mix], {
    fn validate(&self) -> Result<()> {
        let asset = &self.asset;
        if asset.trim().is_empty()
            || asset.chars().count() > crate::registry::MAX_ASSET_NAME
            || asset.contains('\0')
        {
            return fail("LUT needs a .cube file");
        }
        Ok(())
    }
    fn asset(&self) -> Option<&str> {
        Some(&self.asset)
    }
    fn asset_mut(&mut self) -> Option<&mut String> {
        Some(&mut self.asset)
    }
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ocio {
    pub transform: crate::color_management::OcioTransform,
}
pub static OCIO: NodeType = NodeType {
    type_id: "ocio",
    version: 1,
    label: "OCIO colour transform",
    description: "Full-float GPU OpenColorIO transform, including config LUTs. Explicit source/destination or display/view. Unpremultiplies before conversion and restores alpha. Colour-space names describe these pixels; connect transforms deliberately and avoid applying a display transform twice.",
    inputs: &["image"],
    optional: &[],
    parameters: &[other("transform", ParameterKind::OcioTransform, "")],
};
node!(Ocio, OCIO, [], {
    fn validate(&self) -> Result<()> {
        self.transform.validate()
    }
});
