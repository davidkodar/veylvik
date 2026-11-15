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

/// Pattern colours in linear Rec.709, 0–1. Each family gives them its own roles
/// (`MaterialKind::colour_roles`); unused entries are ignored.
pub type Palette = [[f32; 3]; 3];
pub fn validate_palette(palette: &Palette) -> crate::Result<()> {
    if palette
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return crate::fail("Material colours must be finite linear RGB 0–1");
    }
    Ok(())
}
pub fn palette_schema() -> serde_json::Value {
    let colour = serde_json::json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":0,"maximum":1}});
    serde_json::json!({"anyOf":[{"type":"null"},{"type":"array","minItems":3,"maxItems":3,"items":colour}]})
}
impl MaterialKind {
    /// What each palette entry colours. Metals have none: their colour is the
    /// reflectance in `SurfaceOptics`.
    pub fn colour_roles(self) -> &'static [&'static str] {
        use MaterialKind::*;
        match self {
            Gold | Silver | Aluminium | Bronze => &[],
            Phosphor => &["Glow"],
            Wood | OakLight | OakDark => &["Dark grain", "Light grain"],
            Marble => &["Stone", "Veins"],
            BrickLight | BrickRed => &["Mortar", "Dark brick", "Light brick"],
            Concrete => &["Concrete"],
            Paper => &["Paper"],
            AgedPaper => &["Light stain", "Dark stain"],
            LeatherRed | LeatherBrown | LeatherTan | LeatherDark | LeatherBlack => &["Leather"],
        }
    }
    /// The family's own colours, used when a material has no palette.
    pub fn palette(self) -> Palette {
        use MaterialKind::*;
        const DARK_OAK: [f32; 3] = [0.32, 0.23, 0.16];
        let tint = |c: [f32; 3], t: [f32; 3]| [c[0] * t[0], c[1] * t[1], c[2] * t[2]];
        let (a, b, c) = match self {
            Gold | Silver | Aluminium | Bronze => ([0.0; 3], [0.0; 3], [0.0; 3]),
            // Glow is scaled by 1.9 in the shader so the stored colour stays 0–1.
            Phosphor => ([0.12 / 1.9, 1.0, 0.2], [0.0; 3], [0.0; 3]),
            Wood => ([0.065, 0.017, 0.004], [0.5, 0.22, 0.065], [0.0; 3]),
            OakLight => ([0.16, 0.065, 0.018], [0.69, 0.46, 0.23], [0.0; 3]),
            OakDark => (
                tint([0.16, 0.065, 0.018], DARK_OAK),
                tint([0.69, 0.46, 0.23], DARK_OAK),
                [0.0; 3],
            ),
            Marble => ([0.78, 0.77, 0.71], [0.15, 0.18, 0.19], [0.0; 3]),
            BrickLight => ([0.28, 0.26, 0.22], [0.42, 0.36, 0.24], [0.75, 0.67, 0.49]),
            BrickRed => (
                [0.28, 0.26, 0.22],
                [0.22, 0.035, 0.012],
                [0.55, 0.17, 0.055],
            ),
            Concrete => ([0.37, 0.36, 0.33], [0.0; 3], [0.0; 3]),
            Paper => ([0.88, 0.865, 0.82], [0.0; 3], [0.0; 3]),
            AgedPaper => ([0.64, 0.49, 0.28], [0.26, 0.13, 0.045], [0.0; 3]),
            LeatherRed => ([0.25, 0.012, 0.018], [0.0; 3], [0.0; 3]),
            LeatherBrown => ([0.19, 0.064, 0.022], [0.0; 3], [0.0; 3]),
            LeatherTan => ([0.52, 0.29, 0.12], [0.0; 3], [0.0; 3]),
            LeatherDark => ([0.055, 0.018, 0.008], [0.0; 3], [0.0; 3]),
            LeatherBlack => ([0.008, 0.009, 0.012], [0.0; 3], [0.0; 3]),
        };
        [a, b, c]
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
    0.65
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
                // The photographic studio already contains the key and fill.
                // Extra cards are opt-in; stacking them made solid white patches.
                light.enabled = false;
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

/// Auto preserves the finish of documents created before explicit metal finishes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Brushing {
    #[default]
    Auto,
    Smooth,
    Directional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetalFinish {
    Polished,
    Satin,
    Matte,
    Brushed,
}
impl MetalFinish {
    pub const ALL: [Self; 4] = [Self::Polished, Self::Satin, Self::Matte, Self::Brushed];
    pub fn label(self) -> &'static str {
        match self {
            Self::Polished => "Polished",
            Self::Satin => "Satin",
            Self::Matte => "Matte",
            Self::Brushed => "Brushed",
        }
    }
    pub fn settings(self) -> (f32, f32, Brushing) {
        match self {
            Self::Polished => (0.18, 0.25, Brushing::Smooth),
            Self::Satin => (0.38, 0.35, Brushing::Smooth),
            Self::Matte => (0.7, 0.45, Brushing::Smooth),
            Self::Brushed => (0.28, 0.65, Brushing::Directional),
        }
    }
}
impl crate::nodes::Material {
    pub fn is_brushed(&self) -> bool {
        self.kind.optics().metallic
            && match self.brushing {
                Brushing::Auto => self.kind == crate::MaterialKind::Aluminium,
                Brushing::Smooth => false,
                Brushing::Directional => true,
            }
    }
    /// Finish recipes change only roughness, grain and directional brushing.
    /// Geometry, optical response and authored lighting remain independent.
    pub fn apply_finish(&mut self, finish: MetalFinish) {
        if !self.kind.optics().metallic {
            return;
        }
        let (roughness, grain, brushing) = finish.settings();
        self.roughness = roughness.into();
        self.grain = grain.into();
        self.brushing = brushing;
    }
    pub fn finish(&self) -> Option<MetalFinish> {
        if !self.kind.optics().metallic {
            return None;
        }
        let (
            crate::Scalar::Constant { value: roughness },
            crate::Scalar::Constant { value: grain },
        ) = (&self.roughness, &self.grain)
        else {
            return None;
        };
        MetalFinish::ALL.into_iter().find(|finish| {
            let (r, g, b) = finish.settings();
            (roughness - r).abs() < 0.0001
                && (grain - g).abs() < 0.0001
                && self.is_brushed() == (b == Brushing::Directional)
        })
    }
}

/// Where a shot's light comes from, measured from one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotLight {
    /// Centroid of the brightest part of the frame, in pixels.
    pub position: [f32; 2],
    /// Its colour, normalised so the strongest channel is 1.
    pub colour: [f32; 3],
    /// Brightest-region luminance over mean luminance; near 1 means flat light.
    pub contrast: f32,
    /// Mean scene-linear luminance of the frame.
    pub mean: f32,
}
impl ShotLight {
    /// Analyse premultiplied linear pixels on a coarse grid (at most 96 cells wide).
    /// The brightest 5% of cells, weighted by how far they exceed that threshold,
    /// give the light's position and colour. None for an empty or black frame.
    pub fn analyse(pixels: &[[f32; 4]], width: u32, height: u32) -> Option<Self> {
        let (w, h) = (width as usize, height as usize);
        if w == 0 || h == 0 || pixels.len() != w * h {
            return None;
        }
        let gw = w.min(96);
        let gh = (h * gw / w).max(1);
        let mut cells = Vec::with_capacity(gw * gh);
        for gy in 0..gh {
            for gx in 0..gw {
                let (x0, x1) = (gx * w / gw, ((gx + 1) * w / gw).max(gx * w / gw + 1));
                let (y0, y1) = (gy * h / gh, ((gy + 1) * h / gh).max(gy * h / gh + 1));
                let mut sum = [0.0f64; 3];
                for y in y0..y1 {
                    for p in &pixels[y * w + x0..y * w + x1] {
                        for c in 0..3 {
                            sum[c] += f64::from(p[c].max(0.0)).min(1.0e4);
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)) as f64;
                let rgb = sum.map(|v| (v / n) as f32);
                let centre = [(x0 + x1) as f32 * 0.5, (y0 + y1) as f32 * 0.5];
                cells.push((rgb, luminance(rgb), centre));
            }
        }
        let mean = cells.iter().map(|c| c.1).sum::<f32>() / cells.len() as f32;
        if !mean.is_finite() || mean <= 1.0e-6 {
            return None;
        }
        let mut sorted: Vec<f32> = cells.iter().map(|c| c.1).collect();
        sorted.sort_by(f32::total_cmp);
        let threshold = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
        let (mut weight, mut position, mut colour, mut bright) =
            (0.0f32, [0.0f32; 2], [0.0f32; 3], 0.0f32);
        for (rgb, l, centre) in &cells {
            if *l >= threshold {
                let w = (l - threshold) + 1.0e-4;
                weight += w;
                for i in 0..2 {
                    position[i] += centre[i] * w;
                }
                for i in 0..3 {
                    colour[i] += rgb[i] * w;
                }
                bright += l * w;
            }
        }
        let position = position.map(|v| v / weight);
        let peak = colour.iter().copied().fold(0.0f32, f32::max);
        let colour = if peak > 0.0 {
            colour.map(|v| (v / peak).clamp(0.0, 1.0))
        } else {
            [1.0; 3]
        };
        Some(Self {
            position,
            colour,
            contrast: bright / weight / mean,
            mean,
        })
    }
}
fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}
impl StudioRig {
    /// Place the key light (the second card, as in the default rig) where the shot's
    /// light comes from, facing the subject, in the shot's light colour, and scale the
    /// environment to the shot's brightness. Flat light keeps a soft key above.
    /// Image space maps to scene units like the viewer guides: an eighth of the
    /// shorter side per unit around the frame centre.
    pub fn match_shot(&mut self, shot: &ShotLight, width: u32, height: u32) {
        let unit = width.min(height).max(1) as f32 / 8.0;
        let directional = shot.contrast >= 1.3;
        let (x, y) = if directional {
            (
                ((shot.position[0] - width as f32 * 0.5) / unit).clamp(-6.0, 6.0),
                ((shot.position[1] - height as f32 * 0.5) / unit).clamp(-6.0, 6.0),
            )
        } else {
            (0.0, -3.0)
        };
        let position = [x, y, 2.5];
        // Area lights emit along their rotated +Z; aim it at the subject at the origin.
        let length = (x * x + y * y + 2.5 * 2.5).sqrt();
        let d = position.map(|v| v / length);
        let rotation = [
            (-d[1]).asin().to_degrees(),
            d[0].atan2(d[2]).to_degrees(),
            0.0,
        ];
        while self.lights.len() < 2 {
            self.lights.push(AreaLight {
                enabled: false,
                ..AreaLight::default()
            });
        }
        let key = &mut self.lights[1];
        key.enabled = true;
        key.position = position;
        key.rotation = rotation;
        key.colour = shot.colour;
        key.intensity = if directional {
            (1.2 * shot.contrast.min(10.0)).clamp(1.2, 12.0)
        } else {
            1.2
        };
        self.environment = (0.65 * shot.mean / 0.18).clamp(0.05, 4.0);
    }
}
