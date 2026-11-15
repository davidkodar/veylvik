//! GPU layout shared by library samples and composition material nodes.
use veylvik_document::{
    MaterialKind,
    optical::{Palette, StudioRig, SurfaceOptics},
};
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Lighting {
    pub optics: [f32; 4],
    pub environment: [f32; 4],
    pub lights: [[[f32; 4]; 4]; 8],
    /// Pattern colours; the family decides what each one paints.
    pub palette: [[f32; 4]; 3],
}
/// Uniform size, shared by the bind group layout and per-node buffers.
pub(crate) const SIZE: u64 = std::mem::size_of::<Lighting>() as u64;
impl Lighting {
    /// `shot` is the material's Shot mix at the frame time (palette[0].w).
    pub fn material(m: &veylvik_document::nodes::Material, shot: f32) -> Self {
        let mut lighting = Self::new(m.kind, m.optics, m.colours, &m.studio);
        lighting.palette[0][3] = shot.clamp(0.0, 1.0);
        lighting
    }
    pub fn new(
        kind: MaterialKind,
        optics: Option<SurfaceOptics>,
        colours: Option<Palette>,
        rig: &StudioRig,
    ) -> Self {
        let palette = colours.unwrap_or_else(|| kind.palette());
        let optics = optics.unwrap_or_else(|| kind.optics());
        let f0 = optics.f0();
        let mut result = Self {
            optics: [f0[0], f0[1], f0[2], if optics.metallic { 1. } else { 0. }],
            environment: [
                rig.ambient,
                rig.lights.len().min(8) as f32,
                rig.environment,
                rig.environment_rotation.to_radians(),
            ],
            lights: [[[0.; 4]; 4]; 8],
            palette: palette.map(|[r, g, b]| [r, g, b, 0.]),
        };
        for (output, light) in result.lights.iter_mut().zip(&rig.lights) {
            let rotate = |[x, y, z]: [f32; 3]| {
                let (sx, cx) = light.rotation[0].to_radians().sin_cos();
                let (sy, cy) = light.rotation[1].to_radians().sin_cos();
                let (sz, cz) = light.rotation[2].to_radians().sin_cos();
                let (y, z) = (y * cx - z * sx, y * sx + z * cx);
                let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
                [x * cz - y * sz, x * sz + y * cz, z, 0.]
            };
            output[0] = [
                light.position[0],
                light.position[1],
                light.position[2],
                if light.enabled { light.intensity } else { 0. },
            ];
            output[1] = rotate([light.size[0] * 0.5, 0., 0.]);
            output[2] = rotate([0., light.size[1] * 0.5, 0.]);
            output[3] = [light.colour[0], light.colour[1], light.colour[2], 0.];
        }
        result
    }
}
