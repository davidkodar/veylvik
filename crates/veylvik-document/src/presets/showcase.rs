//! Portable, editable scenes built around the same presets offered by the library.
mod art_direction;
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
        "fireflies" => "Toxic nectar",
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
            "Glossy pink lettering stretches into hanging drips, falling droplets and a liquid pool."
        }
        "metallic_shine" => {
            "A narrow warm-white specular reflection travels across gold lettering. Edit its position keys, angle, width and brightness; alpha remains untouched."
        }
        "embers" => {
            "Reflective gold lettering burns above a smoky reflection. Layered turbulent fire rises behind the title while embers rise."
        }
        "fireflies" => {
            "Glossy green lettering drips into luminous nectar, surrounded by drifting fireflies."
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
// Original geometric lettering; no font files or external media are needed.
fn lettering(word: &str) -> PathMask {
    let mut commands = Vec::new();
    for (i, c) in word.chars().enumerate() {
        let x = 168.0 + i as f32 * 162.0;
        let mut rect = |a: f32, b: f32, w: f32, h: f32| {
            polygon(
                &mut commands,
                &[
                    [x + a, 200.0 + b],
                    [x + a + w, 200.0 + b],
                    [x + a + w, 200.0 + b + h],
                    [x + a, 200.0 + b + h],
                ],
            )
        };
        match c {
            'N' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(106.0, 0.0, 24.0, 140.0);
                polygon(
                    &mut commands,
                    &[
                        [x, 200.0],
                        [x + 26.0, 200.0],
                        [x + 130.0, 340.0],
                        [x + 104.0, 340.0],
                    ],
                );
            }
            'E' | 'F' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(24.0, 0.0, 106.0, 24.0);
                rect(24.0, 58.0, 82.0, 24.0);
                if c == 'E' {
                    rect(24.0, 116.0, 106.0, 24.0);
                }
            }
            'O' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(106.0, 0.0, 24.0, 140.0);
                rect(24.0, 0.0, 82.0, 24.0);
                rect(24.0, 116.0, 82.0, 24.0);
            }
            'L' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(24.0, 116.0, 106.0, 24.0);
            }
            'W' => {
                polygon(
                    &mut commands,
                    &[
                        [x, 200.0],
                        [x + 25.0, 200.0],
                        [x + 40.0, 306.0],
                        [x + 55.0, 245.0],
                        [x + 76.0, 245.0],
                        [x + 91.0, 306.0],
                        [x + 106.0, 200.0],
                        [x + 130.0, 200.0],
                        [x + 108.0, 340.0],
                        [x + 83.0, 340.0],
                        [x + 65.0, 285.0],
                        [x + 48.0, 340.0],
                        [x + 23.0, 340.0],
                    ],
                );
            }
            'D' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(24.0, 0.0, 76.0, 24.0);
                rect(24.0, 116.0, 76.0, 24.0);
                polygon(
                    &mut commands,
                    &[
                        [x + 100.0, 200.0],
                        [x + 130.0, 230.0],
                        [x + 130.0, 310.0],
                        [x + 100.0, 340.0],
                        [x + 100.0, 306.0],
                        [x + 106.0, 296.0],
                        [x + 106.0, 244.0],
                        [x + 100.0, 234.0],
                    ],
                );
            }
            'G' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(24.0, 0.0, 106.0, 24.0);
                rect(24.0, 116.0, 106.0, 24.0);
                rect(106.0, 65.0, 24.0, 51.0);
                rect(70.0, 65.0, 36.0, 24.0);
            }
            'R' | 'P' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(24.0, 0.0, 106.0, 24.0);
                rect(106.0, 24.0, 24.0, 46.0);
                rect(24.0, 70.0, 106.0, 24.0);
                if c == 'R' {
                    polygon(
                        &mut commands,
                        &[
                            [x + 60.0, 294.0],
                            [x + 88.0, 294.0],
                            [x + 130.0, 340.0],
                            [x + 98.0, 340.0],
                        ],
                    );
                }
            }
            'I' => {
                rect(0.0, 0.0, 130.0, 24.0);
                rect(53.0, 24.0, 24.0, 92.0);
                rect(0.0, 116.0, 130.0, 24.0);
            }
            'U' => {
                rect(0.0, 0.0, 24.0, 140.0);
                rect(106.0, 0.0, 24.0, 140.0);
                rect(24.0, 116.0, 82.0, 24.0);
            }
            'S' => {
                rect(0.0, 0.0, 130.0, 24.0);
                rect(0.0, 24.0, 24.0, 46.0);
                rect(0.0, 58.0, 130.0, 24.0);
                rect(106.0, 82.0, 24.0, 34.0);
                rect(0.0, 116.0, 130.0, 24.0);
            }
            'T' => {
                rect(0.0, 0.0, 130.0, 24.0);
                rect(53.0, 24.0, 24.0, 116.0);
            }
            'V' => {
                polygon(
                    &mut commands,
                    &[
                        [x, 200.0],
                        [x + 27.0, 200.0],
                        [x + 65.0, 307.0],
                        [x + 103.0, 200.0],
                        [x + 130.0, 200.0],
                        [x + 79.0, 340.0],
                        [x + 51.0, 340.0],
                    ],
                );
            }
            'A' => {
                polygon(
                    &mut commands,
                    &[
                        [x, 340.0],
                        [x + 49.0, 200.0],
                        [x + 81.0, 200.0],
                        [x + 130.0, 340.0],
                        [x + 104.0, 340.0],
                        [x + 65.0, 229.0],
                        [x + 26.0, 340.0],
                    ],
                );
                polygon(
                    &mut commands,
                    &[
                        [x + 35.0, 291.0],
                        [x + 95.0, 291.0],
                        [x + 104.0, 315.0],
                        [x + 26.0, 315.0],
                    ],
                );
            }
            _ => unreachable!(),
        }
    }
    PathMask {
        fill_rule: FillRule::NonZero,
        commands,
    }
}
pub fn build(id: &str) -> Result<Scene> {
    if matches!(super::builtin::category(id), "Materials" | "Atmosphere")
        || matches!(
            id,
            "metallic_shine"
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
        Operation::Solid {
            color: [0.002, 0.004, 0.012, 1.0],
        },
        &[],
    );
    let emblem = matches!(
        id,
        "prismatic_glow" | "radial_echo" | "orbit_echo" | "anamorphic_streaks" | "spotlight"
    ) || super::builtin::category(id) == "Particles";
    let geometry = if emblem {
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
        PathMask {
            fill_rule: FillRule::NonZero,
            commands,
        }
    } else {
        lettering(if id == "neon_outline" { "NEON" } else { "FLOW" })
    };
    let shape = add(
        &mut doc,
        "Editable vector artwork",
        Operation::Path { geometry },
        &[],
    );
    let ramp = add(
        &mut doc,
        "Vertical palette ramp",
        Operation::Gradient {
            mode: crate::GradientMode::Linear,
            start_x: 0.0.into(),
            start_y: 180.0.into(),
            end_x: 0.0.into(),
            end_y: 360.0.into(),
        },
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
        Operation::Colorize { low, high },
        &[("image", ramp)],
    );
    let art = add(
        &mut doc,
        "Color inside artwork",
        Operation::Mask {},
        &[("image", ink), ("mask", shape)],
    );
    let art = add(
        &mut doc,
        "Artwork motion",
        Operation::Affine {
            x: 0.0.into(),
            y: cycle(9.0, -9.0),
            scale_x: cycle(0.94, 1.04),
            scale_y: cycle(0.94, 1.04),
            rotation: if emblem { cycle(-6.0, 6.0) } else { 0.0.into() },
            pivot_x: 480.0.into(),
            pivot_y: 270.0.into(),
        },
        &[("image", art)],
    );
    let art = if id == "neon_outline" {
        add(
            &mut doc,
            "Title reveal",
            Operation::Exposure {
                stops: cycle(-3.0, 1.0),
            },
            &[("image", art)],
        )
    } else {
        art
    };
    let before = add(
        &mut doc,
        "Before · artwork on midnight",
        Operation::Merge {},
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
                    .all(|n| !matches!(n.operation, Operation::Image { .. }))
            );
            let group = doc
                .groups
                .values()
                .find(|g| g.label == entry.label)
                .unwrap();
            assert_eq!(group.label, entry.label);
            if entry.id == "metallic_shine"
                || entry.id == "sun_glints"
                || super::super::builtin::category(entry.id) == "Materials"
            {
                assert_eq!(doc.nodes[&doc.output].inputs["foreground"], group.output);
            } else {
                assert_eq!(group.output, doc.output);
            }
            assert_eq!(
                group.members.len(),
                super::super::builtin::build(entry.id).unwrap().nodes.len()
            );
            if matches!(
                entry.id,
                "surface_reflection" | "clear_glass" | "frosted_glass_material"
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
                    == if entry.id == "metallic_shine"
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
