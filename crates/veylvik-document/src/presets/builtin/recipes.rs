//! Curated combinations of ordinary material, weathering and reflection nodes.
use super::*;
pub fn build(id: &str, b: &mut Builder, source: NodeId) -> Result<NodeId> {
    let family = match id {
        "weathered_bronze" => "bronze_text",
        "brushed_light_sweep" => "aluminium_text",
        "sky_lit_gold" => "gold_text",
        _ => return Err(crate::Error("unknown material recipe".into())),
    };
    let material = appearance::build(family, b, source)?;
    if let Operation::Material(surface) = &mut b.doc.nodes.get_mut(&material).unwrap().operation {
        surface.apply_finish(if id == "brushed_light_sweep" {
            crate::optical::MetalFinish::Brushed
        } else {
            crate::optical::MetalFinish::Satin
        });
        if id == "sky_lit_gold" {
            surface.apply_finish(crate::optical::MetalFinish::Polished);
        }
    }
    // The captured defaults must describe the final recipe, not the family factory.
    for (_, control) in &mut b.controls {
        control.default = control.value(&b.doc)?;
    }
    match id {
        "weathered_bronze" => optics::build("metal_patina", b, material),
        "brushed_light_sweep" => {
            let shine = b.add(
                "Travelling reflection over brushed aluminium",
                metallic_shine(),
                &[("image", material)],
            );
            for (p, l) in [
                ("x", "Sweep position"),
                ("width", "Shine width"),
                ("intensity", "Shine strength"),
            ] {
                b.expose(shine, p, l)?;
            }
            Ok(shine)
        }
        "sky_lit_gold" => {
            let sky = b.add(
                "Blue sky reflection source",
                Operation::Solid(crate::nodes::Solid {
                    color: [0.11, 0.34, 0.8, 1.],
                }),
                &[],
            );
            let controls = b.controls.len();
            let clouds = appearance::build("clouds", b, sky)?;
            b.controls.truncate(controls);
            let cloud = b.doc.nodes[&clouds].inputs["foreground"];
            b.expose(cloud, "density", "Cloud density")?;
            b.expose(cloud, "evolution", "Cloud motion")?;
            let reflection = b.add(
                "Moving sky reflected in gold",
                Operation::SurfaceReflection(crate::nodes::SurfaceReflection {
                    kind: crate::ReflectionKind::Metal,
                    bevel: 5.0.into(),
                    curvature: 0.35.into(),
                    roughness: 1.5.into(),
                    strength: 0.45.into(),
                    rotation: 0.0.into(),
                    scale: 1.0.into(),
                    offset_x: Scalar::Smooth {
                        keys: [(0, -70.), (2, 70.), (4, -70.)]
                            .map(|(ticks, value)| Key {
                                time: Time { ticks, rate: 1 },
                                value,
                                segment: None,
                            })
                            .into(),
                    },
                    offset_y: 0.0.into(),
                }),
                &[("image", material), ("reflection", clouds)],
            );
            b.expose(reflection, "strength", "Sky reflection amount")?;
            b.expose(reflection, "roughness", "Sky reflection softness")?;
            b.expose(reflection, "offset_x", "Sky reflection position")?;
            Ok(reflection)
        }
        _ => unreachable!(),
    }
}
