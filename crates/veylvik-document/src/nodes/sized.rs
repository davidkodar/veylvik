//! A branch rendered at a size of its own: what feeds an Own size node is rendered at that node's width and height, and its picture may only go where a picture of any size is wrapped or placed.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Largest side of an own-size picture, and the most pixels it may hold (1.5 GiB of
/// picture): a whole sky needs more than a composition's 8192.
pub const OWN_SIZE_MAX: u32 = 16384;
pub const OWN_SIZE_PIXELS: u64 = 96 * 1024 * 1024;

/// How an own-size picture meets the frame where a frame-sized image is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Pixel for pixel, centred.
    Actual,
    /// Scaled to sit wholly inside the frame, centred.
    #[default]
    Fit,
    /// Scaled to cover the frame, centred; what does not fit is cut off.
    Fill,
    /// Stretched to the frame's shape.
    Stretch,
}
impl Fit {
    pub const ALL: [Self; 4] = [Self::Actual, Self::Fit, Self::Fill, Self::Stretch];
}
/// Renders its input at `width` by `height` instead of the composition's size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnSize {
    pub width: u32,
    pub height: u32,
    /// Used where the picture feeds something that needs the frame's size.
    #[serde(default)]
    pub fit: Fit,
}
pub static OWN_SIZE: NodeType = NodeType {
    type_id: "own_size",
    version: 1,
    label: "Own size",
    description: "Renders everything connected to its Image input at its own width and height (1 to 16384 pixels each, at most 96 million pixels together, for example a 12288 by 6144 sky) instead of the composition's, so a small picture costs a small picture's memory and time: a 1024 by 1024 galaxy is a sixteenth of a 4K frame. Inside that branch the frame is the node's size: positions, sizes and blurs given in pixels are in its pixels, and its middle is the picture's middle. Feeding an Image card (which takes the picture's shape), a 3D shape's Texture or a scene Environment's Image, the picture is used as it is, at its own size. Anywhere else (any other input, the viewer, the composition's output) it is placed in the frame as Fit says: actual (pixel for pixel, centred), fit (scaled to sit inside the frame), fill (scaled to cover it) or stretch; scaling down averages the pixels it covers. A particle sprite always takes the whole picture. Footage inside the branch is placed for the branch's size.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Picture,
    parameters: &[
        Parameter {
            default: 1024.0,
            ..other("width", ParameterKind::Seed, "pixels")
        },
        Parameter {
            default: 1024.0,
            ..other("height", ParameterKind::Seed, "pixels")
        },
        other("fit", ParameterKind::Fit, "meeting the frame"),
    ],
};
node!(OwnSize, OWN_SIZE, [], {
    fn validate(&self) -> Result<()> {
        if !(1..=OWN_SIZE_MAX).contains(&self.width) || !(1..=OWN_SIZE_MAX).contains(&self.height) {
            return fail("Own size needs a width and height of 1 to 16384 pixels");
        }
        if u64::from(self.width) * u64::from(self.height) > OWN_SIZE_PIXELS {
            return fail("Own size holds at most 96 million pixels, for example 12288 by 6144");
        }
        Ok(())
    }
});
