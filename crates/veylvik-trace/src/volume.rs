//! Volumes (smoke, mist, haze) as both renderers read them: one medium in eight vec4s for the shared `volume.wgsl`, which says what the medium is at any point.
use crate::scene::Mat4;

/// The shared WGSL: a medium's density, edges, noise and phase function (no bindings).
pub const VOLUME_WGSL: &str = include_str!("volume.wgsl");
/// What a volume fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Box,
    Sphere,
    /// Lying on the bottom of the box (+y is down), thinning upwards.
    Ground,
}
/// A medium filling a box placed in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medium {
    /// Box frame to world (row-major); the box spans minus to plus half of `size`.
    pub world: Mat4,
    pub size: [f32; 3],
    pub shape: Shape,
    /// Extinction per world unit.
    pub sigma: f32,
    /// The share of what it stops that it absorbs rather than scatters.
    pub absorption: f32,
    /// The scattering's forward lean (Henyey-Greenstein g).
    pub forward: f32,
    pub softness: f32,
    pub colour: [f32; 3],
    pub noise: f32,
    pub noise_size: f32,
    pub detail: f32,
    pub seed: u32,
    pub drift: [f32; 3],
}
impl Medium {
    /// The eight vec4s `medium_of` reads, or None when its placing cannot be undone (flat).
    pub fn packed(&self) -> Option<[[f32; 4]; 8]> {
        let to_box = crate::scene::affine_inverse(&self.world)?;
        let shape = match self.shape {
            Shape::Box => 0.0,
            Shape::Sphere => 1.0,
            Shape::Ground => 2.0,
        };
        let [x, y, z] = self.size.map(|s| s.max(1e-3) * 0.5);
        let [r, g, b] = self.colour;
        let [dx, dy, dz] = self.drift;
        Some([
            to_box[0],
            to_box[1],
            to_box[2],
            [x, y, z, shape],
            [
                self.sigma.max(0.0),
                self.absorption.clamp(0.0, 1.0),
                self.forward.clamp(-0.95, 0.95),
                self.softness.clamp(0.0, 1.0),
            ],
            [r, g, b, self.noise.clamp(0.0, 1.0)],
            [
                self.noise_size.max(1e-3),
                self.detail.clamp(1.0, 6.0),
                f32::from_bits(self.seed),
                0.0,
            ],
            [dx, dy, dz, 0.0],
        ])
    }
}
