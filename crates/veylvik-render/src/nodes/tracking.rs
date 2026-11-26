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
            ..
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
                veylvik_document::planar::WarpMode::FitInside => {
                    // The surface's proportions from its edges on the reference frame
                    // (in pixels); the picture is the frame's shape. The picture sits
                    // in the middle of the larger rectangle that fills the surface.
                    let side = |a: usize, b: usize| {
                        let (p, q) = (data.reference[a], data.reference[b]);
                        ((p[0] - q[0]) * f64::from(width)).hypot((p[1] - q[1]) * f64::from(height))
                    };
                    let surface = (side(0, 1) + side(3, 2)) / (side(0, 3) + side(1, 2)).max(1e-9);
                    let picture = f64::from(width) / f64::from(height);
                    let (fx, fy) = if picture > surface {
                        (1.0, (surface / picture).max(1e-6))
                    } else {
                        ((picture / surface).max(1e-6), 1.0)
                    };
                    let (x0, x1) = (0.5 - 0.5 / fx, 0.5 + 0.5 / fx);
                    let (y0, y1) = (0.5 - 0.5 / fy, 0.5 + 0.5 / fy);
                    forward.compose(veylvik_document::planar::Homography::between(
                        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
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
            // The tracked area is in front of the camera by construction. Output pixels
            // whose inverse projection has the opposite homogeneous sign see the plane
            // behind the camera, where the layer must not appear (1: keep w > 0, 2: w < 0).
            let area = data.region.unwrap_or(data.reference);
            let centre = [0, 1].map(|axis| area.iter().map(|p| p[axis]).sum::<f64>() / 4.);
            let visible = match mode {
                veylvik_document::planar::WarpMode::Stabilize => Some(centre),
                _ => forward.project(centre),
            };
            p.size_op[3] = visible.map_or(0, |[x, y]| {
                let w = f64::from(matrix[6]) * x + f64::from(matrix[7]) * y + 1.;
                if w > 0. { 1 } else { 2 }
            });
        }
        Ok(())
    }
}
