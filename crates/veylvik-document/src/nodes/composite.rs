//! Layering, blending and mask combination of two or more images.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Merge {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blend {
    pub mode: BlendMode,
    pub opacity: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mask {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombineMask {
    pub mode: MaskMode,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelMask {
    pub channel: MaskChannel,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBlend {
    pub opacity: Scalar,
    pub lighting: Scalar,
    pub detail: Scalar,
    pub wear: Scalar,
    pub threshold: Scalar,
    pub distortion: Scalar,
    pub softness: Scalar,
    pub grain: Scalar,
}

/// Shows its layer with an opacity, only while it is on screen, fading in and out.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    pub opacity: Scalar,
    pub timing: FadeTiming,
}
/// When a faded layer is on screen: from `start` until `end` (or the end of the
/// composition when `None`), ramping linearly up over `fade_in` and down over `fade_out`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FadeTiming {
    pub start: Time,
    pub end: Option<Time>,
    pub fade_in: Time,
    pub fade_out: Time,
}
impl Default for FadeTiming {
    fn default() -> Self {
        let zero = Time { ticks: 0, rate: 1 };
        Self {
            start: zero,
            end: None,
            fade_in: zero,
            fade_out: zero,
        }
    }
}
impl FadeTiming {
    pub fn validate(&self) -> Result<()> {
        let [start, fade_in, fade_out] =
            [self.start, self.fade_in, self.fade_out].map(|t| t.seconds());
        let (start, fade_in, fade_out) = (start?, fade_in?, fade_out?);
        if start < 0. || fade_in < 0. || fade_out < 0. {
            return fail("fade times must not be negative");
        }
        if let Some(end) = self.end {
            let end = end.seconds()?;
            if end <= start {
                return fail("a fade must end after it starts");
            }
            if fade_in + fade_out > end - start + 1e-9 {
                return fail("fade in and fade out must fit between start and end");
            }
        }
        Ok(())
    }
    /// Visibility in 0..=1 at `time`: zero before the start and from the end on.
    pub fn envelope(&self, time: Time) -> Result<f64> {
        let t = time.seconds()?;
        let start = self.start.seconds()?;
        if t < start {
            return Ok(0.);
        }
        let fade_in = self.fade_in.seconds()?;
        let mut level = if fade_in > 0. {
            ((t - start) / fade_in).min(1.)
        } else {
            1.
        };
        if let Some(end) = self.end {
            let end = end.seconds()?;
            if t >= end {
                return Ok(0.);
            }
            let fade_out = self.fade_out.seconds()?;
            if fade_out > 0. {
                level = level.min((end - t) / fade_out);
            }
        }
        Ok(level.clamp(0., 1.))
    }
    /// Fully visible at every time, so the timing changes nothing.
    pub fn always_visible(&self) -> bool {
        self.start.ticks == 0 && self.end.is_none() && self.fade_in.ticks == 0
    }
}
pub fn fade_schema() -> serde_json::Value {
    let time = serde_json::json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":0},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false});
    serde_json::json!({"type":"object","properties":{"start":time,"end":{"anyOf":[time,{"type":"null"}]},"fade_in":time,"fade_out":time},"required":["start","end","fade_in","fade_out"],"additionalProperties":false})
}

pub static FADE: NodeType = NodeType {
    type_id: "fade",
    version: 1,
    label: "Fade",
    description: "Show a layer only while it is on screen: from Start until End (or the end of the composition), fading linearly in and out over the given lengths, multiplied by an animatable Opacity. Premultiplied colour and alpha scale together. A layer faded fully out is not rendered or decoded.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("opacity", "factor", 0.0, 1.0, 1.0),
        other("timing", ParameterKind::FadeTiming, "seconds"),
    ],
};
pub static MERGE: NodeType = NodeType {
    type_id: "merge",
    version: 1,
    label: "Merge",
    description: "Premultiplied foreground over background.",
    inputs: &["background", "foreground"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[],
};
pub static BLEND: NodeType = NodeType {
    type_id: "blend",
    version: 1,
    label: "Blend",
    description: "Scene-linear blending with source-over alpha and animated foreground opacity. RGB is not clamped; Add sums premultiplied RGB with union alpha. Overlay and Soft light let the background's texture show through the foreground (paint on a wall, a logo on cloth): they work on perceptual values of colours within 0..1, so brighter HDR values are held at 1 where the two overlap.",
    inputs: &["background", "foreground"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        other("mode", ParameterKind::BlendMode, "blend mode"),
        scalar("opacity", "factor", 0.0, 1.0, 1.0),
    ],
};
pub static MASK: NodeType = NodeType {
    type_id: "mask",
    version: 1,
    label: "Apply mask",
    description: "Multiply all premultiplied image channels by mask alpha; mask RGB is ignored.",
    inputs: &["image", "mask"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[],
};
pub static COMBINE_MASK: NodeType = NodeType {
    type_id: "combine_mask",
    version: 1,
    label: "Combine masks",
    description: "Combine input alpha into a white premultiplied mask. For alpha a from mask and b from other: union a+b-a*b, intersect a*b, subtract a*(1-b), exclude a*(1-b)+b*(1-a). Input RGB is ignored; soft edges remain fractional.",
    inputs: &["mask", "other"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[other("mode", ParameterKind::MaskMode, "coverage operation")],
};
pub static CHANNEL_MASK: NodeType = NodeType {
    type_id: "channel_mask",
    version: 1,
    label: "Channel mask",
    description: "Convert premultiplied red, green, blue, alpha or linear Rec.709 luminance to white mask coverage. Coverage clamps to [0, source alpha], preserving transparent boundaries without division by alpha.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[other("channel", ParameterKind::MaskChannel, "channel")],
};
pub static SURFACE_BLEND: NodeType = NodeType {
    type_id: "surface_blend",
    version: 1,
    label: "Surface blend",
    description: "Composite a premultiplied layer onto a surface using its local illumination, fine luminance detail, and dark recesses. Optional gradient displacement, edge softness and temporal grain run on GPU. This is image-based blending, not automatic object segmentation.",
    inputs: &["background", "foreground"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("opacity", "factor", 0.0, 1.0, 1.0),
        scalar("lighting", "amount", 0.0, 1.0, 0.35),
        scalar("detail", "amount", 0.0, 1.0, 0.5),
        scalar("wear", "amount", 0.0, 1.0, 0.0),
        scalar("threshold", "contrast", 0.0, 1.0, 0.3),
        scalar("distortion", "pixels", 0.0, 16.0, 0.0),
        scalar("softness", "pixels", 0.0, 8.0, 0.0),
        scalar("grain", "amount", 0.0, 0.25, 0.0),
    ],
};

node!(Merge, MERGE, []);
node!(Blend, BLEND, [opacity]);
node!(Mask, MASK, []);
node!(CombineMask, COMBINE_MASK, []);
node!(ChannelMask, CHANNEL_MASK, []);
node!(
    SurfaceBlend,
    SURFACE_BLEND,
    [
        opacity, lighting, detail, wear, threshold, distortion, softness, grain
    ]
);
node!(Fade, FADE, [opacity], {
    fn validate(&self) -> Result<()> {
        self.timing.validate()
    }
});
