//! Portable optical response, independent of texture and lighting.
use crate::MaterialKind;
use serde::{Deserialize, Serialize};

/// Linear RGB reflectance at normal incidence. These are artistic defaults,
/// not a spectral measurement of a particular alloy, coating or specimen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceOptics {
    /// Conductors have coloured specular reflection and no diffuse lobe.
    pub metallic: bool,
    pub reflectance: [f32; 3],
    /// Dielectric refractive index; F0 is derived from this when nonmetallic.
    pub ior: f32,
}
impl SurfaceOptics {
    pub fn f0(self) -> [f32; 3] {
        if self.metallic {
            self.reflectance
        } else {
            [((self.ior - 1.0) / (self.ior + 1.0)).powi(2); 3]
        }
    }
    pub fn validate(&self) -> crate::Result<()> {
        if !self.ior.is_finite()
            || !(1.0..=3.0).contains(&self.ior)
            || self
                .reflectance
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return crate::fail("Surface optics require IOR 1–3 and finite reflectance 0–1");
        }
        Ok(())
    }
    pub fn schema() -> serde_json::Value {
        serde_json::json!({"type":"object","additionalProperties":false,
            "required":["metallic","reflectance","ior"],"properties":{
                "metallic":{"type":"boolean"},
                "reflectance":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":0,"maximum":1}},
                "ior":{"type":"number","minimum":1,"maximum":3}}})
    }
}
impl MaterialKind {
    pub fn optics(self) -> SurfaceOptics {
        use MaterialKind::*;
        let reflectance = match self {
            Gold => [0.95, 0.64, 0.21],
            Silver => [0.97, 0.96, 0.92],
            Aluminium => [0.91, 0.92, 0.92],
            Bronze => [0.78, 0.38, 0.16],
            _ => [0.04; 3],
        };
        SurfaceOptics {
            metallic: matches!(self, Gold | Silver | Aluminium | Bronze),
            reflectance,
            ior: 1.5,
        }
    }
}

pub const MAX_STUDIO_LIGHTS: usize = 8;
/// A rectangular emitter; scene units are relative to the preview sphere radius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaLight {
    pub enabled: bool,
    pub position: [f32; 3],
    /// Euler rotations in degrees, applied X then Y then Z.
    pub rotation: [f32; 3],
    pub size: [f32; 2],
    pub colour: [f32; 3],
    pub intensity: f32,
}
impl Default for AreaLight {
    fn default() -> Self {
        Self {
            enabled: true,
            position: [-3.2, -3.0, 1.5],
            rotation: [25., -45., -20.],
            size: [3.0, 4.5],
            colour: [1., 0.96, 0.89],
            intensity: 4.,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioRig {
    /// Intensity of the prefiltered studio environment, separate from extra emitters.
    #[serde(default = "default_environment")]
    pub environment: f32,
    #[serde(default = "default_environment_rotation")]
    pub environment_rotation: f32,
    pub ambient: f32,
    pub lights: Vec<AreaLight>,
}
fn default_environment() -> f32 {
    1.0
}
fn default_environment_rotation() -> f32 {
    -45.0
}
impl Default for StudioRig {
    fn default() -> Self {
        Self {
            environment: default_environment(),
            environment_rotation: default_environment_rotation(),
            ambient: 0.0,
            lights: vec![
                AreaLight {
                    position: [-3.0, -0.7, -1.0],
                    rotation: [0.0, -105.0, 0.0],
                    size: [1.5, 4.0],
                    intensity: 1.2,
                    ..AreaLight::default()
                },
                AreaLight {
                    position: [3., -0.3, 1.5],
                    rotation: [0., 50., 0.],
                    size: [2.3, 3.2],
                    colour: [0.96, 0.98, 1.],
                    intensity: 3.5,
                    ..AreaLight::default()
                },
                AreaLight {
                    position: [-2.8, 2.6, 1.5],
                    rotation: [-30., -45., 0.],
                    size: [1.8, 2.0],
                    colour: [1., 0.93, 0.8],
                    intensity: 3.2,
                    ..AreaLight::default()
                },
                AreaLight {
                    position: [0., -4., 2.],
                    rotation: [60., 0., 0.],
                    size: [7., 4.],
                    colour: [1.; 3],
                    intensity: 0.45,
                    ..AreaLight::default()
                },
            ]
            .into_iter()
            .enumerate()
            .map(|(index, mut light)| {
                light.enabled = index <= 1;
                if index == 1 {
                    light.intensity = 1.2;
                }
                light
            })
            .collect(),
        }
    }
}
impl StudioRig {
    pub fn validate(&self) -> crate::Result<()> {
        fn within(v: f32, lo: f32, hi: f32) -> bool {
            v.is_finite() && (lo..=hi).contains(&v)
        }
        if !within(self.environment_rotation, -360., 360.)
            || !within(self.environment, 0., 16.)
            || !within(self.ambient, 0., 16.)
            || self.lights.len() > MAX_STUDIO_LIGHTS
        {
            return crate::fail("Studio lighting supports up to eight lights and ambient 0–16");
        }
        for l in &self.lights {
            if l.position.iter().any(|&v| !within(v, -100., 100.))
                || l.rotation.iter().any(|&v| !within(v, -360., 360.))
                || l.size.iter().any(|&v| !within(v, 0.01, 100.))
                || l.colour.iter().any(|&v| !within(v, 0., 1.))
                || !within(l.intensity, 0., 100.)
            {
                return crate::fail(
                    "Invalid studio light position, rotation, size, colour or intensity",
                );
            }
        }
        Ok(())
    }
    pub fn schema() -> serde_json::Value {
        use serde_json::json;
        let vector = |n, lo, hi| json!({"type":"array","minItems":n,"maxItems":n,"items":{"type":"number","minimum":lo,"maximum":hi}});
        json!({"type":"object","additionalProperties":false,"required":["environment","environment_rotation","ambient","lights"],"properties":{
            "environment":{"type":"number","minimum":0,"maximum":16},
            "environment_rotation":{"type":"number","minimum":-360,"maximum":360},
            "ambient":{"type":"number","minimum":0,"maximum":16},
            "lights":{"type":"array","maxItems":MAX_STUDIO_LIGHTS,"items":{"type":"object","additionalProperties":false,
                "required":["enabled","position","rotation","size","colour","intensity"],"properties":{
                    "enabled":{"type":"boolean"},"position":vector(3,-100.,100.),"rotation":vector(3,-360.,360.),
                    "size":vector(2,0.01,100.),"colour":vector(3,0.,1.),"intensity":{"type":"number","minimum":0,"maximum":100}}}}}})
    }
}
