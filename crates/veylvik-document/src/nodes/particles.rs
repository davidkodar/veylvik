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
    /// Particles each releases when it ends (0: none), for fireworks and sparks.
    #[serde(default = "zero")]
    pub burst: Scalar,
    #[serde(default = "burst_speed")]
    pub burst_speed: Scalar,
    /// Degrees round the way the particle was going (360: all round).
    #[serde(default = "all_round")]
    pub burst_spread: Scalar,
    #[serde(default = "first")]
    pub burst_life: Scalar,
    /// A burst particle's size as a factor of the size.
    #[serde(default = "burst_size")]
    pub burst_size: Scalar,
    #[serde(default = "white")]
    pub burst_colour: [f32; 4],
    /// Bursts are set off by the obstacle, where a particle first touches it and ends;
    /// otherwise by the end of a particle's life.
    #[serde(default)]
    pub burst_on_impact: bool,
    /// A ribbon along the path each particle took, that many seconds back (0: none).
    #[serde(default = "zero")]
    pub trail: Scalar,
    /// The ribbon's width as a factor of the particle's size.
    #[serde(default = "first")]
    pub trail_width: Scalar,
    /// Lines join particles nearer each other than this (0: no lines), as a network.
    #[serde(default = "zero")]
    pub link_distance: Scalar,
    #[serde(default = "link_opacity")]
    pub link_opacity: Scalar,
    #[serde(default = "first")]
    pub link_width: Scalar,
    /// Size over the life, as a factor of the size, at 8 even steps (none: grow).
    #[serde(default)]
    pub size_over_life: Option<[f32; 8]>,
    /// Opacity over the life at 8 even steps (none: the fade).
    #[serde(default)]
    pub opacity_over_life: Option<[f32; 8]>,
    /// Speed over the life, as a factor of the flight, at 8 even steps (none: as is).
    #[serde(default)]
    pub speed_over_life: Option<[f32; 8]>,
    /// Colour over the life at 8 even steps (none: colour to end colour).
    #[serde(default)]
    pub colour_over_life: Option<[[f32; 4]; 8]>,
    /// How much of a particle's colour is the light reaching it from the scene's lights
    /// (0: it glows by itself, as before).
    #[serde(default = "zero")]
    pub lit: Scalar,
    /// Light everywhere, for lit particles.
    #[serde(default = "ambient")]
    pub ambient: Scalar,
    /// How much brighter lit particles are looking into the light (negative: away).
    #[serde(default = "scatter")]
    pub scatter: Scalar,
    /// Flocking: particles within this many pixels of one another are neighbours that
    /// steer by each other (0: they do not, and the system is not simulated).
    #[serde(default = "zero")]
    pub flock_radius: Scalar,
    /// How hard a particle steers towards its neighbours' middle.
    #[serde(default = "zero")]
    pub cohesion: Scalar,
    /// How hard it keeps clear of neighbours that come too near.
    #[serde(default = "zero")]
    pub separation: Scalar,
    /// How much it takes up its neighbours' heading and speed.
    #[serde(default = "zero")]
    pub alignment: Scalar,
    /// Fastest a flocking particle goes (0: no limit).
    #[serde(default = "zero")]
    pub top_speed: Scalar,
    /// A 3D object of the connected scene whose surface particles are born on, in place
    /// of the box (none: the box).
    #[serde(default)]
    pub emit_from: Option<crate::NodeId>,
    /// Particles bounce off the connected scene's solids, coming back with bounce of the
    /// speed they hit them at.
    #[serde(default)]
    pub collide: bool,
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
fn burst_speed() -> Scalar {
    200.0.into()
}
fn all_round() -> Scalar {
    360.0.into()
}
fn ambient() -> Scalar {
    0.2.into()
}
fn scatter() -> Scalar {
    0.4.into()
}
fn link_opacity() -> Scalar {
    0.6.into()
}
fn burst_size() -> Scalar {
    0.6.into()
}
fn white() -> [f32; 4] {
    [1.0; 4]
}
pub static PARTICLES: NodeType = NodeType {
    type_id: "particles",
    version: 1,
    label: "Particles",
    description: "Soft dots born over time and moved by a launch speed, gravity, wind and swirling turbulence, on a transparent frame. With emitter connected, particles are born where that image has alpha (text, a shape, a matte), density of them a second for every 100 x 100 pixels of it; otherwise rate a second are born inside the box of width x height centred on x, y (0 x 0 is a point). Each lives life seconds, launched at speed along direction (degrees, 0 right, 90 up) within spread degrees, then pulled by gravity (down) and wind (right) in pixels per second squared and pushed about by turbulence (pixels per second, swirls about turbulence_scale pixels across, never converging or thinning the cloud). Size is the dot's radius; colour runs to end_colour over the life (linear RGBA, values above 1 glow), fading out over the last fade share of it. Glow 1 adds light where dots overlap, 0 lets them cover one another. Variation makes speeds, sizes and lives differ. Emission begins at start seconds and lasts duration seconds (0 is endless; a short duration is a burst). Inherit tints each particle with the colour of the emitter where it was born (1 is fully the picture's own colour, times colour), so an image breaks up into dust of itself. With target connected, each particle is given a place in that image's alpha and steered onto it over the last homing share of its life (1 is the whole life, 0 never), arriving exactly as it dies and, with inherit, taking the target's colour on the way; particles that find no place are not born. Streak draws each particle as a trail back along where it was streak seconds earlier (at most 96 pixels), for sparks and rain. Field is an image that pushes particles: field_push accelerates them from its bright parts towards its dark parts (negative draws them in) and field_flow along its edges, so they stream round a shape. Pull draws them to the point pull_x, pull_y (negative drives them away) and vortex swings them round it. Obstacle is a matte they bounce off, keeping bounce of their speed. Sprite is an image drawn as each particle in place of a dot (the whole frame of that input, size wide either side, tinted by the colours), turning at spin degrees a second; grow is the size at the end of the life as a factor of the size at birth. Colour_variation turns each particle's hue by its own random amount (1 is anywhere round the colour wheel), for confetti. Particles live in the 3D scenes' space: the frame at depth 0, in pixels, z towards the viewer. Depth spreads their births that many pixels in front of and behind the frame, lets the launch spread and the turbulence carry them through depth, and draws near ones larger. They are filmed by the camera numbered camera in the scene connected to scene (as a Render scene node counts them; 0, or no scene, is the view that sees the frame exactly), so they move with a 3D or solved camera and show parallax. Every particle is recomputed from the frame's time alone, so frames render in any order and identically. Keyed settings are followed: a particle is launched with the settings in force when it was born, and forces act as keyed while it flies, sampled 16 times across the last life; target, field, obstacle and sprite images are read as they are at the frame, and so is the emitter unless history is set: then the emitter is rendered at that many earlier moments across the last life (4, 9 or 16 are used, on whole frames, at a half, a third or a quarter of the size) and each particle is born on the emitter as it was nearest its birth, so moving text or footage leaves its particles behind. That costs as many extra renders of the emitter's branch for every frame. At most 2000000 particles from a box. Connect a Render scene with Depth on (same scene and camera) to Depth and 3D objects hide the particles behind them; the scene camera's depth of field blurs particles out of focus. In depth: Z places the box in front of (positive) or behind the frame, Towards launches particles at the viewer (away when negative), and Reach is how far from the camera they are drawn, fading out over its last quarter (0: any distance), so a volume of stars to fly through shows no far end. Shutter is an exposure time in seconds: with a scene connected, the camera's move during it streaks each particle, so a fast flight draws stars as streaks. Particles exist from Start onwards: set Start to minus their Life to have a full cloud at the first frame. Life may be up to an hour, so a field of stars can simply stand (rate times life is still at most 2,000,000 particles). In a scene, Points keeps every particle the same size on screen at any distance, as stars are points however near, and In focus keeps the camera's depth of field from blurring them. Burst sets off that many particles from each particle as it ends (fireworks, sparks): thrown at burst_speed within burst_spread degrees round the way it was going (360 all round), carrying on its speed, living burst_life seconds, burst_size times the size, in burst_colour, moved by the same forces. With burst_on_impact the obstacle sets them off instead: a particle ends where it first touches the obstacle and its burst is thrown back off it (splashes, sparks off a surface). Burst particles are computed from the frame's time like the others. Trail draws a ribbon along the path each particle actually took, trail seconds back, unbroken and narrowing and fading towards its tail, trail_width times the size wide (a streak is a straight row of dots instead); bursts draw trails too. Trails are drawn for dots, not sprites. Link_distance joins particles nearer each other than that many pixels on the frame with lines, link_width pixels wide, the stronger the nearer (link_opacity at their nearest), in the two particles' colours: a network that grows and breaks as they move. Lines join the first 8,192 particles of a system (bursts are not joined). Over-life curves shape each particle from birth to end: size_over_life (a factor of the size, in place of grow), opacity_over_life (in place of the fade), speed_over_life (a factor of how fast it travels) and colour_over_life (eight colours in place of colour to end_colour), each eight values at even steps of the life joined by straight lines; null leaves the plain settings. Bursts follow the size and opacity curves. Lit makes particles take their light from the 3D scene connected to scene (1: wholly; 0: they glow by themselves): each is lit by up to six of its lights as a lit 3D surface there would be (sun, point and spot lights with their falloff and cones, area lights from their centre), plus ambient light everywhere, and scatter makes them brighter looking into a light (smoke and dust in a sunbeam; negative: brighter with the light behind you). The scene's solids shadow lit particles from each of those lights that has Shadows on, as they shadow one another (a beam through dust is cut by an object in it); cards and shadow catchers cast none. Emit_from names a 3D object of the connected scene (a shape, a 3D text, an image card) whose surface particles are born on in place of the box, spread evenly over it and launched straight out of it at speed, within spread degrees of its outward direction; each is born where the surface was at its birth, so a moving object leaves its particles behind. Imported models cannot emit yet. Collide makes particles bounce off the solids of the connected scene (its first 16 shapes, 3D texts, cards and models; shadow catchers are not solid), coming back off them with bounce of the speed they hit them at and sliding along them, and with burst_on_impact a solid sets off their bursts as the obstacle does; solids that move push particles along. Shapes are met exactly, a 3D text by its letters' outline as a grid of up to 128 cells across (coarser in a small frame), a model by its box. Forces takes a chain of Force nodes (attract, vortex, wind, turbulence, drag, each with its own place, reach and keys), up to 8 of them acting on top of the settings here. Flock_radius above 0 makes the particles a flock: each steers by its neighbours within that many pixels, towards their middle (cohesion, pixels per second squared at that distance), clear of those that come too near (separation, strongest when touching) and into their heading and speed (alignment, per second), going no faster than top_speed (0: no limit). A flock cannot be worked out from the frame's time alone, so it is simulated: stepped forward from its start in steps of a 60th of a second, keeping where every particle is once a second, so a frame is found from the nearest second before it and playing on costs two steps a frame; any frame still renders identically. A flock is born in the box or on a 3D object (not on an emitter image), has no target, field, obstacle, bursts, streaks or trails, and holds at most 65,536 particles (fewer in a frame smaller than 362 x 362).",
    inputs: &[
        "emitter", "target", "field", "obstacle", "sprite", "scene", "depth", "forces",
    ],
    optional: &[
        "emitter", "target", "field", "obstacle", "sprite", "scene", "depth", "forces",
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
        scalar("burst", "particles", 0.0, 64.0, 0.0),
        scalar("burst_speed", "pixels per second", 0.0, 8192.0, 200.0),
        scalar("burst_spread", "degrees", 0.0, 360.0, 360.0),
        scalar("burst_life", "seconds", 0.05, 60.0, 1.0),
        scalar("burst_size", "factor", 0.0, 8.0, 0.6),
        scalar("trail", "seconds", 0.0, 10.0, 0.0),
        scalar("trail_width", "factor", 0.0, 4.0, 1.0),
        scalar("link_distance", "pixels", 0.0, 4096.0, 0.0),
        scalar("link_opacity", "factor", 0.0, 1.0, 0.6),
        scalar("link_width", "pixels", 0.25, 8.0, 1.0),
        scalar("lit", "factor", 0.0, 1.0, 0.0),
        scalar("ambient", "factor", 0.0, 4.0, 0.2),
        scalar("scatter", "factor", -0.95, 0.95, 0.4),
        scalar("flock_radius", "pixels", 0.0, 2000.0, 0.0),
        scalar("cohesion", "pixels per second squared", 0.0, 100_000.0, 0.0),
        scalar(
            "separation",
            "pixels per second squared",
            0.0,
            100_000.0,
            0.0,
        ),
        scalar("alignment", "per second", 0.0, 100.0, 0.0),
        scalar("top_speed", "pixels per second", 0.0, 100_000.0, 0.0),
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
        Parameter {
            default: 1.0,
            ..other("burst_colour", ParameterKind::Color, "linear RGBA")
        },
        other(
            "burst_on_impact",
            ParameterKind::Boolean,
            "set off by the obstacle",
        ),
        other("size_over_life", ParameterKind::LifeCurve, "factor"),
        other("opacity_over_life", ParameterKind::LifeCurve, "factor"),
        other("speed_over_life", ParameterKind::LifeCurve, "factor"),
        other(
            "colour_over_life",
            ParameterKind::LifeColours,
            "linear RGBA",
        ),
        other("emit_from", ParameterKind::NodeRef, "3D object"),
        other(
            "collide",
            ParameterKind::Boolean,
            "bounce off the scene's solids",
        ),
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
        burst,
        burst_speed,
        burst_spread,
        burst_life,
        burst_size,
        trail,
        trail_width,
        link_distance,
        link_opacity,
        link_width,
        lit,
        ambient,
        scatter,
        start,
        duration,
        x,
        y,
        width,
        height,
        flock_radius,
        cohesion,
        separation,
        alignment,
        top_speed
    ],
    {
        fn validate(&self) -> Result<()> {
            if self
                .colour
                .iter()
                .chain(&self.end_colour)
                .chain(&self.burst_colour)
                .any(|v| !v.is_finite() || !(0.0..=64.0).contains(v))
            {
                return fail("Particle colours need values in 0..=64");
            }
            for curve in [
                &self.size_over_life,
                &self.opacity_over_life,
                &self.speed_over_life,
            ]
            .into_iter()
            .flatten()
            {
                if curve
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=8.0).contains(v))
                {
                    return fail("A curve over the life needs values in 0..=8");
                }
            }
            if self.colour_over_life.is_some_and(|ramp| {
                ramp.iter()
                    .flatten()
                    .any(|v| !v.is_finite() || !(0.0..=64.0).contains(v))
            }) {
                return fail("Colours over the life need values in 0..=64");
            }
            if self.history > 16 {
                return fail("Particles keep at most 16 moments of the emitter");
            }
            Ok(())
        }
    }
);
