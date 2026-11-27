//! A bounding volume hierarchy over triangles, built by the surface area heuristic over binned centroids, laid out flat for the GPU (a node's children are next to each other, a leaf names a run of triangles), with an exact CPU walk that is the reference the GPU's is checked against.

/// One node, 32 bytes as the GPU reads it. An inner node's children are `first` and
/// `first + 1`; a leaf (`count` > 0) holds triangles `first .. first + count` of `order`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Node {
    pub min: [f32; 3],
    pub first: u32,
    pub max: [f32; 3],
    pub count: u32,
}
/// The most triangles a leaf holds.
pub const LEAF: usize = 4;
/// Centroid bins the split is chosen among on each axis.
const BINS: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct Bvh {
    pub nodes: Vec<Node>,
    /// Triangle numbers in leaf order.
    pub order: Vec<u32>,
}
/// Where a ray first meets a triangle: its distance along the ray, the triangle and the
/// barycentric weights of its second and third corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f32,
    pub triangle: u32,
    pub u: f32,
    pub v: f32,
}
#[derive(Clone, Copy)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}
impl Bounds {
    const EMPTY: Self = Self {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };
    fn grow(&mut self, p: [f32; 3]) {
        for ((lo, hi), v) in self.min.iter_mut().zip(&mut self.max).zip(p) {
            *lo = lo.min(v);
            *hi = hi.max(v);
        }
    }
    fn join(&mut self, b: &Bounds) {
        self.grow(b.min);
        self.grow(b.max);
    }
    fn area(&self) -> f32 {
        let d = [0, 1, 2].map(|k| (self.max[k] - self.min[k]).max(0.0));
        2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0])
    }
}
fn corners(positions: &[[f32; 3]], indices: &[u32], t: usize) -> [[f32; 3]; 3] {
    [0, 1, 2].map(|k| positions[indices[3 * t + k] as usize])
}
impl Bvh {
    /// The hierarchy over the triangles `indices` makes of `positions` (three indices a
    /// triangle). An empty mesh has one empty leaf.
    pub fn build(positions: &[[f32; 3]], indices: &[u32]) -> Self {
        let boxes: Vec<([f32; 3], [f32; 3])> = (0..indices.len() / 3)
            .map(|t| {
                let mut b = Bounds::EMPTY;
                for p in corners(positions, indices, t) {
                    b.grow(p);
                }
                (b.min, b.max)
            })
            .collect();
        Self::over_boxes(&boxes)
    }
    /// The hierarchy over any boxes (each `(min, max)`): triangles, or placed objects for
    /// the top level. `order` names boxes by their place in `items`.
    pub fn over_boxes(items: &[([f32; 3], [f32; 3])]) -> Self {
        let count = items.len();
        let boxes: Vec<Bounds> = items
            .iter()
            .map(|&(min, max)| Bounds { min, max })
            .collect();
        let centres: Vec<[f32; 3]> = boxes
            .iter()
            .map(|b| [0, 1, 2].map(|k| 0.5 * (b.min[k] + b.max[k])))
            .collect();
        let mut order: Vec<u32> = (0..count as u32).collect();
        let mut nodes = vec![Node::default()];
        // (node, first, count) still to split.
        let mut todo = vec![(0usize, 0usize, count)];
        while let Some((at, first, n)) = todo.pop() {
            let mut b = Bounds::EMPTY;
            let mut c = Bounds::EMPTY;
            for &t in &order[first..first + n] {
                b.join(&boxes[t as usize]);
                c.grow(centres[t as usize]);
            }
            let leaf = |nodes: &mut Vec<Node>| {
                nodes[at] = Node {
                    min: b.min,
                    first: first as u32,
                    max: b.max,
                    count: n as u32,
                };
            };
            if n <= LEAF {
                // An empty mesh: one node holding nothing (no node ever has child 0).
                if n == 0 {
                    nodes[at] = Node::default();
                } else {
                    leaf(&mut nodes);
                }
                continue;
            }
            // The cheapest split among the bins of each axis, by the surface area heuristic.
            let mut best: Option<(f32, usize, f32)> = None;
            for (axis, (&lo, &hi)) in c.min.iter().zip(&c.max).enumerate() {
                if hi - lo <= 1e-12 * (1.0 + lo.abs()) {
                    continue;
                }
                let scale = BINS as f32 / (hi - lo);
                let bin =
                    |t: u32| (((centres[t as usize][axis] - lo) * scale) as usize).min(BINS - 1);
                let mut bins = [(Bounds::EMPTY, 0usize); BINS];
                for &t in &order[first..first + n] {
                    let k = bin(t);
                    bins[k].0.join(&boxes[t as usize]);
                    bins[k].1 += 1;
                }
                let mut right = [(0.0f32, 0usize); BINS];
                let (mut rb, mut rn) = (Bounds::EMPTY, 0usize);
                for k in (1..BINS).rev() {
                    rb.join(&bins[k].0);
                    rn += bins[k].1;
                    right[k] = (rb.area(), rn);
                }
                let (mut lb, mut ln) = (Bounds::EMPTY, 0usize);
                for k in 0..BINS - 1 {
                    lb.join(&bins[k].0);
                    ln += bins[k].1;
                    let (ra, rn) = right[k + 1];
                    if ln == 0 || rn == 0 {
                        continue;
                    }
                    let cost = lb.area() * ln as f32 + ra * rn as f32;
                    if best.is_none_or(|(c, _, _)| cost < c) {
                        best = Some((cost, axis, lo + (k + 1) as f32 / scale));
                    }
                }
            }
            let leaf_cost = b.area() * n as f32;
            let mid = match best {
                // Splitting must pay for one more step through a box, unless the leaf is too big.
                Some((cost, axis, at_x)) if cost < leaf_cost || n > 2 * LEAF => {
                    let part = &mut order[first..first + n];
                    let mut i = 0;
                    for j in 0..part.len() {
                        if centres[part[j] as usize][axis] < at_x {
                            part.swap(i, j);
                            i += 1;
                        }
                    }
                    i
                }
                _ if n <= 2 * LEAF => {
                    leaf(&mut nodes);
                    continue;
                }
                // Every centroid in one place: halve by count, so the tree stays bounded.
                _ => n / 2,
            };
            let mid = if mid == 0 || mid == n { n / 2 } else { mid };
            let child = nodes.len();
            nodes.push(Node::default());
            nodes.push(Node::default());
            nodes[at] = Node {
                min: b.min,
                first: child as u32,
                max: b.max,
                count: 0,
            };
            todo.push((child + 1, first + mid, n - mid));
            todo.push((child, first, mid));
        }
        Self { nodes, order }
    }
    /// The nearest triangle the ray from `origin` along `direction` meets nearer than
    /// `far` (in lengths of `direction`), walking the tree as the GPU does.
    pub fn hit(
        &self,
        positions: &[[f32; 3]],
        indices: &[u32],
        origin: [f32; 3],
        direction: [f32; 3],
        far: f32,
    ) -> Option<Hit> {
        if self.order.is_empty() {
            return None;
        }
        let inverse = direction.map(|d| 1.0 / d);
        let mut best: Option<Hit> = None;
        let mut limit = far;
        let mut stack = vec![0usize];
        while let Some(at) = stack.pop() {
            let node = self.nodes[at];
            if box_distance(&node, origin, inverse, limit).is_none() {
                continue;
            }
            if node.count > 0 {
                for &t in &self.order[node.first as usize..(node.first + node.count) as usize] {
                    if let Some((d, u, v)) =
                        triangle(corners(positions, indices, t as usize), origin, direction)
                        && d < limit
                    {
                        limit = d;
                        best = Some(Hit {
                            t: d,
                            triangle: t,
                            u,
                            v,
                        });
                    }
                }
            } else if node.first > 0 {
                // Nearer child last, so it is walked first.
                let (a, b) = (node.first as usize, node.first as usize + 1);
                let da = box_distance(&self.nodes[a], origin, inverse, limit);
                let db = box_distance(&self.nodes[b], origin, inverse, limit);
                match (da, db) {
                    (Some(x), Some(y)) if x <= y => stack.extend([b, a]),
                    (Some(_), Some(_)) => stack.extend([a, b]),
                    (Some(_), None) => stack.push(a),
                    (None, Some(_)) => stack.push(b),
                    (None, None) => {}
                }
            }
        }
        best
    }
}
/// Where the ray enters the node's box, if it does before `far`.
fn box_distance(node: &Node, origin: [f32; 3], inverse: [f32; 3], far: f32) -> Option<f32> {
    let (mut near, mut out) = (0.0f32, far);
    for k in 0..3 {
        let a = (node.min[k] - origin[k]) * inverse[k];
        let b = (node.max[k] - origin[k]) * inverse[k];
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        // NaN (a ray in the box's plane, parallel to it) leaves the bounds as they are.
        near = if lo > near { lo } else { near };
        out = if hi < out { hi } else { out };
    }
    (near <= out).then_some(near)
}
/// Möller-Trumbore: the distance and barycentric weights where the ray meets the triangle
/// from either side.
pub(crate) fn triangle(c: [[f32; 3]; 3], o: [f32; 3], d: [f32; 3]) -> Option<(f32, f32, f32)> {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (e1, e2) = (sub(c[1], c[0]), sub(c[2], c[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = sub(o, c[0]);
    let u = dot(s, p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) / det;
    (t > 0.0).then_some((t, u, v))
}
#[cfg(test)]
mod tests;
