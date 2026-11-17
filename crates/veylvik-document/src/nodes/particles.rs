//! Particle systems. A particle's whole life is a function of the frame's time, so any frame renders on its own.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Particles born from a box or from another image's alpha, moved by a launch
/// speed, gravity, wind and swirling turbulence, and drawn as soft dots.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Particles(Box<ParticleSettings>);
impl std::ops::Deref for Particles {
    type Target = ParticleSettings;
    fn deref(&self) -> &ParticleSettings {
        &self.0
    }
}
impl std::ops::DerefMut for Particles {
    fn deref_mut(&mut self) -> &mut ParticleSettings {
        &mut self.0
    }
}
/// A particle node's settings, boxed so the node stays the size of the others.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleSettings {
    pub rate: Scalar,
    pub density: Scalar,
    pub life: Scalar,
    pub speed: Scalar,
    pub direction: Scalar,
    pub spread: Scalar,
    pub gravity: Scalar,
    pub wind: Scalar,
    pub turbulence: Scalar,
    pub turbulence_scale: Scalar,
    pub size: Scalar,
    pub glow: Scalar,
    pub variation: Scalar,
    pub fade: Scalar,
    pub inherit: Scalar,
    pub homing: Scalar,
    pub streak: Scalar,
    pub start: Scalar,
    pub duration: Scalar,
    pub x: Scalar,
    pub y: Scalar,
    pub width: Scalar,
    pub height: Scalar,
    pub colour: [f32; 4],
    pub end_colour: [f32; 4],
    pub seed: u32,
}
pub static PARTICLES: NodeType = NodeType {
    type_id: "particles",
    version: 1,
    label: "Particles",
    description: "Soft dots born over time and moved by a launch speed, gravity, wind and swirling turbulence, on a transparent frame. With emitter connected, particles are born where that image has alpha (text, a shape, a matte), density of them a second for every 100 x 100 pixels of it; otherwise rate a second are born inside the box of width x height centred on x, y (0 x 0 is a point). Each lives life seconds, launched at speed along direction (degrees, 0 right, 90 up) within spread degrees, then pulled by gravity (down) and wind (right) in pixels per second squared and pushed about by turbulence (pixels per second, swirls about turbulence_scale pixels across, never converging or thinning the cloud). Size is the dot's radius; colour runs to end_colour over the life (linear RGBA, values above 1 glow), fading out over the last fade share of it. Glow 1 adds light where dots overlap, 0 lets them cover one another. Variation makes speeds, sizes and lives differ. Emission begins at start seconds and lasts duration seconds (0 is endless; a short duration is a burst). Inherit tints each particle with the colour of the emitter where it was born (1 is fully the picture's own colour, times colour), so an image breaks up into dust of itself. With target connected, each particle is given a place in that image's alpha and steered onto it over the last homing share of its life (1 is the whole life, 0 never), arriving exactly as it dies and, with inherit, taking the target's colour on the way; particles that find no place are not born. Streak draws each particle as a trail back along where it was streak seconds earlier (at most 96 pixels), for sparks and rain. Every particle is recomputed from the frame's time alone, so frames render in any order and identically; the settings in force at a frame apply to the particles' whole lives, and an emitter image is read as it is at that frame. At most 400000 particles from a box.",
    inputs: &["emitter", "target"],
    optional: &["emitter", "target"],
    output: PortKind::Image,
    parameters: &[
        scalar("rate", "per second", 0.0, 200_000.0, 300.0),
        scalar(
            "density",
            "per second per 100 px square",
            0.0,
            10_000.0,
            60.0,
        ),
        scalar("life", "seconds", 0.05, 30.0, 2.0),
        scalar("speed", "pixels per second", 0.0, 8192.0, 300.0),
        scalar("direction", "degrees", -360.0, 360.0, 90.0),
        scalar("spread", "degrees", 0.0, 360.0, 60.0),
        scalar(
            "gravity",
            "pixels per second squared",
            -8192.0,
            8192.0,
            300.0,
        ),
        scalar("wind", "pixels per second squared", -8192.0, 8192.0, 0.0),
        scalar("turbulence", "pixels per second", 0.0, 4096.0, 0.0),
        scalar("turbulence_scale", "pixels", 8.0, 4096.0, 240.0),
        scalar("size", "pixels", 0.25, 24.0, 3.0),
        scalar("glow", "factor", 0.0, 1.0, 1.0),
        scalar("variation", "factor", 0.0, 1.0, 0.5),
        scalar("fade", "share of life", 0.0, 1.0, 0.5),
        scalar("inherit", "factor", 0.0, 1.0, 0.0),
        scalar("homing", "share of life", 0.0, 1.0, 1.0),
        scalar("streak", "seconds", 0.0, 1.0, 0.0),
        scalar("start", "seconds", -3600.0, 3600.0, 0.0),
        scalar("duration", "seconds", 0.0, 3600.0, 0.0),
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 960.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 540.0),
        scalar("width", "pixels", 0.0, 8192.0, 0.0),
        scalar("height", "pixels", 0.0, 8192.0, 0.0),
        Parameter {
            default: 1.0,
            ..other("colour", ParameterKind::Color, "linear RGBA")
        },
        Parameter {
            default: 1.0,
            ..other("end_colour", ParameterKind::Color, "linear RGBA")
        },
        other("seed", ParameterKind::Seed, "random seed"),
    ],
};
node!(
    Particles,
    PARTICLES,
    [
        rate,
        density,
        life,
        speed,
        direction,
        spread,
        gravity,
        wind,
        turbulence,
        turbulence_scale,
        size,
        glow,
        variation,
        fade,
        inherit,
        homing,
        streak,
        start,
        duration,
        x,
        y,
        width,
        height
    ],
    {
        fn validate(&self) -> Result<()> {
            if self
                .colour
                .iter()
                .chain(&self.end_colour)
                .any(|v| !v.is_finite() || !(0.0..=64.0).contains(v))
            {
                return fail("Particle colours need values in 0..=64");
            }
            Ok(())
        }
    }
);
