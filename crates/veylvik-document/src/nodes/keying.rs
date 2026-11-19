//! Keyers that turn a backdrop colour or brightness range into transparency.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Keys out a coloured backdrop (green or blue screen) by colour difference, with despill.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChromaKey {
    /// Straight scene-linear RGB of the backdrop; alpha is ignored.
    pub key_color: [f32; 4],
    pub tolerance: Scalar,
    pub softness: Scalar,
    pub despill: Scalar,
    /// Matte values at or below this become fully transparent (clears a veil of
    /// leftover screen).
    #[serde(default = "zero")]
    pub clip_black: Scalar,
    /// Matte values at or above this become fully opaque (fills thin spots in the subject).
    #[serde(default = "one")]
    pub clip_white: Scalar,
    /// Pixels moves the matte's edge inwards (positive) or outwards (negative).
    #[serde(default = "zero")]
    pub shrink: Scalar,
    /// Blurs the matte's edge by this many pixels.
    #[serde(default = "zero")]
    pub soften: Scalar,
    /// Keeps pixels much darker than the screen (dark hair, shadow detail) whatever
    /// their colour leans to: the share of the screen's brightness below which a pixel
    /// counts as subject. 0 keys by colour alone, as files from before it do.
    #[serde(default = "zero")]
    pub darks: Scalar,
    /// What the node shows: the keyed picture, or its matte in grey.
    #[serde(default)]
    pub show: KeyView,
}
fn zero() -> Scalar {
    0.0.into()
}
fn one() -> Scalar {
    1.0.into()
}
/// What a keyer shows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyView {
    #[default]
    Result,
    /// The matte as grey: white is subject, black is gone, grey is partly there.
    Matte,
}
impl KeyView {
    pub const ALL: [Self; 2] = [Self::Result, Self::Matte];
    pub fn label(self) -> &'static str {
        match self {
            Self::Result => "Keyed picture",
            Self::Matte => "Matte",
        }
    }
}
/// Keys by luminance: transparent below `low`, opaque above `high`; swap them to invert.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LumaKey {
    pub low: Scalar,
    pub high: Scalar,
}

/// Snaps a coarse matte (such as a Subject matte) to the image's real edges and removes the old
/// background's colour from the subject's edge pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefineMatte {
    pub radius: Scalar,
    pub detail: Scalar,
    pub decontaminate: Scalar,
}

pub static CHROMA_KEY: NodeType = NodeType {
    type_id: "chroma_key",
    version: 1,
    label: "Chroma key",
    description: "Make a green, blue or other coloured backdrop transparent with a colour-difference key: how strongly a pixel leans towards Key colour's dominant channel (for green, green minus the larger of red and blue), relative to the pixel's own brightness and compared with the key, so shadowed and HDR parts of the screen key alike and the screen need not match Key colour exactly. Pixels leaning at least 1 − Tolerance as much as the key become transparent; over a further Softness they fade back to opaque. Skin, greys and colours without that bias stay opaque, and near-black pixels are kept. Despill limits the key's channel in what remains to the average of the other two, removing coloured fringes and bounce light. Then the matte can be shaped: Keep darks treats pixels far darker than the screen (dark hair, shadow detail) as subject, by the share of the screen's brightness given; Clip black makes matte values at or below it fully transparent (clearing a veil of leftover screen) and Clip white makes values at or above it fully opaque (filling thin spots); Shrink moves the edge inwards by that many pixels (negative grows it) and Soften blurs it. The optional Keep input is a mask of what is always subject (its alpha), Garbage a mask of what is always removed (rigging, the edge of the screen). Show matte gives the matte in grey instead of the picture, to judge all of this. Output is premultiplied; input alpha is preserved.",
    inputs: &["image", "keep", "garbage"],
    optional: &["keep", "garbage"],
    output: PortKind::Image,
    parameters: &[
        other("key_color", ParameterKind::KeyColor, "linear RGB"),
        scalar("tolerance", "amount", 0.0, 1.0, 0.5),
        scalar("softness", "amount", 0.0, 1.0, 0.25),
        scalar("despill", "amount", 0.0, 1.0, 1.0),
        scalar("darks", "amount", 0.0, 0.5, 0.05),
        scalar("clip_black", "amount", 0.0, 1.0, 0.0),
        scalar("clip_white", "amount", 0.0, 1.0, 1.0),
        scalar("shrink", "pixels", -32.0, 32.0, 0.0),
        scalar("soften", "pixels", 0.0, 32.0, 0.0),
        other("show", ParameterKind::KeyView, ""),
    ],
};
pub static LUMA_KEY: NodeType = NodeType {
    type_id: "luma_key",
    version: 1,
    label: "Luma key",
    description: "Make pixels transparent by Rec.709 luminance: fully transparent at or below Low, fully opaque at or above High, with a linear ramp between. Set Low above High to key out bright areas instead. Scene-linear values above 1 work for HDR keys. Output is premultiplied; input alpha is preserved.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("low", "luminance", 0.0, 64.0, 0.02),
        scalar("high", "luminance", 0.0, 64.0, 0.1),
    ],
};

pub static REFINE_MATTE: NodeType = NodeType {
    type_id: "refine_matte",
    version: 1,
    label: "Refine matte",
    description: "Cut out a subject with a refined edge: Image is the footage, Matte a coarse mask of the subject (its alpha), such as a Subject matte from segmentation. Within Radius pixels of the matte's edge, each pixel's opacity is re-estimated from where its colour lies between the nearby subject and background colours, snapping the edge to the image and recovering hair and fine detail; higher Detail also refines where subject and background colours are closer. Decontaminate re-estimates the subject's colour at semi-transparent edges, removing the old background's colour so no halo shows over a new background. Refinement never weakens what the matte is sure of: its confident interior stays opaque, and changes fade out within about one Radius of its edge. Output is the premultiplied subject.",
    inputs: &["image", "matte"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("radius", "pixels", 1.0, 64.0, 6.0),
        scalar("detail", "amount", 0.0, 1.0, 0.7),
        scalar("decontaminate", "amount", 0.0, 1.0, 1.0),
    ],
};

node!(RefineMatte, REFINE_MATTE, [radius, detail, decontaminate]);
node!(
    ChromaKey,
    CHROMA_KEY,
    [
        tolerance, softness, despill, darks, clip_black, clip_white, shrink, soften
    ],
    {
        fn validate(&self) -> Result<()> {
            let [r, g, b, _] = self.key_color;
            if [r, g, b].iter().any(|v| !v.is_finite() || *v < 0.0) || r + g + b <= 1e-6 {
                return fail("chroma key colour needs finite, non-negative RGB that is not black");
            }
            Ok(())
        }
    }
);
node!(LumaKey, LUMA_KEY, [low, high]);
