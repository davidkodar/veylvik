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
    pub field_push: Scalar,
    pub field_flow: Scalar,
    pub bounce: Scalar,
    pub pull_x: Scalar,
    pub pull_y: Scalar,
    pub pull: Scalar,
    pub vortex: Scalar,
    pub spin: Scalar,
    pub grow: Scalar,
    #[serde(default = "zero")]
    pub colour_variation: Scalar,
    #[serde(default = "zero")]
    pub depth: Scalar,
    #[serde(default = "first")]
    pub camera: Scalar,
    /// Where the box sits in depth (pixels from the frame, towards the viewer).
    #[serde(default = "zero")]
    pub z: Scalar,
    /// How far from the camera particles are drawn (0: any distance).
    #[serde(default = "zero")]
    pub reach: Scalar,
    /// Launch speed towards the viewer (away when negative).
    #[serde(default = "zero")]
    pub towards: Scalar,
    /// Exposure time: with a scene camera, its move during that time streaks the particles.
    #[serde(default = "zero")]
    pub shutter: Scalar,
    /// Earlier moments of the emitter kept, so particles are born where it was (0: none).
    #[serde(default)]
    pub history: u32,
    /// In a scene: the same size on screen at any distance, as a star is a point however
    /// near, where particles otherwise grow as they come close.
    #[serde(default)]
    pub points: bool,
    /// In a scene: not blurred by the camera's depth of field.
    #[serde(default)]
    pub in_focus: bool,
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
fn zero() -> Scalar {
    0.0.into()
}
fn first() -> Scalar {
    1.0.into()
}
pub static PARTICLES: NodeType = NodeType {
    type_id: "particles",
    version: 1,
    label: "Particles",
    description: "Soft dots born over time and moved by a launch speed, gravity, wind and swirling turbulence, on a transparent frame. With emitter connected, particles are born where that image has alpha (text, a shape, a matte), density of them a second for every 100 x 100 pixels of it; otherwise rate a second are born inside the box of width x height centred on x, y (0 x 0 is a point). Each lives life seconds, launched at speed along direction (degrees, 0 right, 90 up) within spread degrees, then pulled by gravity (down) and wind (right) in pixels per second squared and pushed about by turbulence (pixels per second, swirls about turbulence_scale pixels across, never converging or thinning the cloud). Size is the dot's radius; colour runs to end_colour over the life (linear RGBA, values above 1 glow), fading out over the last fade share of it. Glow 1 adds light where dots overlap, 0 lets them cover one another. Variation makes speeds, sizes and lives differ. Emission begins at start seconds and lasts duration seconds (0 is endless; a short duration is a burst). Inherit tints each particle with the colour of the emitter where it was born (1 is fully the picture's own colour, times colour), so an image breaks up into dust of itself. With target connected, each particle is given a place in that image's alpha and steered onto it over the last homing share of its life (1 is the whole life, 0 never), arriving exactly as it dies and, with inherit, taking the target's colour on the way; particles that find no place are not born. Streak draws each particle as a trail back along where it was streak seconds earlier (at most 96 pixels), for sparks and rain. Field is an image that pushes particles: field_push accelerates them from its bright parts towards its dark parts (negative draws them in) and field_flow along its edges, so they stream round a shape. Pull draws them to the point pull_x, pull_y (negative drives them away) and vortex swings them round it. Obstacle is a matte they bounce off, keeping bounce of their speed. Sprite is an image drawn as each particle in place of a dot (the whole frame of that input, size wide either side, tinted by the colours), turning at spin degrees a second; grow is the size at the end of the life as a factor of the size at birth. Colour_variation turns each particle's hue by its own random amount (1 is anywhere round the colour wheel), for confetti. Particles live in the 3D scenes' space: the frame at depth 0, in pixels, z towards the viewer. Depth spreads their births that many pixels in front of and behind the frame, lets the launch spread and the turbulence carry them through depth, and draws near ones larger. They are filmed by the camera numbered camera in the scene connected to scene (as a Render scene node counts them; 0, or no scene, is the view that sees the frame exactly), so they move with a 3D or solved camera and show parallax. Every particle is recomputed from the frame's time alone, so frames render in any order and identically. Keyed settings are followed: a particle is launched with the settings in force when it was born, and forces act as keyed while it flies, sampled 16 times across the last life; target, field, obstacle and sprite images are read as they are at the frame, and so is the emitter unless history is set: then the emitter is rendered at that many earlier moments across the last life (4, 9 or 16 are used, on whole frames, at a half, a third or a quarter of the size) and each particle is born on the emitter as it was nearest its birth, so moving text or footage leaves its particles behind. That costs as many extra renders of the emitter's branch for every frame. At most 2000000 particles from a box. Connect a Render scene with Depth on (same scene and camera) to Depth and 3D objects hide the particles behind them; the scene camera's depth of field blurs particles out of focus. In depth: Z places the box in front of (positive) or behind the frame, Towards launches particles at the viewer (away when negative), and Reach is how far from the camera they are drawn, fading out over its last quarter (0: any distance), so a volume of stars to fly through shows no far end. Shutter is an exposure time in seconds: with a scene connected, the camera's move during it streaks each particle, so a fast flight draws stars as streaks. Particles exist from Start onwards: set Start to minus their Life to have a full cloud at the first frame. Life may be up to an hour, so a field of stars can simply stand (rate times life is still at most 2,000,000 particles). In a scene, Points keeps every particle the same size on screen at any distance, as stars are points however near, and In focus keeps the camera's depth of field from blurring them.",
    inputs: &[
        "emitter", "target", "field", "obstacle", "sprite", "scene", "depth",
    ],
    optional: &[
        "emitter", "target", "field", "obstacle", "sprite", "scene", "depth",
    ],
    output: PortKind::Image,
    parameters: &[
        scalar("rate", "per second", 0.0, 2_000_000.0, 300.0),
        scalar(
            "density",
            "per second per 100 px square",
            0.0,
            10_000.0,
            60.0,
        ),
        scalar("life", "seconds", 0.05, 3600.0, 2.0),
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
        scalar(
            "field_push",
            "pixels per second squared",
            -8192.0,
            8192.0,
            400.0,
        ),
        scalar(
            "field_flow",
            "pixels per second squared",
            -8192.0,
            8192.0,
            0.0,
        ),
        scalar("bounce", "factor", 0.0, 1.0, 0.6),
        scalar("pull_x", "pixels", -1_000_000.0, 1_000_000.0, 960.0),
        scalar("pull_y", "pixels", -1_000_000.0, 1_000_000.0, 540.0),
        scalar("pull", "pixels per second squared", -8192.0, 8192.0, 0.0),
        scalar("vortex", "pixels per second squared", -8192.0, 8192.0, 0.0),
        scalar("spin", "degrees per second", -3600.0, 3600.0, 0.0),
        scalar("grow", "factor", 0.0, 8.0, 1.0),
        scalar("colour_variation", "factor", 0.0, 1.0, 0.0),
        scalar("depth", "pixels", 0.0, 1_000_000.0, 0.0),
        scalar("camera", "index", 0.0, 64.0, 1.0),
        scalar("z", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("reach", "pixels", 0.0, 1_000_000.0, 0.0),
        scalar("towards", "pixels per second", -100_000.0, 100_000.0, 0.0),
        scalar("shutter", "seconds", 0.0, 0.5, 0.0),
        scalar("start", "seconds", -3600.0, 3600.0, 0.0),
        scalar("duration", "seconds", 0.0, 3600.0, 0.0),
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 960.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 540.0),
        scalar("width", "pixels", 0.0, 1_000_000.0, 0.0),
        scalar("height", "pixels", 0.0, 1_000_000.0, 0.0),
        Parameter {
            default: 1.0,
            ..other("colour", ParameterKind::Color, "linear RGBA")
        },
        Parameter {
            default: 1.0,
            ..other("end_colour", ParameterKind::Color, "linear RGBA")
        },
        other("seed", ParameterKind::Seed, "random seed"),
        other("history", ParameterKind::Seed, "moments"),
        other(
            "points",
            ParameterKind::Boolean,
            "same size at any distance",
        ),
        other("in_focus", ParameterKind::Boolean, "ignore depth of field"),
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
        field_push,
        field_flow,
        bounce,
        pull_x,
        pull_y,
        pull,
        vortex,
        spin,
        grow,
        colour_variation,
        depth,
        camera,
        z,
        reach,
        towards,
        shutter,
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
            if self.history > 16 {
                return fail("Particles keep at most 16 moments of the emitter");
            }
            Ok(())
        }
    }
);
