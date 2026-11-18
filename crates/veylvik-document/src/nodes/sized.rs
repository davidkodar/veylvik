//! A branch rendered at a size of its own: what feeds an Own size node is rendered at that node's width and height, and its picture may only go where a picture of any size is wrapped or placed.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Largest side of an own-size picture, as for a composition.
pub const OWN_SIZE_MAX: u32 = 8192;

/// Renders its input at `width` by `height` instead of the composition's size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnSize {
    pub width: u32,
    pub height: u32,
}
pub static OWN_SIZE: NodeType = NodeType {
    type_id: "own_size",
    version: 1,
    label: "Own size",
    description: "Renders everything connected to its Image input at its own width and height (1 to 8192 pixels each) instead of the composition's, so a small picture costs a small picture's memory and time: a 1024 by 1024 galaxy is a sixteenth of a 4K frame. Inside that branch the frame is the node's size: positions, sizes and blurs given in pixels are in its pixels, and its middle is the picture's middle. The picture can feed an Image card (which then takes the picture's shape), a 3D shape's Texture, or a scene Environment's Image; other inputs need a frame-sized image and refuse it. It cannot be the composition's output or be viewed on its own: view the scene it feeds.",
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
    ],
};
node!(OwnSize, OWN_SIZE, [], {
    fn validate(&self) -> Result<()> {
        if !(1..=OWN_SIZE_MAX).contains(&self.width) || !(1..=OWN_SIZE_MAX).contains(&self.height) {
            return fail("Own size needs a width and height of 1 to 8192 pixels");
        }
        Ok(())
    }
});
