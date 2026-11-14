//! GPU layout shared by library samples and composition material nodes.
use veylvik_document::{
    MaterialKind,
    optical::{StudioRig, SurfaceOptics},
};
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Lighting {
    pub optics: [f32; 4],
    pub environment: [f32; 4],
    pub lights: [[[f32; 4]; 4]; 8],
}
impl Lighting {
    pub fn new(kind: MaterialKind, optics: Option<SurfaceOptics>, rig: &StudioRig) -> Self {
        let optics = optics.unwrap_or_else(|| kind.optics());
        let f0 = optics.f0();
        let mut result = Self {
            optics: [f0[0], f0[1], f0[2], if optics.metallic { 1. } else { 0. }],
            environment: [rig.ambient, rig.lights.len().min(8) as f32, 0., 0.],
            lights: [[[0.; 4]; 4]; 8],
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
