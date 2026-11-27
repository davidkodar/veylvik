//! The scene as the tracer sees it: each mesh's tree with its triangles in tree order (made once and shared by every copy and frame), and for one frame the meshes placed in the world with their surfaces, a top tree over the placed boxes, and all of it packed into flat arrays the GPU walks, with an exact two-level CPU walk to check the GPU against.
use crate::bvh::{Bvh, Node};
use std::sync::Arc;

/// Row-major 4 x 4, acting on column vectors, as the scene's.
pub type Mat4 = [[f32; 4]; 4];

/// A mesh's tree and its geometry in the tree's order: a leaf's triangles are
/// `first .. first + count` of `triangles`.
#[derive(Debug, Default)]
pub struct MeshTree {
    pub nodes: Vec<Node>,
    /// Position, then normal and uv: two vec4s a vertex.
    pub vertices: Vec<[f32; 4]>,
    /// Corner indices (into this mesh's vertices) and a spare.
    pub triangles: Vec<[u32; 4]>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}
impl MeshTree {
    pub fn new(mesh: &veylvik_mesh::Mesh) -> Self {
        let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
        let count = mesh.indices.len() / 3 * 3;
        let indices = &mesh.indices[..count];
        let bvh = Bvh::build(&positions, indices);
        let triangles = bvh
            .order
            .iter()
            .map(|&t| {
                let t = t as usize;
                [indices[3 * t], indices[3 * t + 1], indices[3 * t + 2], 0]
            })
            .collect();
        let vertices = mesh
            .vertices
            .iter()
            .flat_map(|v| {
                [
                    [v.position[0], v.position[1], v.position[2], v.normal[0]],
                    [v.normal[1], v.normal[2], v.uv[0], v.uv[1]],
                ]
            })
            .collect();
        let root = bvh.nodes[0];
        Self {
            nodes: bvh.nodes,
            vertices,
            triangles,
            min: root.min,
            max: root.max,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }
}
/// How a surface takes light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub colour: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    /// Its own light, the colour times this, added as it is.
    pub emission: f32,
    pub opacity: f32,
    /// What kind of surface it is and how its texture is read: the constants below.
    pub flags: u32,
    /// Its picture or texture (a number in the job's textures), if any.
    pub texture: Option<u32>,
    /// A shadow catcher's strength.
    pub shadow: f32,
    /// The finish, as the interactive renderer reads it: brushing, the grain's direction
    /// (radians), imperfections, visible grain and ridges (all but the direction 0 to 1).
    pub finish: [f32; 5],
    /// A library look (a number in the job's looks), lit in place of the plain colour.
    pub look: Option<u32>,
    /// A picture cast onto it through a camera.
    pub projection: Option<Projection>,
}
/// A picture cast through a camera: where the camera saw a point is the picture there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    /// World to the camera's own space (rows); it looks along its -z with y down.
    pub to_camera: [[f32; 4]; 3],
    /// Where the camera is, in the world.
    pub eye: [f32; 3],
    /// Its focal length in pixels of the frame.
    pub focal: f32,
    /// The picture (a number in the job's textures), premultiplied.
    pub picture: u32,
    /// The picture becomes the surface's colour, lit (else it shows as it is).
    pub lit: bool,
    /// What the camera did not see is left out (else keeps the surface's own colour).
    pub hide_unseen: bool,
}
impl Default for Surface {
    fn default() -> Self {
        Self {
            colour: [0.8; 3],
            roughness: 0.5,
            metallic: 0.0,
            emission: 0.0,
            opacity: 1.0,
            flags: 0,
            texture: None,
            shadow: 1.0,
            finish: [0.0; 5],
            look: None,
            projection: None,
        }
    }
}
/// Shows its colour as it is.
pub const UNLIT: u32 = 1;
/// Shows only the shadows falling on it, for 3D over footage; other rays pass through.
pub const CATCHER: u32 = 2;
/// An image card: its picture, premultiplied and unlit, edges clamped; partly transparent
/// pixels let rays through in proportion.
pub const CARD: u32 = 4;
/// A shape's picture: premultiplied, repeating; under half opaque it is a hole.
pub const CUT: u32 = 8;
/// A model part's own texture: straight colour, repeating.
pub const STRAIGHT: u32 = 16;
/// A glass look: light passes through it to what lies behind, tinted by the glass, so its
/// shadow is coloured and faint rather than solid.
pub const GLASS: u32 = 32;
/// A mesh placed in the world with a surface.
#[derive(Clone)]
pub struct Placed {
    pub tree: Arc<MeshTree>,
    pub world: Mat4,
    pub surface: Surface,
}
/// One placed mesh as the GPU reads it (112 bytes): the world-to-object and
/// object-to-world rows, and where its tree and geometry start in the packed arrays.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub to_object: [[f32; 4]; 3],
    pub to_world: [[f32; 4]; 3],
    pub root: u32,
    pub triangle_base: u32,
    pub vertex_base: u32,
    pub surface: u32,
}
/// A frame's scene in flat arrays.
#[derive(Debug, Default)]
pub struct Packed {
    /// Every distinct mesh's tree, one after another (an instance's `root` is its first).
    pub nodes: Vec<Node>,
    pub vertices: Vec<[f32; 4]>,
    pub triangles: Vec<[u32; 4]>,
    pub instances: Vec<Instance>,
    pub surfaces: Vec<Surface>,
    /// The top tree over the instances' world boxes; its leaves name instances through `top_order`.
    pub top: Vec<Node>,
    pub top_order: Vec<u32>,
    /// The meshes stored, in order (by identity): the same list means the same geometry
    /// arrays, so a GPU copy of them can be kept.
    pub meshes: Vec<usize>,
}
/// The inverse of an affine matrix, or None when it squashes space flat.
pub fn affine_inverse(m: &Mat4) -> Option<Mat4> {
    let a = |i: usize, j: usize| m[i][j];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
        - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if !det.is_finite() || det.abs() < 1e-20 {
        return None;
    }
    let mut inv = [
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (i, row) in inv.iter_mut().take(3).enumerate() {
        for (j, v) in row.iter_mut().take(3).enumerate() {
            let (r0, r1) = ((j + 1) % 3, (j + 2) % 3);
            let (c0, c1) = ((i + 1) % 3, (i + 2) % 3);
            *v = (a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0)) / det;
        }
        row[3] = -(0..3).map(|k| row[k] * m[k][3]).sum::<f32>();
    }
    Some(inv)
}
fn apply(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}
fn turn(m: &Mat4, d: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * d[0] + m[i][1] * d[1] + m[i][2] * d[2])
}
/// Pack a frame's placed meshes. Meshes shared by several placings are stored once;
/// empty meshes and placings that squash space flat are left out.
pub fn pack(placed: &[Placed]) -> Packed {
    let mut out = Packed::default();
    // Each distinct tree once: (pointer, node base, triangle base, vertex base).
    let mut stored: Vec<(*const MeshTree, u32, u32, u32)> = Vec::new();
    let mut boxes = Vec::new();
    for p in placed {
        let tree = &p.tree;
        if tree.is_empty() {
            continue;
        }
        let Some(to_object) = affine_inverse(&p.world) else {
            continue;
        };
        let key = Arc::as_ptr(tree);
        let bases = match stored.iter().find(|s| s.0 == key) {
            Some(&(_, n, t, v)) => (n, t, v),
            None => {
                let bases = (
                    out.nodes.len() as u32,
                    out.triangles.len() as u32,
                    (out.vertices.len() / 2) as u32,
                );
                out.nodes.extend_from_slice(&tree.nodes);
                out.triangles.extend_from_slice(&tree.triangles);
                out.vertices.extend_from_slice(&tree.vertices);
                stored.push((key, bases.0, bases.1, bases.2));
                out.meshes.push(key as usize);
                bases
            }
        };
        let surface = match out.surfaces.iter().position(|s| *s == p.surface) {
            Some(i) => i,
            None => {
                out.surfaces.push(p.surface);
                out.surfaces.len() - 1
            }
        } as u32;
        out.instances.push(Instance {
            to_object: [to_object[0], to_object[1], to_object[2]],
            to_world: [p.world[0], p.world[1], p.world[2]],
            root: bases.0,
            triangle_base: bases.1,
            vertex_base: bases.2,
            surface,
        });
        // The world box round the mesh's box's eight corners.
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for k in 0..8 {
            let c = [0, 1, 2].map(|a| {
                if k >> a & 1 == 0 {
                    tree.min[a]
                } else {
                    tree.max[a]
                }
            });
            let w = apply(&p.world, c);
            for a in 0..3 {
                lo[a] = lo[a].min(w[a]);
                hi[a] = hi[a].max(w[a]);
            }
        }
        boxes.push((lo, hi));
    }
    let top = Bvh::over_boxes(&boxes);
    out.top = top.nodes;
    out.top_order = top.order;
    out
}
/// Where a ray first meets the packed scene: distance, instance, triangle (in the packed
/// array) and barycentric weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneHit {
    pub t: f32,
    pub instance: u32,
    pub triangle: u32,
    pub u: f32,
    pub v: f32,
}
impl Packed {
    /// The two-level walk the GPU does: the top tree to instances, each ray turned into the
    /// instance's own space (where distances stay in world lengths, the direction being
    /// turned not normalised), then its mesh's tree.
    pub fn hit(&self, origin: [f32; 3], direction: [f32; 3], far: f32) -> Option<SceneHit> {
        if self.instances.is_empty() {
            return None;
        }
        let mut best: Option<SceneHit> = None;
        let mut limit = far;
        let mut stack = vec![0usize];
        while let Some(at) = stack.pop() {
            let n = self.top[at];
            if !enters(&n, origin, direction, limit) {
                continue;
            }
            if n.count > 0 {
                for &i in &self.top_order[n.first as usize..(n.first + n.count) as usize] {
                    let inst = self.instances[i as usize];
                    let m: Mat4 = [
                        inst.to_object[0],
                        inst.to_object[1],
                        inst.to_object[2],
                        [0.0, 0.0, 0.0, 1.0],
                    ];
                    let (o, d) = (apply(&m, origin), turn(&m, direction));
                    if let Some(h) = self.mesh_hit(&inst, o, d, limit) {
                        limit = h.t;
                        best = Some(SceneHit { instance: i, ..h });
                    }
                }
            } else if n.first > 0 {
                stack.extend([n.first as usize + 1, n.first as usize]);
            }
        }
        best
    }
    fn mesh_hit(&self, inst: &Instance, o: [f32; 3], d: [f32; 3], far: f32) -> Option<SceneHit> {
        let mut best = None;
        let mut limit = far;
        let mut stack = vec![inst.root as usize];
        while let Some(at) = stack.pop() {
            let n = self.nodes[at];
            if !enters(&n, o, d, limit) {
                continue;
            }
            if n.count > 0 {
                for k in n.first..n.first + n.count {
                    let t = (inst.triangle_base + k) as usize;
                    let c = [0, 1, 2].map(|j| {
                        let v =
                            self.vertices[2 * (inst.vertex_base + self.triangles[t][j]) as usize];
                        [v[0], v[1], v[2]]
                    });
                    if let Some((dist, u, v)) = crate::bvh::triangle(c, o, d)
                        && dist < limit
                    {
                        limit = dist;
                        best = Some(SceneHit {
                            t: dist,
                            instance: 0,
                            triangle: t as u32,
                            u,
                            v,
                        });
                    }
                }
            } else if n.first > 0 {
                let child = inst.root + n.first;
                stack.extend([child as usize + 1, child as usize]);
            }
        }
        best
    }
}
fn enters(n: &Node, o: [f32; 3], d: [f32; 3], far: f32) -> bool {
    let (mut near, mut out) = (0.0f32, far);
    for k in 0..3 {
        let inv = 1.0 / d[k];
        let a = (n.min[k] - o[k]) * inv;
        let b = (n.max[k] - o[k]) * inv;
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        near = if lo > near { lo } else { near };
        out = if hi < out { hi } else { out };
    }
    near <= out
}
#[cfg(test)]
#[path = "scene/tests.rs"]
mod tests;
