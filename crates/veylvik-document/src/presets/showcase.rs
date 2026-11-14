//! Portable, editable scenes built around the same presets offered by the library.
mod art_direction;
mod surface_paint;
use crate::{
    Composition, Document, Key, Node, NodeId, NodePosition, Operation, Result, Scalar, Time,
    vector::{FillRule, PathCommand, PathMask},
};

pub struct Scene {
    pub composition: Composition,
    /// The composition before the effect, for a time-matched comparison.
    pub before: NodeId,
}
pub fn title(id: &str) -> &'static str {
    match id {
        "neon_outline" => "After hours",
        "prismatic_glow" => "Prism / identity",
        "liquid_shimmer" => "Pink melt",
        "embers" => "Gilded inferno",
        "metallic_shine" => "Polished gold",
        "fireflies" => "Green liquid",
        "dust_motes" => "Sands of time",
        "snowfall" => "Deep freeze",
        "starfield" => "Nova ignition",
        _ => super::builtin::ALL
            .iter()
            .find(|e| e.id == id)
            .map_or("Example", |e| e.label),
    }
}
pub fn description(id: &str) -> &'static str {
    match id {
        "neon_outline" => {
            "A cyan title emerges from darkness. Edit the reveal, vector lettering and neon controls."
        }
        "prismatic_glow" => {
            "A floating geometric emblem catches cyan and rose light. Edit its motion and light spill."
        }
        "liquid_shimmer" => {
            "Glossy pink lettering forms hanging drips and falling droplets against a neutral background."
        }
        "metallic_shine" => {
            "A narrow warm-white specular reflection travels across gold lettering. Edit its position keys, angle, width and brightness; alpha remains untouched."
        }
        "embers" => {
            "Reflective gold lettering burns above a smoky reflection. Layered turbulent fire rises behind the title while embers rise."
        }
        "fireflies" => {
            "Green wet liquid follows editable lettering and drips against a neutral background."
        }
        "dust_motes" => {
            "Grainy sandstone lettering stands over a wind-carved dune with suspended golden dust."
        }
        "snowfall" => {
            "Crystalline lettering over a mountain landscape, with snow moving across the foreground."
        }
        "starfield" => {
            "A turbulent plasma corona surrounds an eclipsed nova, with a star flight across the title."
        }
        _ => super::builtin::ALL
            .iter()
            .find(|e| e.id == id)
            .map_or("Editable example", |e| e.description),
    }
}
fn cycle(a: f32, b: f32) -> Scalar {
    Scalar::Smooth {
        keys: [(0, a), (2, b), (4, a)]
            .into_iter()
            .map(|(ticks, value)| Key {
                time: Time { ticks, rate: 1 },
                value,
                segment: None,
            })
            .collect(),
    }
}
fn add(
    doc: &mut Composition,
    label: &str,
    operation: Operation,
    inputs: &[(&str, NodeId)],
) -> NodeId {
    let mut node = Node::new(label, operation, inputs);
    let i = doc.nodes.len();
    node.position = Some(NodePosition {
        x: (i % 5) as f32 * 230.0,
        y: (i / 5) as f32 * 145.0,
    });
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn polygon(commands: &mut Vec<PathCommand>, points: &[[f32; 2]]) {
    commands.push(PathCommand::Move { to: points[0] });
    commands.extend(points[1..].iter().map(|p| PathCommand::Line { to: *p }));
    commands.push(PathCommand::Close {});
}
// Editable title source shared by the showcase scenes.
fn title_text(word: &str) -> Operation {
    let mut operation: Operation =
        serde_json::from_value(crate::registry::TEXT.default_operation()).unwrap();
    if let Operation::Text(crate::nodes::Text {
        content,
        size,
        x,
        y,
        tracking,
        ..
    }) = &mut operation
    {
        *content = word.into();
        *size = 200.0.into();
        *x = 480.0.into();
        *y = 340.0.into();
        *tracking = 12.0.into();
    }
    operation
}

pub fn build(id: &str) -> Result<Scene> {
    if id == "surface_paint" {
        return surface_paint::build();
    }
    if matches!(super::builtin::category(id), "Materials" | "Atmosphere")
        || matches!(
            id,
            "condensation"
                | "wet_drips"
                | "water_drips"
                | "metallic_shine"
                | "sun_glints"
                | "embers"
                | "fireflies"
                | "liquid_shimmer"
                | "dust_motes"
                | "snowfall"
                | "starfield"
        )
    {
        let mut scene = art_direction::build(id)?;
        super::resolution::map_graph(
            &mut scene.composition.nodes,
            &mut scene.composition.groups,
            [960, 540],
            [1920, 1080],
            super::resolution::Sizing::Fit,
        )?;
        scene.composition.width = 1920;
        scene.composition.height = 1080;
        scene.composition.validate()?;
        return Ok(scene);
    }
    let preset = super::builtin::build(id)?;
    let mut doc = crate::blank(960, 540, Time::new(24, 1)?)?;
    doc.frame_count = 96;
    doc.nodes.clear();
    let background = add(
        &mut doc,
        "Midnight background",
        Operation::Solid(crate::nodes::Solid {
            color: [0.002, 0.004, 0.012, 1.0],
        }),
        &[],
    );
    doc.nodes.get_mut(&background).unwrap().scene_background = true;
    let emblem = matches!(
        id,
        "prismatic_glow" | "radial_echo" | "orbit_echo" | "anamorphic_streaks" | "spotlight"
    ) || super::builtin::category(id) == "Particles";
    let artwork = if emblem {
        let mut commands = vec![];
        polygon(
            &mut commands,
            &[
                [480.0, 125.0],
                [650.0, 365.0],
                [600.0, 365.0],
                [480.0, 195.0],
                [360.0, 365.0],
                [310.0, 365.0],
            ],
        );
        polygon(
            &mut commands,
            &[[480.0, 235.0], [565.0, 365.0], [395.0, 365.0]],
        );
        Operation::Path(crate::nodes::Path {
            geometry: PathMask {
                keys: vec![],
                fill_rule: FillRule::NonZero,
                commands,
            },
        })
    } else {
        title_text(if id == "neon_outline" { "NEON" } else { "FLOW" })
    };
    let shape = add(&mut doc, "Editable vector artwork", artwork, &[]);
    let ramp = add(
        &mut doc,
        "Vertical palette ramp",
        Operation::Gradient(crate::nodes::Gradient {
            mode: crate::GradientMode::Linear,
            start_x: 0.0.into(),
            start_y: 180.0.into(),
            end_x: 0.0.into(),
            end_y: 360.0.into(),
        }),
        &[],
    );
    let (low, high) = if super::builtin::category(id) == "Particles" {
        ([0.01, 0.025, 0.05, 1.0], [0.04, 0.09, 0.14, 1.0])
    } else if id == "liquid_shimmer" {
        ([0.04, 0.2, 0.65, 1.0], [0.7, 1.2, 1.3, 1.0])
    } else if id == "neon_outline" {
        ([0.015, 0.035, 0.07, 1.0], [0.06, 0.14, 0.22, 1.0])
    } else {
        ([0.3, 0.45, 0.8, 1.0], [1.6, 1.8, 2.0, 1.0])
    };
    let ink = add(
        &mut doc,
        "Artwork palette",
        Operation::Colorize(crate::nodes::Colorize { low, high }),
        &[("image", ramp)],
    );
    let art = add(
        &mut doc,
        "Color inside artwork",
        Operation::Mask(crate::nodes::Mask {}),
        &[("image", ink), ("mask", shape)],
    );
    let art = add(
        &mut doc,
        "Artwork motion",
        Operation::Affine(crate::nodes::Affine {
            x: if id == "shutter_motion" {
                Scalar::Linear {
                    keys: [
                        (0, 0.),
                        (18, -180.),
                        (30, 180.),
                        (48, 0.),
                        (66, -180.),
                        (78, 180.),
                        (96, 0.),
                    ]
                    .into_iter()
                    .map(|(frame, value)| crate::Key {
                        time: Time::new(frame, 24).unwrap(),
                        value,
                        segment: None,
                    })
                    .collect(),
                }
            } else {
                0.0.into()
            },
            y: cycle(9.0, -9.0),
            scale_x: cycle(0.94, 1.04),
            scale_y: cycle(0.94, 1.04),
            rotation: if emblem { cycle(-6.0, 6.0) } else { 0.0.into() },
            pivot_x: 480.0.into(),
            pivot_y: 270.0.into(),
        }),
        &[("image", art)],
    );
    let art = if id == "neon_outline" {
        add(
            &mut doc,
            "Title reveal",
            Operation::Exposure(crate::nodes::Exposure {
                stops: cycle(-3.0, 1.0),
            }),
            &[("image", art)],
        )
    } else {
        art
    };
    let before = add(
        &mut doc,
        "Before · artwork on midnight",
        Operation::Merge(crate::nodes::Merge {}),
        &[("background", background), ("foreground", art)],
    );
    doc.output = before;
    let instance = preset.instantiate(
        &doc,
        &[("input_1".into(), before)].into(),
        Some(NodePosition { x: 0.0, y: 500.0 }),
    )?;
    let document = Document::new(doc)?;
    let mut composition = document.prepare(0, &instance.commands)?;
    composition.output = instance.output;
    composition.validate()?;
    Ok(Scene {
        composition,
        before,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn examples_are_portable_editable_and_use_the_library_effects() {
        for entry in super::super::builtin::ALL {
            let scene = build(entry.id).unwrap();
            let doc = &scene.composition;
            doc.validate().unwrap();
            assert_ne!(scene.before, doc.output);
            assert!(
                doc.nodes
                    .values()
                    .all(|n| !matches!(n.operation, Operation::Image(crate::nodes::Image { .. })))
            );
            let group = doc
                .groups
                .values()
                .find(|g| {
                    g.label
                        == if matches!(entry.id, "fireflies" | "liquid_shimmer") {
                            "Wet Ink Drips"
                        } else {
                            entry.label
                        }
                })
                .unwrap();
            assert_eq!(
                group.label,
                if matches!(entry.id, "fireflies" | "liquid_shimmer") {
                    "Wet Ink Drips"
                } else {
                    entry.label
                }
            );
            if matches!(
                entry.id,
                "condensation" | "fireflies" | "liquid_shimmer" | "wet_drips" | "water_drips"
            ) || entry.id == "metallic_shine"
                || entry.id == "sun_glints"
                || super::super::builtin::category(entry.id) == "Materials"
            {
                assert_eq!(doc.nodes[&doc.output].inputs["foreground"], group.output);
            } else {
                assert_eq!(group.output, doc.output);
            }
            assert_eq!(
                group.members.len(),
                super::super::builtin::build(
                    if matches!(entry.id, "fireflies" | "liquid_shimmer") {
                        "wet_drips"
                    } else {
                        entry.id
                    }
                )
                .unwrap()
                .nodes
                .len()
            );
            if matches!(
                entry.id,
                "water_drips"
                    | "surface_paint"
                    | "surface_reflection"
                    | "clear_glass"
                    | "frosted_glass_material"
            ) {
                for input in group.inputs.values() {
                    let sources: std::collections::BTreeSet<_> = input
                        .targets
                        .iter()
                        .map(|p| doc.nodes[&p.node].inputs[&p.port])
                        .collect();
                    assert_eq!(sources.len(), 1);
                    assert!(sources.iter().all(|n| !group.members.contains(n)));
                }
                continue;
            }
            assert!(group.inputs.values().flat_map(|i| &i.targets).all(
                |p| doc.nodes[&p.node].inputs[&p.port]
                    == if matches!(
                        entry.id,
                        "condensation"
                            | "fireflies"
                            | "liquid_shimmer"
                            | "wet_drips"
                            | "water_drips"
                    ) || entry.id == "metallic_shine"
                        || entry.id == "sun_glints"
                        || super::super::builtin::category(entry.id) == "Materials"
                    {
                        doc.nodes[&scene.before].inputs["foreground"]
                    } else {
                        scene.before
                    }
            ));
        }
        assert!(build("missing").is_err());
    }
}
