//! Independent surface optics and weathering, with explicit image boundaries.
use super::*;
use crate::{PatinaKind, ReflectionKind};
fn external(b: &mut Builder, label: &str) -> NodeId {
    let node = Node::new(label, Operation::Solid { color: [0.0; 4] }, &[]);
    let id = node.id;
    b.doc.nodes.insert(id, node);
    id
}
fn loop_value(a: f32, b: f32) -> Scalar {
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
pub fn build(id: &str, b: &mut Builder, source: NodeId) -> Result<NodeId> {
    match id {
        "surface_reflection" => {
            let environment = external(b, "Reflection scene");
            let node = b.add(
                "Reflected scene on surface",
                Operation::SurfaceReflection {
                    kind: ReflectionKind::Metal,
                    bevel: 5.0.into(),
                    curvature: 0.35.into(),
                    roughness: 1.0.into(),
                    strength: 0.75.into(),
                    rotation: loop_value(-8.0, 8.0),
                    scale: 1.0.into(),
                    offset_x: loop_value(-60.0, 60.0),
                    offset_y: 0.0.into(),
                },
                &[("image", source), ("reflection", environment)],
            );
            for (p, l) in [
                ("bevel", "Bevel width"),
                ("curvature", "Curvature"),
                ("roughness", "Reflection softness"),
                ("strength", "Reflection amount"),
                ("rotation", "Reflection rotation"),
                ("scale", "Reflection scale"),
                ("offset_x", "Reflection X"),
                ("offset_y", "Reflection Y"),
            ] {
                b.expose(node, p, l)?;
            }
            Ok(node)
        }
        "clear_glass" | "frosted_glass_material" => {
            let background = external(b, "Background to refract");
            let environment = external(b, "Reflection scene");
            let roughness = if id == "clear_glass" { 0.0 } else { 18.0 };
            let glass = b.add(
                "Refracted glass surface",
                Operation::Glass {
                    bevel: 7.0.into(),
                    refraction: loop_value(12.0, 28.0),
                    roughness: roughness.into(),
                    ior: 1.5.into(),
                    transmission: 0.96.into(),
                    tint: 0.15.into(),
                    dispersion: 0.12.into(),
                },
                &[("image", source), ("background", background)],
            );
            let reflection = b.add(
                "Glass scene reflection",
                Operation::SurfaceReflection {
                    kind: ReflectionKind::Glass,
                    bevel: 7.0.into(),
                    curvature: 0.5.into(),
                    roughness: roughness.into(),
                    strength: 0.9.into(),
                    rotation: 0.0.into(),
                    scale: 1.0.into(),
                    offset_x: loop_value(-70.0, 70.0),
                    offset_y: 0.0.into(),
                },
                &[("image", glass), ("reflection", environment)],
            );
            for (p, l) in [
                ("bevel", "Glass bevel"),
                ("refraction", "Refraction"),
                ("roughness", "Frosting"),
                ("ior", "Index of refraction"),
                ("transmission", "Transmission"),
                ("tint", "Tint absorption"),
                ("dispersion", "Dispersion"),
            ] {
                b.expose(glass, p, l)?;
            }
            b.expose(reflection, "strength", "Reflection amount")?;
            b.expose(reflection, "roughness", "Reflection softness")?;
            Ok(reflection)
        }
        "sun_glints" => {
            let node = b.add(
                "Sunlight catching polished details",
                super::sun_glints(),
                &[("image", source)],
            );
            for (p, l) in [
                ("size", "Glint size"),
                ("intensity", "Glint brightness"),
                ("threshold", "Highlight threshold"),
                ("evolution", "Glint animation"),
                ("density", "Glint density"),
                ("angle", "Ray angle"),
            ] {
                b.expose(node, p, l)?;
            }
            Ok(node)
        }
        "metal_patina" => {
            let node = b.add(
                "Uneven tarnish and dirt",
                Operation::Patina {
                    kind: PatinaKind::Tarnish,
                    seed: 71,
                    amount: 0.75.into(),
                    dirt: 0.35.into(),
                    scale: 110.0.into(),
                    wear: 0.55.into(),
                    scratches: 0.25.into(),
                },
                &[("image", source)],
            );
            for (p, l) in [
                ("amount", "Tarnish amount"),
                ("dirt", "Dirt amount"),
                ("scale", "Patch size"),
                ("wear", "Clean worn edges"),
                ("scratches", "Scratches"),
            ] {
                b.expose(node, p, l)?;
            }
            Ok(node)
        }
        _ => Err(crate::Error("unknown optics preset".into())),
    }
}
