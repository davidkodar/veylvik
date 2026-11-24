//! Freeform warps of one image.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, integer, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Fewest and most grid points along one side.
pub const GRID_SIDE: std::ops::RangeInclusive<u32> = 2..=17;
/// Furthest a grid point may be moved, in pixels.
pub const GRID_REACH: f32 = 8192.0;

/// A mesh laid over the image, each point moved by an offset in pixels: the image
/// under a point goes where the point went, smoothly in between.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridWarp {
    pub columns: u32,
    pub rows: u32,
    /// Offsets of the points from their even rest positions, row by row.
    pub points: Vec<[f32; 2]>,
    pub amount: Scalar,
}
impl GridWarp {
    /// The same warp on a grid of another size: offsets are resampled bilinearly.
    pub fn resized(&self, columns: u32, rows: u32) -> Self {
        let at = |x: f32, y: f32| {
            let (c, r) = (self.columns as usize, self.rows as usize);
            let (x0, y0) = (
                (x.floor() as usize).min(c - 1),
                (y.floor() as usize).min(r - 1),
            );
            let (x1, y1) = ((x0 + 1).min(c - 1), (y0 + 1).min(r - 1));
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            let p = |i: usize, j: usize| self.points[j * c + i];
            [0, 1].map(|k| {
                (p(x0, y0)[k] * (1.0 - fx) + p(x1, y0)[k] * fx) * (1.0 - fy)
                    + (p(x0, y1)[k] * (1.0 - fx) + p(x1, y1)[k] * fx) * fy
            })
        };
        let points = (0..rows)
            .flat_map(|j| (0..columns).map(move |i| (i, j)))
            .map(|(i, j)| {
                at(
                    i as f32 * (self.columns - 1) as f32 / (columns - 1) as f32,
                    j as f32 * (self.rows - 1) as f32 / (rows - 1) as f32,
                )
            })
            .collect();
        Self {
            columns,
            rows,
            points,
            amount: self.amount.clone(),
        }
    }
}
pub static GRID_WARP: NodeType = NodeType {
    type_id: "grid_warp",
    version: 1,
    label: "Grid warp",
    description: "Warps the image with a mesh of columns by rows points (2 to 17 a side) spread evenly over the frame. points holds each point's offset in pixels from its rest position, row by row; the image under a point moves with it and everything between follows smoothly (Catmull-Rom across the mesh). Amount scales every offset (0 leaves the image alone, 1 is the full warp), so the warp can be keyed in and out. Each output pixel finds its source by solving the mesh backwards (eight fixed-point steps), exact while neighbouring points do not cross; bilinear premultiplied sampling, transparent beyond the frame. In Studio the points are dragged in the viewer.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        integer(
            "columns",
            "points across",
            *GRID_SIDE.start(),
            *GRID_SIDE.end(),
            4,
        ),
        integer(
            "rows",
            "points down",
            *GRID_SIDE.start(),
            *GRID_SIDE.end(),
            4,
        ),
        other("points", ParameterKind::GridPoints, "pixel offsets"),
        scalar("amount", "factor", 0.0, 1.0, 1.0),
    ],
};
node!(GridWarp, GRID_WARP, [amount], {
    fn validate(&self) -> Result<()> {
        if self.points.len() != (self.columns * self.rows) as usize {
            return fail("Grid warp needs one offset for each point (columns x rows)");
        }
        if self
            .points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > GRID_REACH)
        {
            return fail("Grid warp points move at most 8192 pixels");
        }
        Ok(())
    }
});

/// Most closed outlines a warp shape may have.
pub const WARP_CONTOURS: usize = 8;

/// Two drawn shapes: the image is pulled so the start shape lands on the end shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeWarp {
    pub from: vector::PathMask,
    pub to: vector::PathMask,
    pub amount: Scalar,
    pub reach: Scalar,
}
pub static SHAPE_WARP: NodeType = NodeType {
    type_id: "shape_warp",
    version: 1,
    label: "Shape warp",
    description: "Pulls the image so that the start shape (from) lands on the end shape (to): draw from round a feature and to where it should go. Both are closed paths in pixels with the same number of outlines (1 to 8); their point counts may differ, because each outline is matched by distance along it from its first point, going the same way round. Everything inside the start shape follows it, and the surroundings stretch over reach pixels beyond it, untouched further out. Amount scales the move (0 leaves the image alone, 1 lands on the end shape) and can be keyed for a morph; either shape may carry geometry keys. The warp runs through a mesh with a point about every 16 pixels (at most 129 a side), so detail finer than that is smoothed; a move longer than reach folds the picture.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        other("from", ParameterKind::Path, "pixels"),
        other("to", ParameterKind::Path, "pixels"),
        scalar("amount", "factor", 0.0, 1.0, 1.0),
        scalar("reach", "pixels", 1.0, 4096.0, 200.0),
    ],
};
node!(ShapeWarp, SHAPE_WARP, [amount, reach], {
    fn validate(&self) -> Result<()> {
        self.from.validate()?;
        self.to.validate()?;
        let outlines = |path: &vector::PathMask| {
            path.commands
                .iter()
                .filter(|c| matches!(c, vector::PathCommand::Close {}))
                .count()
        };
        if outlines(&self.from) > WARP_CONTOURS {
            return fail("A warp shape has at most 8 outlines");
        }
        if outlines(&self.from) != outlines(&self.to)
            || self.from.keys.iter().chain(&self.to.keys).any(|k| {
                k.commands
                    .iter()
                    .filter(|c| matches!(c, vector::PathCommand::Close {}))
                    .count()
                    != outlines(&self.from)
            })
        {
            return fail("The start and end shapes need the same number of outlines");
        }
        Ok(())
    }
});
