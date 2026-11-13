//! Planar warps read their track's motion; tracks alias their source.
#[allow(unused_imports)]
use super::{Cx, Passes, RenderNode};
#[allow(unused_imports)]
use crate::{Kernel, Params, Result, error};
#[allow(unused_imports)]
use veylvik_document::{Operation, Scalar};

impl RenderNode for veylvik_document::nodes::PlanarTrack {
    /// A track node passes its source through; consumers read its saved motion.
    fn identity(&self) -> bool {
        true
    }
}
impl RenderNode for veylvik_document::nodes::PlanarWarp {
    fn passes(&self) -> Passes {
        Passes::One(Kernel::PlanarWarp)
    }
    #[allow(unused_variables)]
    fn params(
        &self,
        cx: &Cx<'_>,
        sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let Cx {
            node,
            nodes,
            width,
            height,
            time,
        } = *cx;
        let veylvik_document::nodes::PlanarWarp { mode } = self;
        {
            let track = node
                .inputs
                .get("track")
                .and_then(|id| nodes.iter().find(|n| n.id == *id))
                .ok_or_else(|| error("Planar track missing"))?;
            let Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack { data }) =
                &track.operation
            else {
                return Err(error("Invalid planar track input"));
            };
            let forward = data.motion_at(time)?;
            let motion = match mode {
                veylvik_document::planar::WarpMode::Follow => forward,
                veylvik_document::planar::WarpMode::Fit => {
                    forward.compose(veylvik_document::planar::Homography::between(
                        [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
                        data.reference,
                    )?)
                }
                veylvik_document::planar::WarpMode::Stabilize => forward
                    .inverse()
                    .ok_or_else(|| error("Singular planar motion"))?,
            };
            let inverse = motion
                .inverse()
                .ok_or_else(|| error("Singular planar warp"))?;
            let matrix = inverse.matrix.map(|v| (v / inverse.matrix[8]) as f32);
            if matrix.iter().any(|v| !v.is_finite()) {
                return Err(error("Invalid planar warp"));
            }
            p.values.copy_from_slice(&matrix[..4]);
            p.extra.copy_from_slice(&matrix[4..8]);
        }
        Ok(())
    }
}
