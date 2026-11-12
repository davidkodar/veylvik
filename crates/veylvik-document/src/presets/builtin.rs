//! Editable starter effects; the library and automation use the same factories.
use super::Preset;
mod extended;
use crate::{
    BlendMode, Composition, Key, MaskChannel, Node, NodeId, Operation, Result, Scalar, Time,
    groups::{Control, Group},
};
#[derive(Clone, Copy)]
pub struct Builtin {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}
pub const ALL: [Builtin; 28] = [
    Builtin {
        id: "prismatic_glow",
        label: "Prismatic Glow",
        description: "Cyan and rose light spill with a subtle chromatic offset. Best on bright details against darkness.",
    },
    Builtin {
        id: "neon_outline",
        label: "Neon Outline",
        description: "Turn luminance edges into luminous cyan contours with a soft halo.",
    },
    Builtin {
        id: "liquid_shimmer",
        label: "Liquid Shimmer",
        description: "Animated flowing distortion for heat haze, liquid titles and drifting light.",
    },
    Builtin {
        id: "soft_bloom",
        label: "Soft Bloom",
        description: "A broad luminous bloom around bright details, with adjustable energy and softness.",
    },
    Builtin {
        id: "anamorphic_streaks",
        label: "Anamorphic Streaks",
        description: "Long horizontal light streaks from bright details. Adjust width, tint and strength.",
    },
    Builtin {
        id: "dream_focus",
        label: "Dream Focus",
        description: "A soft screened image creates a dreamy diffusion while retaining the original detail.",
    },
    Builtin {
        id: "edge_engrave",
        label: "Edge Engrave",
        description: "Directional luminance edges etched into a dark cyan drawing.",
    },
    Builtin {
        id: "chromatic_echo",
        label: "Chromatic Echo",
        description: "Opposing cyan and rose echoes drift around the source image.",
    },
    Builtin {
        id: "radial_echo",
        label: "Radial Echo",
        description: "Expanding image copies form a rhythmic graphic tunnel.",
    },
    Builtin {
        id: "duotone_print",
        label: "Duotone Print",
        description: "Map luminance into a bold ink-and-paper palette with an adjustable mix.",
    },
    Builtin {
        id: "silver_relief",
        label: "Silver Relief",
        description: "Directional edge contrast becomes a metallic relief over the source.",
    },
    Builtin {
        id: "ink_bleed",
        label: "Ink Bleed",
        description: "Luminance spreads into a soft ink silhouette, with adjustable spread and softness.",
    },
    Builtin {
        id: "light_sweep",
        label: "Light Sweep",
        description: "A moving shaft of light sweeps across the image and returns in a four-second loop.",
    },
    Builtin {
        id: "soft_vignette",
        label: "Soft Vignette",
        description: "Feathered elliptical shading focuses attention toward the center.",
    },
    Builtin {
        id: "film_grain",
        label: "Film Grain",
        description: "Fine animated procedural grain, mixed gently over the source.",
    },
    Builtin {
        id: "frosted_glass",
        label: "Frosted Glass",
        description: "Fine refraction and diffusion scatter detail like textured glass.",
    },
    Builtin {
        id: "heat_haze",
        label: "Heat Haze",
        description: "Tall, flowing refraction creates a protected-edge heat shimmer.",
    },
    Builtin {
        id: "underwater",
        label: "Underwater",
        description: "Slow broad refraction and a blue-green tint suggest submerged motion.",
    },
    Builtin {
        id: "scanlines",
        label: "Scanlines",
        description: "Fine horizontal lines give imagery a graphic screen texture.",
    },
    Builtin {
        id: "hologram",
        label: "Hologram",
        description: "Cyan remapping, scanning lines and a drifting ghost image build a holographic look.",
    },
    Builtin {
        id: "glitch_slices",
        label: "Glitch Slices",
        description: "Horizontal slices shift sideways over the original image in a repeatable digital glitch.",
    },
    Builtin {
        id: "orbit_echo",
        label: "Orbit Echo",
        description: "Two ghost images orbit the source with independent warm and cool tints.",
    },
    Builtin {
        id: "spotlight",
        label: "Spotlight",
        description: "A soft pool of light travels around the composition, revealing the source beneath.",
    },
    Builtin {
        id: "dust_motes",
        label: "Dust Motes",
        description: "Soft floating dust with staggered fades, editable paths, tint and intensity.",
    },
    Builtin {
        id: "fireflies",
        label: "Fireflies",
        description: "Warm glowing points wander and pulse on independent looping paths.",
    },
    Builtin {
        id: "embers",
        label: "Embers",
        description: "Orange sparks rise and fade, with staggered lifetimes and a soft glow.",
    },
    Builtin {
        id: "snowfall",
        label: "Snowfall",
        description: "Layered white flakes fall and drift with staggered looping lifetimes.",
    },
    Builtin {
        id: "starfield",
        label: "Starfield",
        description: "Twinkling points radiate from the center in a looping star flight.",
    },
];
struct Builder {
    doc: Composition,
    members: std::collections::BTreeSet<NodeId>,
    controls: Vec<(String, Control)>,
}
impl Builder {
    fn add(&mut self, label: &str, operation: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
        let node = Node::new(label, operation, inputs);
        let id = node.id;
        self.members.insert(id);
        self.doc.nodes.insert(id, node);
        id
    }
    fn expose(&mut self, id: NodeId, parameter: &str, label: &str) -> Result<()> {
        let control = Control::new(&self.doc, id, parameter, label)?;
        self.controls
            .push((label.to_lowercase().replace(' ', "_"), control));
        Ok(())
    }
}
pub fn build(id: &str) -> Result<Preset> {
    let builtin = ALL
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| crate::Error("unknown starter effect".into()))?;
    let mut doc = crate::blank(960, 540, Time::new(24, 1)?)?;
    doc.frame_count = 96;
    let source = doc.output;
    let mut b = Builder {
        doc,
        members: Default::default(),
        controls: Vec::new(),
    };
    let output = match id {
        "prismatic_glow" => {
            let blur = b.add(
                "Light spread",
                Operation::Blur {
                    radius: 12.0.into(),
                },
                &[("image", source)],
            );
            let cyan = b.add(
                "Cyan light",
                Operation::Colorize {
                    low: [0.0; 4],
                    high: [0.25, 0.9, 1.8, 0.7],
                },
                &[("image", blur)],
            );
            let rose = b.add(
                "Rose light",
                Operation::Colorize {
                    low: [0.0; 4],
                    high: [1.5, 0.15, 0.55, 0.55],
                },
                &[("image", blur)],
            );
            let offset = b.add(
                "Color separation",
                Operation::Transform {
                    x: 10.0.into(),
                    y: (-3.0).into(),
                },
                &[("image", rose)],
            );
            let first = b.add(
                "Cyan spill",
                Operation::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.5.into(),
                },
                &[("background", source), ("foreground", cyan)],
            );
            let output = b.add(
                "Prismatic light",
                Operation::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.5.into(),
                },
                &[("background", first), ("foreground", offset)],
            );
            b.expose(blur, "radius", "Spread")?;
            b.expose(first, "opacity", "Cyan intensity")?;
            b.expose(output, "opacity", "Rose intensity")?;
            b.expose(offset, "x", "Separation")?;
            output
        }
        "neon_outline" => {
            let shifted = b.add(
                "Edge sampling",
                Operation::Transform {
                    x: 3.0.into(),
                    y: 3.0.into(),
                },
                &[("image", source)],
            );
            let difference = b.add(
                "Edge contrast",
                Operation::Blend {
                    mode: BlendMode::Difference,
                    opacity: 1.0.into(),
                },
                &[("background", source), ("foreground", shifted)],
            );
            let valid = b.add(
                "Valid edge samples",
                Operation::Mask {},
                &[("image", difference), ("mask", shifted)],
            );
            let mask = b.add(
                "Luminance edges",
                Operation::ChannelMask {
                    channel: MaskChannel::Luminance,
                },
                &[("image", valid)],
            );
            let grow = b.add(
                "Edge width",
                Operation::Morphology { radius: 1.0.into() },
                &[("image", mask)],
            );
            let edge = grow;
            let color = b.add(
                "Neon color",
                Operation::Solid {
                    color: [0.1, 1.2, 2.0, 1.0],
                },
                &[],
            );
            let core = b.add(
                "Neon core",
                Operation::Mask {},
                &[("image", color), ("mask", edge)],
            );
            let halo = b.add(
                "Halo spread",
                Operation::Blur { radius: 8.0.into() },
                &[("image", core)],
            );
            let boost = b.add(
                "Halo energy",
                Operation::Exposure { stops: 2.0.into() },
                &[("image", halo)],
            );
            let first = b.add(
                "Neon halo",
                Operation::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.75.into(),
                },
                &[("background", source), ("foreground", boost)],
            );
            let output = b.add(
                "Neon contours",
                Operation::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.8.into(),
                },
                &[("background", first), ("foreground", core)],
            );
            b.expose(grow, "radius", "Width")?;
            b.expose(color, "color", "Color")?;
            b.expose(halo, "radius", "Halo spread")?;
            b.expose(boost, "stops", "Halo energy")?;
            b.expose(output, "opacity", "Edge intensity")?;
            output
        }
        "liquid_shimmer" => {
            let flow = b.add(
                "Flow field",
                Operation::FractalNoise {
                    seed: 47,
                    scale: 60.0.into(),
                    octaves: 3.0.into(),
                    roughness: 0.55.into(),
                    evolution: Scalar::Smooth {
                        keys: vec![
                            Key {
                                time: Time::new(0, 1)?,
                                value: 0.0,
                                segment: None,
                            },
                            Key {
                                time: Time::new(2, 1)?,
                                value: 0.8,
                                segment: None,
                            },
                            Key {
                                time: Time::new(4, 1)?,
                                value: 0.0,
                                segment: None,
                            },
                        ],
                    },
                },
                &[],
            );
            // Attenuate the vector field at the canvas boundary at any resolution.
            // This avoids transparent slivers without cropping or scaling the image.
            let canvas = b.add("Frame coverage", Operation::Solid { color: [1.0; 4] }, &[]);
            let inset = b.add(
                "Protected edges",
                Operation::Morphology {
                    radius: (-16.0).into(),
                },
                &[("image", canvas)],
            );
            let feather = b.add(
                "Edge transition",
                Operation::Blur { radius: 4.0.into() },
                &[("image", inset)],
            );
            let field = b.add(
                "Bounded flow",
                Operation::Mask {},
                &[("image", flow), ("mask", feather)],
            );
            let output = b.add(
                "Liquid refraction",
                Operation::Displace {
                    x: 12.0.into(),
                    y: 6.0.into(),
                },
                &[("image", source), ("map", field)],
            );
            b.expose(output, "x", "Horizontal shimmer")?;
            b.expose(output, "y", "Vertical shimmer")?;
            b.expose(flow, "scale", "Flow size")?;
            b.expose(flow, "evolution", "Flow animation")?;
            output
        }
        _ => extended::build(id, &mut b, source)?,
    };
    let mut group = Group::capture(&b.doc, b.members, output, builtin.label)?;
    group.controls = b.controls.into_iter().collect();
    for input in group.inputs.values_mut() {
        input.label = "Image".into();
    }
    let group_id = group.id;
    b.doc.groups.insert(group_id, group);
    Preset::capture(&b.doc, group_id)
}

pub const CATEGORIES: [&str; 6] = [
    "Light",
    "Surface",
    "Distortion",
    "Motion",
    "Particles",
    "All",
];
pub fn category(id: &str) -> &'static str {
    match id {
        "soft_bloom" | "anamorphic_streaks" | "dream_focus" | "light_sweep" | "soft_vignette"
        | "spotlight" | "prismatic_glow" | "neon_outline" => "Light",
        "edge_engrave" | "duotone_print" | "silver_relief" | "ink_bleed" | "film_grain"
        | "scanlines" | "hologram" => "Surface",
        "frosted_glass" | "heat_haze" | "underwater" | "glitch_slices" | "liquid_shimmer" => {
            "Distortion"
        }
        "chromatic_echo" | "radial_echo" | "orbit_echo" => "Motion",
        "dust_motes" | "fireflies" | "embers" | "snowfall" | "starfield" => "Particles",
        _ => "All",
    }
}
