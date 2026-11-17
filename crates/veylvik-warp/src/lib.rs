//! Shape-to-shape warp fields: both shapes are resampled to matching points along their outlines, and a mesh of pixel offsets is built that moves each start point exactly onto its end point, carries the shape's inside with it and fades to nothing `reach` pixels outside. Bounded: at most 8 contours, 96 points each, 129 mesh points a side.
use veylvik_document::vector::{PathCommand, PathMask};

/// Points each contour is resampled to.
pub const SAMPLES: usize = 96;
/// Most contours a shape may have.
pub const MAX_CONTOURS: usize = 8;
/// Most mesh points along one side, and the pixels between them when there is room.
pub const MAX_SIDE: u32 = 129;
const CELL: f32 = 16.0;
const CURVE_STEPS: usize = 16;

/// A mesh of offsets in pixels, `columns` x `rows`, row by row, spread evenly over the frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub columns: u32,
    pub rows: u32,
    pub offsets: Vec<[f32; 2]>,
}

/// Each closed contour as a polyline (the closing point is not repeated).
fn contours(path: &PathMask) -> Vec<Vec<[f32; 2]>> {
    let mut all = Vec::new();
    let mut current: Vec<[f32; 2]> = Vec::new();
    let mut at = [0.0f32; 2];
    let mix =
        |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    for command in &path.commands {
        match *command {
            PathCommand::Move { to } => {
                current = vec![to];
                at = to;
            }
            PathCommand::Line { to } => {
                current.push(to);
                at = to;
            }
            PathCommand::Quad { control, to } => {
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    current.push(mix(mix(at, control, t), mix(control, to, t), t));
                }
                at = to;
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let (a, b, c) = (
                        mix(at, control1, t),
                        mix(control1, control2, t),
                        mix(control2, to, t),
                    );
                    current.push(mix(mix(a, b, t), mix(b, c, t), t));
                }
                at = to;
            }
            PathCommand::Close {} => {
                if current.len() > 1 && current.first() == current.last() {
                    current.pop();
                }
                all.push(std::mem::take(&mut current));
            }
        }
    }
    all
}
fn area(points: &[[f32; 2]]) -> f32 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        * 0.5
}
/// `SAMPLES` points evenly spaced along a closed polyline, from its first point,
/// always going the same way round (positive area) so two shapes match up.
fn resample(mut points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    if area(&points) < 0.0 {
        points[1..].reverse();
    }
    let n = points.len();
    let lengths: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            (b[0] - a[0]).hypot(b[1] - a[1])
        })
        .collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 {
        return vec![points[0]; SAMPLES];
    }
    let (mut edge, mut before) = (0, 0.0f32);
    (0..SAMPLES)
        .map(|i| {
            let target = total * i as f32 / SAMPLES as f32;
            while edge + 1 < n && before + lengths[edge] < target {
                before += lengths[edge];
                edge += 1;
            }
            let t = if lengths[edge] > 0.0 {
                ((target - before) / lengths[edge]).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (a, b) = (points[edge], points[(edge + 1) % n]);
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        })
        .collect()
}
/// The matching points of a shape, contour by contour. Errors on too many contours.
pub fn samples(path: &PathMask) -> Result<Vec<Vec<[f32; 2]>>, String> {
    let contours = contours(path);
    if contours.is_empty() || contours.len() > MAX_CONTOURS {
        return Err("A warp shape has 1 to 8 closed outlines".into());
    }
    Ok(contours.into_iter().map(resample).collect())
}
/// Whether two shapes can be matched: the same number of outlines.
pub fn check(from: &PathMask, to: &PathMask) -> Result<(), String> {
    if samples(from)?.len() != samples(to)?.len() {
        return Err("The start and end shapes need the same number of outlines".into());
    }
    Ok(())
}
/// The mesh's points across and down for a frame.
pub fn sides(width: u32, height: u32) -> (u32, u32) {
    // A point about every CELL pixels, but never fewer than 33 a side in a small frame.
    let side = |pixels: u32| {
        ((pixels as f32 / CELL).ceil() as u32 + 1)
            .max(pixels.min(33))
            .clamp(2, MAX_SIDE)
    };
    (side(width), side(height))
}
fn inside(contours: &[Vec<[f32; 2]>], p: [f32; 2]) -> bool {
    let mut winding = 0i32;
    for points in contours {
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let side = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
            if a[1] <= p[1] && b[1] > p[1] && side > 0.0 {
                winding += 1;
            } else if a[1] > p[1] && b[1] <= p[1] && side < 0.0 {
                winding -= 1;
            }
        }
    }
    winding != 0
}
/// The mesh that carries `from` onto `to` in a frame of `width` x `height` pixels.
/// Outside the start shape the pull fades over `reach` pixels; inside, everything follows.
pub fn field(
    from: &PathMask,
    to: &PathMask,
    width: u32,
    height: u32,
    reach: f32,
) -> Result<Field, String> {
    check(from, to)?;
    let (starts, ends) = (samples(from)?, samples(to)?);
    let pairs: Vec<([f32; 2], [f32; 2])> = starts
        .iter()
        .zip(&ends)
        .flat_map(|(a, b)| {
            a.iter()
                .zip(b)
                .map(|(a, b)| (*a, [b[0] - a[0], b[1] - a[1]]))
        })
        .collect();
    let (columns, rows) = sides(width, height);
    let reach = reach.max(1.0);
    let mut offsets = Vec::with_capacity((columns * rows) as usize);
    for j in 0..rows {
        for i in 0..columns {
            let p = [
                width as f32 * i as f32 / (columns - 1) as f32,
                height as f32 * j as f32 / (rows - 1) as f32,
            ];
            let (mut sum, mut weights, mut nearest) = ([0.0f64; 2], 0.0f64, f32::MAX);
            for (at, offset) in &pairs {
                let d2 = (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2);
                nearest = nearest.min(d2);
                // Inverse fourth power: close points dominate, so the outline is followed tightly.
                let w = 1.0 / (f64::from(d2) * f64::from(d2) + 1e-6);
                sum[0] += w * f64::from(offset[0]);
                sum[1] += w * f64::from(offset[1]);
                weights += w;
            }
            let fade = if inside(&starts, p) {
                1.0
            } else {
                let t = (nearest.sqrt() / reach).clamp(0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            };
            offsets.push([
                (sum[0] / weights) as f32 * fade,
                (sum[1] / weights) as f32 * fade,
            ]);
        }
    }
    Ok(Field {
        columns,
        rows,
        offsets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(x: f32, y: f32, w: f32, h: f32) -> PathMask {
        PathMask {
            keys: vec![],
            fill_rule: Default::default(),
            commands: vec![
                PathCommand::Move { to: [x, y] },
                PathCommand::Line { to: [x + w, y] },
                PathCommand::Line { to: [x + w, y + h] },
                PathCommand::Line { to: [x, y + h] },
                PathCommand::Close {},
            ],
        }
    }
    fn at(field: &Field, width: u32, height: u32, p: [f32; 2]) -> [f32; 2] {
        // Bilinear read, enough for the tests.
        let gx = p[0] / width as f32 * (field.columns - 1) as f32;
        let gy = p[1] / height as f32 * (field.rows - 1) as f32;
        let (i, j) = (gx.floor() as u32, gy.floor() as u32);
        let (i1, j1) = ((i + 1).min(field.columns - 1), (j + 1).min(field.rows - 1));
        let (fx, fy) = (gx - i as f32, gy - j as f32);
        let o = |i: u32, j: u32| field.offsets[(j * field.columns + i) as usize];
        [0, 1].map(|k| {
            (o(i, j)[k] * (1.0 - fx) + o(i1, j)[k] * fx) * (1.0 - fy)
                + (o(i, j1)[k] * (1.0 - fx) + o(i1, j1)[k] * fx) * fy
        })
    }
    #[test]
    fn a_moved_shape_carries_its_inside_and_leaves_the_far_frame_alone() {
        let from = rectangle(200.0, 150.0, 160.0, 120.0);
        let to = rectangle(240.0, 130.0, 160.0, 120.0);
        let field = field(&from, &to, 640, 480, 80.0).unwrap();
        assert_eq!((field.columns, field.rows), (41, 33));
        // The whole inside moves by the shape's move.
        for p in [[280.0, 210.0], [210.0, 160.0], [350.0, 260.0]] {
            let o = at(&field, 640, 480, p);
            assert!(
                (o[0] - 40.0).abs() < 0.5 && (o[1] + 20.0).abs() < 0.5,
                "{o:?}"
            );
        }
        // Beyond the reach nothing moves; within it the pull is partial.
        assert_eq!(at(&field, 640, 480, [20.0, 20.0]), [0.0, 0.0]);
        assert_eq!(at(&field, 640, 480, [600.0, 440.0]), [0.0, 0.0]);
        let near = at(&field, 640, 480, [400.0, 210.0]);
        assert!(near[0] > 2.0 && near[0] < 38.0, "{near:?}");
    }
    #[test]
    fn matching_shapes_do_nothing_and_a_stretched_side_pulls_only_that_side() {
        let from = rectangle(200.0, 150.0, 160.0, 120.0);
        let same = field(&from, &from.clone(), 640, 480, 80.0).unwrap();
        assert!(same.offsets.iter().all(|o| *o == [0.0, 0.0]));
        // The right side pulled 48 px out: the right edge follows, the left edge stays.
        let wider = rectangle(200.0, 150.0, 208.0, 120.0);
        let field = field(&from, &wider, 640, 480, 80.0).unwrap();
        let right = at(&field, 640, 480, [352.0, 208.0]);
        let left = at(&field, 640, 480, [208.0, 208.0]);
        assert!(right[0] > 35.0, "{right:?}");
        assert!(left[0].abs() < 6.0, "{left:?}");
    }
    #[test]
    fn shapes_drawn_the_other_way_round_still_match_and_counts_must_agree() {
        let from = rectangle(100.0, 100.0, 100.0, 100.0);
        let mut reversed = rectangle(130.0, 100.0, 100.0, 100.0);
        reversed.commands = vec![
            PathCommand::Move { to: [130.0, 100.0] },
            PathCommand::Line { to: [130.0, 200.0] },
            PathCommand::Line { to: [230.0, 200.0] },
            PathCommand::Line { to: [230.0, 100.0] },
            PathCommand::Close {},
        ];
        let field = field(&from, &reversed, 400, 300, 60.0).unwrap();
        let o = at(&field, 400, 300, [150.0, 150.0]);
        assert!((o[0] - 30.0).abs() < 0.5 && o[1].abs() < 0.5, "{o:?}");
        let mut two = from.clone();
        two.commands
            .extend(rectangle(300.0, 100.0, 40.0, 40.0).commands);
        assert!(check(&from, &two).is_err());
    }
}
