//! Keyers that turn a backdrop colour or brightness range into transparency.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
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
}
/// Keys by luminance: transparent below `low`, opaque above `high`; swap them to invert.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LumaKey {
    pub low: Scalar,
    pub high: Scalar,
}

pub static CHROMA_KEY: NodeType = NodeType {
    type_id: "chroma_key",
    version: 1,
    label: "Chroma key",
    description: "Make a green, blue or other coloured backdrop transparent with a colour-difference key: how strongly a pixel leans towards Key colour's dominant channel (for green, green minus the larger of red and blue), relative to the pixel's own brightness and compared with the key, so shadowed and HDR parts of the screen key alike and the screen need not match Key colour exactly. Pixels leaning at least 1 − Tolerance as much as the key become transparent; over a further Softness they fade back to opaque. Skin, greys and colours without that bias stay opaque, and near-black pixels are kept. Despill limits the key's channel in what remains to the average of the other two, removing coloured fringes and bounce light. Output is premultiplied; input alpha is preserved.",
    inputs: &["image"],
    parameters: &[
        other("key_color", ParameterKind::KeyColor, "linear RGB"),
        scalar("tolerance", "amount", 0.0, 1.0, 0.5),
        scalar("softness", "amount", 0.0, 1.0, 0.25),
        scalar("despill", "amount", 0.0, 1.0, 1.0),
    ],
};
pub static LUMA_KEY: NodeType = NodeType {
    type_id: "luma_key",
    version: 1,
    label: "Luma key",
    description: "Make pixels transparent by Rec.709 luminance: fully transparent at or below Low, fully opaque at or above High, with a linear ramp between. Set Low above High to key out bright areas instead. Scene-linear values above 1 work for HDR keys. Output is premultiplied; input alpha is preserved.",
    inputs: &["image"],
    parameters: &[
        scalar("low", "luminance", 0.0, 64.0, 0.02),
        scalar("high", "luminance", 0.0, 64.0, 0.1),
    ],
};

node!(ChromaKey, CHROMA_KEY, [tolerance, softness, despill], {
    fn validate(&self) -> Result<()> {
        let [r, g, b, _] = self.key_color;
        if [r, g, b].iter().any(|v| !v.is_finite() || *v < 0.0) || r + g + b <= 1e-6 {
            return fail("chroma key colour needs finite, non-negative RGB that is not black");
        }
        Ok(())
    }
});
node!(LumaKey, LUMA_KEY, [low, high]);
