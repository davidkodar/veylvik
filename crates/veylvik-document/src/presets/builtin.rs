//! Editable starter effects; the library and automation use the same factories.
use super::Preset;
mod appearance;
mod extended;
mod optics;
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
pub const ALL: [Builtin; 61] = [
    Builtin {
        id: "surface_paint",
        label: "Paint on Surface",
        description: "Blend a layer into a real surface with lighting, texture, dark-crack wear, subtle distortion and grain. Bind the clean surface and foreground separately.",
    },
    Builtin {
        id: "oak_light",
        label: "Light Oak",
        description: "Pale oak with flowing growth rings, open pores and fine longitudinal fibres.",
    },
    Builtin {
        id: "oak_dark",
        label: "Dark Oak",
        description: "Deep stained oak with warm growth rings and open grain.",
    },
    Builtin {
        id: "marble",
        label: "White Marble",
        description: "Polished ivory stone with branching grey mineral veins.",
    },
    Builtin {
        id: "brick_light",
        label: "Light Brick",
        description: "Cream masonry with recessed mortar, porous faces and individual brick variation.",
    },
    Builtin {
        id: "brick_red",
        label: "Red Brick",
        description: "Warm red and brown fired brick with recessed mortar and rough faces.",
    },
    Builtin {
        id: "concrete",
        label: "Rough Concrete",
        description: "Uneven concrete with aggregate, pits and directional relief shading.",
    },
    Builtin {
        id: "paper_clean",
        label: "Clean Paper",
        description: "Soft white paper with fine fibres and a matte surface.",
    },
    Builtin {
        id: "paper_aged",
        label: "Weathered Paper",
        description: "Warm aged paper with mottled stains, fibres and worn creases.",
    },
    Builtin {
        id: "leather_red",
        label: "Red Leather",
        description: "Deep red leather with irregular pebbled grain and soft highlights.",
    },
    Builtin {
        id: "leather_brown",
        label: "Brown Leather",
        description: "Warm brown leather with pebbled grain and fine creases.",
    },
    Builtin {
        id: "leather_tan",
        label: "Tan Leather",
        description: "Light saddle leather with tactile grain and soft highlights.",
    },
    Builtin {
        id: "leather_dark",
        label: "Dark Leather",
        description: "Dark chocolate leather with irregular pores and creases.",
    },
    Builtin {
        id: "leather_black",
        label: "Black Leather",
        description: "Black leather with subtly raised grain and satin highlights.",
    },
    Builtin {
        id: "surface_reflection",
        label: "Surface Reflection",
        description: "Reflect animated clouds, another object or an image in a metallic surface. Bind the surface and reflection scene separately.",
    },
    Builtin {
        id: "wet_drips",
        label: "Wet Ink Drips",
        description: "Wet coloured liquid grows and drips from any text or cutout silhouette. Adjustable colour, drop size, spacing and surface roughness.",
    },
    Builtin {
        id: "water_drips",
        label: "Clear Water Drips",
        description: "Clear water drips from a text or cutout silhouette, refracting an explicitly connected background with wet surface reflections.",
    },
    Builtin {
        id: "clear_glass",
        label: "Clear Glass Material",
        description: "Clear beveled glass with background refraction, colour dispersion and an independently connected reflection scene.",
    },
    Builtin {
        id: "frosted_glass_material",
        label: "Frosted Glass Material",
        description: "Diffused glass with softened background details, refraction and reflected light.",
    },
    Builtin {
        id: "sun_glints",
        label: "Sun Glints",
        description: "Tiny animated sun-mirror flashes on bright polished details. Adjust density, size and highlight threshold.",
    },
    Builtin {
        id: "metal_patina",
        label: "Metal Patina",
        description: "Optional uneven tarnish, dirt and scratches with cleaner worn edges. Open the node for verdigris or rust variants.",
    },
    Builtin {
        id: "gold_text",
        label: "Gold",
        description: "Rich gold with studio reflections, fine surface grain and beveled edges.",
    },
    Builtin {
        id: "silver_text",
        label: "Silver",
        description: "Cool silver with bright reflections and polished bevels.",
    },
    Builtin {
        id: "aluminium_text",
        label: "Aluminium",
        description: "Satin aluminium with directional brushed grain and broad reflections.",
    },
    Builtin {
        id: "bronze_text",
        label: "Bronze",
        description: "Warm bronze reflections with subtle mottled oxidation.",
    },
    Builtin {
        id: "phosphor_text",
        label: "Phosphor",
        description: "Emissive green phosphor granules with a soft luminous halo.",
    },
    Builtin {
        id: "wood_text",
        label: "Carved Wood",
        description: "Warm wood with flowing growth rings, pores and carved edges.",
    },
    Builtin {
        id: "light_smoke",
        label: "Light Smoke",
        description: "Pale turbulent rising smoke with translucent wisps and directional shading.",
    },
    Builtin {
        id: "dense_smoke",
        label: "Dense Dark Smoke",
        description: "Thick soot-dark billows inspired by oil or rubber smoke.",
    },
    Builtin {
        id: "fog_layer",
        label: "Fog Layer",
        description: "A translucent low fog layer with evolving density.",
    },
    Builtin {
        id: "drifting_fog",
        label: "Drifting Fog",
        description: "A floating fog bank with flowing turbulence and lateral drift.",
    },
    Builtin {
        id: "clouds",
        label: "Clouds",
        description: "Soft billowing cloud volume with sunlit edges and shaded interior.",
    },
    Builtin {
        id: "metallic_shine",
        label: "Metallic Shine",
        description: "A crisp diagonal light reflection sweeps across isolated text or logos. Apply before the background; keeps transparency intact. Four-second pass with editable motion, width, angle and brightness.",
    },
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
        "surface_paint" => {
            let background = Node::new(
                "Clean surface",
                Operation::Solid(crate::nodes::Solid {
                    color: [0.5, 0.5, 0.5, 1.],
                }),
                &[],
            );
            let plate = background.id;
            b.doc.nodes.insert(plate, background);
            let node = b.add(
                "Paint integrated with surface",
                crate::surface::Look::Weathered.operation(),
                &[("background", plate), ("foreground", source)],
            );
            for (name, label) in [
                ("opacity", "Opacity"),
                ("lighting", "Surface lighting"),
                ("detail", "Surface texture"),
                ("wear", "Wear / cracks"),
                ("threshold", "Dark-recess threshold"),
                ("distortion", "Surface distortion"),
                ("softness", "Match softness"),
                ("grain", "Match grain"),
            ] {
                b.expose(node, name, label)?;
            }
            node
        }
        id if appearance::material_kind(id).is_some() => appearance::build(id, &mut b, source)?,
        "wet_drips"
        | "water_drips"
        | "surface_reflection"
        | "clear_glass"
        | "frosted_glass_material"
        | "sun_glints"
        | "metal_patina" => optics::build(id, &mut b, source)?,
        "metallic_shine" => {
            let node = b.add(
                "Travelling specular reflection",
                metallic_shine(),
                &[("image", source)],
            );
            for (parameter, label) in [
                ("x", "Sweep position"),
                ("y", "Vertical center"),
                ("width", "Shine width"),
                ("angle", "Shine angle"),
                ("intensity", "Brightness"),
            ] {
                b.expose(node, parameter, label)?;
            }
            node
        }
        "prismatic_glow" => {
            let blur = b.add(
                "Light spread",
                Operation::Blur(crate::nodes::Blur {
                    radius: 12.0.into(),
                }),
                &[("image", source)],
            );
            let cyan = b.add(
                "Cyan light",
                Operation::Colorize(crate::nodes::Colorize {
                    low: [0.0; 4],
                    high: [0.25, 0.9, 1.8, 0.7],
                }),
                &[("image", blur)],
            );
            let rose = b.add(
                "Rose light",
                Operation::Colorize(crate::nodes::Colorize {
                    low: [0.0; 4],
                    high: [1.5, 0.15, 0.55, 0.55],
                }),
                &[("image", blur)],
            );
            let offset = b.add(
                "Color separation",
                Operation::Transform(crate::nodes::Transform {
                    x: 10.0.into(),
                    y: (-3.0).into(),
                }),
                &[("image", rose)],
            );
            let first = b.add(
                "Cyan spill",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.5.into(),
                }),
                &[("background", source), ("foreground", cyan)],
            );
            let output = b.add(
                "Prismatic light",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.5.into(),
                }),
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
                Operation::Transform(crate::nodes::Transform {
                    x: 3.0.into(),
                    y: 3.0.into(),
                }),
                &[("image", source)],
            );
            let difference = b.add(
                "Edge contrast",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Difference,
                    opacity: 1.0.into(),
                }),
                &[("background", source), ("foreground", shifted)],
            );
            let valid = b.add(
                "Valid edge samples",
                Operation::Mask(crate::nodes::Mask {}),
                &[("image", difference), ("mask", shifted)],
            );
            let mask = b.add(
                "Luminance edges",
                Operation::ChannelMask(crate::nodes::ChannelMask {
                    channel: MaskChannel::Luminance,
                }),
                &[("image", valid)],
            );
            let grow = b.add(
                "Edge width",
                Operation::Morphology(crate::nodes::Morphology { radius: 1.0.into() }),
                &[("image", mask)],
            );
            let edge = grow;
            let color = b.add(
                "Neon color",
                Operation::Solid(crate::nodes::Solid {
                    color: [0.1, 1.2, 2.0, 1.0],
                }),
                &[],
            );
            let core = b.add(
                "Neon core",
                Operation::Mask(crate::nodes::Mask {}),
                &[("image", color), ("mask", edge)],
            );
            let halo = b.add(
                "Halo spread",
                Operation::Blur(crate::nodes::Blur { radius: 8.0.into() }),
                &[("image", core)],
            );
            let boost = b.add(
                "Halo energy",
                Operation::Exposure(crate::nodes::Exposure { stops: 2.0.into() }),
                &[("image", halo)],
            );
            let first = b.add(
                "Neon halo",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.75.into(),
                }),
                &[("background", source), ("foreground", boost)],
            );
            let output = b.add(
                "Neon contours",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.8.into(),
                }),
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
                Operation::FractalNoise(crate::nodes::FractalNoise {
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
                }),
                &[],
            );
            // Attenuate the vector field at the canvas boundary at any resolution.
            // This avoids transparent slivers without cropping or scaling the image.
            let canvas = b.add(
                "Frame coverage",
                Operation::Solid(crate::nodes::Solid { color: [1.0; 4] }),
                &[],
            );
            let inset = b.add(
                "Protected edges",
                Operation::Morphology(crate::nodes::Morphology {
                    radius: (-16.0).into(),
                }),
                &[("image", canvas)],
            );
            let feather = b.add(
                "Edge transition",
                Operation::Blur(crate::nodes::Blur { radius: 4.0.into() }),
                &[("image", inset)],
            );
            let field = b.add(
                "Bounded flow",
                Operation::Mask(crate::nodes::Mask {}),
                &[("image", flow), ("mask", feather)],
            );
            let output = b.add(
                "Liquid refraction",
                Operation::Displace(crate::nodes::Displace {
                    x: 12.0.into(),
                    y: 6.0.into(),
                }),
                &[("image", source), ("map", field)],
            );
            b.expose(output, "x", "Horizontal shimmer")?;
            b.expose(output, "y", "Vertical shimmer")?;
            b.expose(flow, "scale", "Flow size")?;
            b.expose(flow, "evolution", "Flow animation")?;
            output
        }
        "gold_text" | "silver_text" | "aluminium_text" | "bronze_text" | "phosphor_text"
        | "wood_text" | "light_smoke" | "dense_smoke" | "fog_layer" | "drifting_fog" | "clouds" => {
            appearance::build(id, &mut b, source)?
        }
        _ => extended::build(id, &mut b, source)?,
    };
    let mut group = Group::capture(&b.doc, b.members, output, builtin.label)?;
    group.controls = b.controls.into_iter().collect();
    for input in group.inputs.values_mut() {
        input.label = match input.targets.first().map(|target| target.port.as_str()) {
            Some("background") if id == "surface_paint" => "Clean surface",
            Some("foreground") if id == "surface_paint" => "Text / image / effect",
            Some("reflection") => "Reflection scene",
            Some("background")
                if matches!(id, "clear_glass" | "frosted_glass_material" | "water_drips") =>
            {
                "Background to refract"
            }
            Some("image")
                if matches!(id, "clear_glass" | "frosted_glass_material" | "water_drips") =>
            {
                "Shape / text"
            }
            _ => "Image",
        }
        .into();
    }
    // Multi-input effects get role names so automation can bind them without reading
    // the graph; single-input effects keep `input_1` for existing scripts.
    if group.inputs.len() > 1 {
        let inputs = std::mem::take(&mut group.inputs);
        for (key, input) in inputs {
            let role = match input.targets.first().map(|target| target.port.as_str()) {
                Some("background") if id == "surface_paint" => "surface",
                Some("foreground") if id == "surface_paint" => "layer",
                Some("background") => "background",
                Some("reflection") => "reflection",
                Some("image") => "shape",
                _ => key.as_str(),
            };
            if group.inputs.insert(role.to_owned(), input).is_some() {
                return Err(crate::Error(format!("{id}: duplicate input role {role}")));
            }
        }
    }
    let group_id = group.id;
    b.doc.groups.insert(group_id, group);
    Preset::capture(&b.doc, group_id)
}

pub const CATEGORIES: [&str; 8] = [
    "Light",
    "Surface",
    "Distortion",
    "Motion",
    "Particles",
    "Materials",
    "Atmosphere",
    "All",
];
pub fn category(id: &str) -> &'static str {
    match id {
        id if appearance::material_kind(id).is_some() => "Materials",
        "sun_glints" | "metallic_shine" | "soft_bloom" | "anamorphic_streaks" | "dream_focus"
        | "light_sweep" | "soft_vignette" | "spotlight" | "prismatic_glow" | "neon_outline" => {
            "Light"
        }
        "surface_paint" | "edge_engrave" | "duotone_print" | "silver_relief" | "ink_bleed"
        | "film_grain" | "scanlines" | "hologram" => "Surface",
        "frosted_glass" | "heat_haze" | "underwater" | "glitch_slices" | "liquid_shimmer" => {
            "Distortion"
        }
        "chromatic_echo" | "radial_echo" | "orbit_echo" => "Motion",
        "dust_motes" | "fireflies" | "embers" | "snowfall" | "starfield" => "Particles",
        "gold_text" | "silver_text" | "aluminium_text" | "bronze_text" | "phosphor_text"
        | "wood_text" => "Materials",
        "light_smoke" | "dense_smoke" | "fog_layer" | "drifting_fog" | "clouds" => "Atmosphere",
        "wet_drips"
        | "water_drips"
        | "surface_reflection"
        | "clear_glass"
        | "frosted_glass_material"
        | "metal_patina" => "Materials",
        _ => "All",
    }
}

/// Authored one-way pass: both endpoints are outside the canvas for a clean loop.
pub(crate) fn metallic_shine() -> Operation {
    Operation::MetallicShine(crate::nodes::MetallicShine {
        x: Scalar::Linear {
            keys: [(0, -480.0), (4, 1440.0)]
                .into_iter()
                .map(|(ticks, value)| Key {
                    time: Time { ticks, rate: 1 },
                    value,
                    segment: None,
                })
                .collect(),
        },
        y: 270.0.into(),
        width: 90.0.into(),
        angle: 20.0.into(),
        intensity: 1.8.into(),
    })
}

pub(crate) fn sun_glints() -> Operation {
    Operation::SunGlints(crate::nodes::SunGlints {
        seed: 193,
        size: 18.0.into(),
        intensity: 3.0.into(),
        threshold: 0.55.into(),
        evolution: Scalar::Linear {
            keys: [(0, 0.0), (4, 4.0)]
                .into_iter()
                .map(|(ticks, value)| Key {
                    time: Time { ticks, rate: 1 },
                    value,
                    segment: None,
                })
                .collect(),
        },
        density: 0.75.into(),
        angle: 15.0.into(),
    })
}

/// True for reusable surface presets that can shade an opaque full-frame source.
pub fn is_material(id: &str) -> bool {
    appearance::material_kind(id).is_some()
}

/// Insert a self-contained material and composite it around the current output.
/// All commands form one transaction; the previous graph remains editable.
pub fn material_layer(
    id: &str,
    doc: &crate::Composition,
    foreground: bool,
    position: Option<crate::NodePosition>,
) -> Result<super::Instance> {
    let mut instance = material_surface(id, doc, foreground, position)?;
    let (back, front) = if foreground {
        (doc.output, instance.output)
    } else {
        (instance.output, doc.output)
    };
    let mut merge = Node::new(
        "Material layer composite",
        Operation::Blend(crate::nodes::Blend {
            mode: BlendMode::Normal,
            opacity: 1.0.into(),
        }),
        &[("background", back), ("foreground", front)],
    );
    merge.position = position.map(|p| crate::NodePosition {
        x: p.x + 240.0,
        y: p.y,
    });
    let output = merge.id;
    instance.commands.push(crate::Command::Put {
        node: Box::new(merge),
    });
    instance
        .commands
        .push(crate::Command::Output { id: output });
    Ok(instance)
}

pub(super) fn material_surface(
    id: &str,
    doc: &crate::Composition,
    foreground: bool,
    position: Option<crate::NodePosition>,
) -> Result<super::Instance> {
    if !is_material(id) {
        return Err(crate::Error(
            "This effect is not a full-frame material".into(),
        ));
    }
    let mut preset = build(id)?;
    let source = Node::new(
        "Full-frame surface",
        Operation::Solid(crate::nodes::Solid { color: [1.0; 4] }),
        &[],
    );
    for input in preset.group.inputs.values() {
        for target in &input.targets {
            preset
                .nodes
                .get_mut(&target.node)
                .unwrap()
                .inputs
                .insert(target.port.clone(), source.id);
        }
    }
    preset.group.inputs.clear();
    preset.group.members.insert(source.id);
    preset.nodes.insert(source.id, source);
    preset.group.label = format!(
        "{} {}",
        preset.group.label,
        if foreground {
            "foreground"
        } else {
            "background"
        }
    );
    preset.instantiate_sized(
        doc,
        &Default::default(),
        position,
        super::resolution::Sizing::Fit,
    )
}
