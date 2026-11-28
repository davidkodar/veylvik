//! Imported 3D models as plain data: parts, each a mesh with its own material and optional base colour texture, normalised so the model's largest side is 1 and its box is centred on the origin.
use crate::Mesh;
use std::sync::Arc;

/// Limits on what an imported model may hold.
pub const MAX_PARTS: usize = 1024;
pub const MAX_VERTICES: usize = 4_000_000;
/// Textures are reduced to at most this many pixels on their longest side.
pub const MAX_TEXTURE: u32 = 1024;

/// A base colour texture: straight (not premultiplied) scene-linear RGBA rows.
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}
/// One part of a model and its material (glTF metallic-roughness).
#[derive(Debug, Clone)]
pub struct Part {
    /// Positions in the model's normalised frame (x right, y down, z towards the viewer);
    /// uv in 0..1 with v down, as glTF stores it.
    pub mesh: Mesh,
    /// Scene-linear base colour factor; alpha is kept but parts render opaque.
    pub colour: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub texture: Option<Arc<Texture>>,
    /// The node of the file's tree it hangs from, which its clips may move.
    pub joint: Option<usize>,
    /// The inverse of that node's place at rest (glTF's frame), so its motion is its
    /// place now times this.
    pub rest_inverse: crate::animation::Mat,
    /// A skinned part bends with the joints of its skin instead of moving as one piece.
    pub skin: Option<Arc<Skin>>,
}
/// How a skinned part's vertices follow its joints (glTF skins: linear blend skinning).
#[derive(Debug, Clone, Default)]
pub struct Skin {
    /// The file's nodes that are its joints.
    pub joints: Vec<usize>,
    /// Each joint's inverse bind matrix (row-major, glTF's frame).
    pub inverse_bind: Vec<crate::animation::Mat>,
    /// Each vertex in the mesh's own space (glTF's frame, y up), and its normal.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Each vertex's four joints (into `joints`) and their weights (summing to 1).
    pub influences: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub parts: Vec<Part>,
    /// The normalised box's size: its largest side is 1.
    pub extent: [f32; 3],
    /// The file's node tree at rest, and the clips that key it.
    pub joints: Vec<crate::animation::Joint>,
    pub clips: Vec<crate::animation::Clip>,
    /// The normalised frame: glTF positions (y flipped) less `centre`, over `largest`.
    pub centre: [f32; 3],
    pub largest: f32,
}
