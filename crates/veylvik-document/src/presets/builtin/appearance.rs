//! Surface and atmosphere starters with host-sized, editable controls.
use super::*;
use crate::{AtmosphereKind, MaterialKind};
pub fn material_kind(id: &str) -> Option<MaterialKind> {
    match id {
        "gold_text" => Some(MaterialKind::Gold),
        "silver_text" => Some(MaterialKind::Silver),
        "aluminium_text" => Some(MaterialKind::Aluminium),
        "bronze_text" => Some(MaterialKind::Bronze),
        "phosphor_text" => Some(MaterialKind::Phosphor),
        "wood_text" => Some(MaterialKind::Wood),
        "oak_light" => Some(MaterialKind::OakLight),
        "oak_dark" => Some(MaterialKind::OakDark),
        "marble" => Some(MaterialKind::Marble),
        "brick_light" => Some(MaterialKind::BrickLight),
        "brick_red" => Some(MaterialKind::BrickRed),
        "concrete" => Some(MaterialKind::Concrete),
        "paper_clean" => Some(MaterialKind::Paper),
        "paper_aged" => Some(MaterialKind::AgedPaper),
        "leather_red" => Some(MaterialKind::LeatherRed),
        "leather_brown" => Some(MaterialKind::LeatherBrown),
        "leather_tan" => Some(MaterialKind::LeatherTan),
        "leather_dark" => Some(MaterialKind::LeatherDark),
        "leather_black" => Some(MaterialKind::LeatherBlack),
        _ => None,
    }
}
pub fn build(id: &str, b: &mut Builder, source: NodeId) -> Result<NodeId> {
    if let Some(kind) = material_kind(id) {
        return surface(
            b,
            source,
            crate::nodes::Material {
                smooth_edges: true,
                brushing: Default::default(),
                colours: None,
                optics: None,
                studio: Default::default(),
                kind,
                scale: 180.0.into(),
                bevel: 5.0.into(),
                roughness: (match kind {
                    MaterialKind::Gold => 0.18,
                    MaterialKind::Silver => 0.18,
                    MaterialKind::Bronze => 0.38,
                    MaterialKind::Aluminium => 0.28,
                    MaterialKind::Phosphor | MaterialKind::Marble => 0.15,
                    _ => 0.7,
                })
                .into(),
                lighting: Scalar::Smooth {
                    keys: [(0, 0.0), (2, 0.12), (4, 0.0)]
                        .into_iter()
                        .map(|(ticks, value)| Key {
                            time: Time { ticks, rate: 1 },
                            value,
                            segment: None,
                        })
                        .collect(),
                },
                grain: (match kind {
                    MaterialKind::Gold | MaterialKind::Silver => 0.25,
                    MaterialKind::Bronze => 0.35,
                    _ => 0.65,
                })
                .into(),
                emission: 1.0.into(),
                shot: 1.0.into(),
            },
        );
    }
    let (kind, width, height, y, density) = match id {
        "light_smoke" => (AtmosphereKind::Smoke, 650.0, 470.0, 510.0, 1.7),
        "dense_smoke" => (AtmosphereKind::Soot, 700.0, 470.0, 510.0, 7.0),
        "fog_layer" => (AtmosphereKind::Fog, 1500.0, 300.0, 560.0, 0.7),
        "drifting_fog" => (AtmosphereKind::DriftingFog, 1000.0, 360.0, 540.0, 1.5),
        "clouds" => (AtmosphereKind::Clouds, 760.0, 330.0, 400.0, 4.0),
        _ => return Err(crate::Error("unknown appearance preset".into())),
    };
    let volume = b.add(
        "Turbulent atmospheric volume",
        Operation::Atmosphere(crate::nodes::Atmosphere {
            kind,
            seed: 397,
            x: 480.0.into(),
            y: y.into(),
            width: width.into(),
            height: height.into(),
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
            density: density.into(),
            detail: 1.0.into(),
            lighting: 1.0.into(),
        }),
        &[],
    );
    for (p, l) in [
        ("x", "Center X"),
        ("y", "Base Y"),
        ("width", "Volume width"),
        ("height", "Volume height"),
        ("density", "Density"),
        ("evolution", "Flow motion"),
        ("detail", "Turbulence detail"),
        ("lighting", "Illumination"),
    ] {
        b.expose(volume, p, l)?;
    }
    Ok(b.add(
        "Atmosphere over image",
        Operation::Merge(crate::nodes::Merge {}),
        &[("background", source), ("foreground", volume)],
    ))
}

/// A material node with its public controls; glowing phosphor adds its bloom.
pub(in crate::presets) fn surface(
    b: &mut Builder,
    source: NodeId,
    material: crate::nodes::Material,
) -> Result<NodeId> {
    let kind = material.kind;
    let node = b.add(
        "Surface grain, bevel and reflection",
        Operation::Material(material),
        &[("image", source)],
    );
    for (p, l) in [
        ("scale", "Texture scale"),
        ("bevel", "Bevel width"),
        ("roughness", "Roughness"),
        ("lighting", "Reflection motion"),
        ("grain", "Grain strength"),
    ] {
        if p != "roughness" || kind != MaterialKind::Phosphor {
            b.expose(node, p, l)?;
        }
    }
    if kind == MaterialKind::Phosphor {
        b.expose(node, "emission", "Phosphor emission")?;
        let halo = b.add(
            "Phosphor bloom",
            Operation::Blur(crate::nodes::Blur {
                radius: 8.0.into(),
                filter: Default::default(),
            }),
            &[("image", node)],
        );
        let out = b.add(
            "Emissive glow",
            Operation::Blend(crate::nodes::Blend {
                mode: BlendMode::Add,
                opacity: 0.5.into(),
            }),
            &[("background", node), ("foreground", halo)],
        );
        b.expose(halo, "radius", "Glow spread")?;
        b.expose(out, "opacity", "Glow strength")?;
        return Ok(out);
    }
    Ok(node)
}
