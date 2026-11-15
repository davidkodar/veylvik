//! Animated material recipes: a catalogue look plus keyed effects, looping over four seconds.
use super::*;

pub const IDS: [&str; 4] = [
    "chrome_shine_sweep",
    "glint_reveal",
    "heat_to_embers",
    "rust_spreading",
];

/// Keys at whole seconds; the first and last values match so the loop is seamless.
fn keyed(smooth: bool, keys: &[(i64, f32)]) -> Scalar {
    let keys = keys
        .iter()
        .map(|&(ticks, value)| Key {
            time: Time { ticks, rate: 1 },
            value,
            segment: None,
        })
        .collect();
    if smooth {
        Scalar::Smooth { keys }
    } else {
        Scalar::Linear { keys }
    }
}

fn look(b: &mut Builder, source: NodeId, id: &str) -> Result<NodeId> {
    let entry = super::super::catalogue::get(id)
        .ok_or_else(|| crate::Error(format!("unknown look {id}")))?;
    appearance::surface(b, source, entry.material())
}

pub fn build(id: &str, b: &mut Builder, source: NodeId) -> Result<Option<NodeId>> {
    let output = match id {
        "chrome_shine_sweep" => {
            let chrome = look(b, source, "chrome")?;
            let shine = b.add(
                "Shine sweeping across the chrome",
                metallic_shine(),
                &[("image", chrome)],
            );
            for (p, l) in [
                ("x", "Sweep position"),
                ("width", "Shine width"),
                ("intensity", "Shine strength"),
            ] {
                b.expose(shine, p, l)?;
            }
            shine
        }
        "glint_reveal" => {
            let gold = look(b, source, "rose_gold")?;
            let mut glints = sun_glints();
            if let Operation::SunGlints(g) = &mut glints {
                // Rose gold's highlights sit lower than chrome's; catch more of them.
                g.threshold = 0.3.into();
                g.density = 0.9.into();
                g.intensity = keyed(true, &[(0, 0.0), (1, 5.0), (3, 5.0), (4, 0.0)]);
            }
            let glints = b.add("Glints flaring up and fading", glints, &[("image", gold)]);
            for (p, l) in [
                ("intensity", "Glint strength"),
                ("density", "Glint density"),
                ("size", "Glint size"),
            ] {
                b.expose(glints, p, l)?;
            }
            glints
        }
        "heat_to_embers" => {
            let steel = look(b, source, "brushed_steel")?;
            let heat = keyed(true, &[(0, 0.0), (2, 1.0), (4, 0.0)]);
            // The metal's own brightness picks the colour: dark areas glow deep red,
            // highlights white-hot, as heated steel does.
            let glow = b.add(
                "Heat colour",
                Operation::Colorize(crate::nodes::Colorize {
                    low: [0.6, 0.04, 0.0, 1.0],
                    high: [4.0, 1.4, 0.25, 1.0],
                }),
                &[("image", steel)],
            );
            let core = b.add(
                "Glowing metal",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Add,
                    opacity: heat.clone(),
                }),
                &[("background", steel), ("foreground", glow)],
            );
            let halo = b.add(
                "Ember halo",
                Operation::Blur(crate::nodes::Blur {
                    radius: 14.0.into(),
                    filter: crate::nodes::BlurFilter::Gaussian,
                }),
                &[("image", glow)],
            );
            let out = b.add(
                "Heat haze glow",
                Operation::Blend(crate::nodes::Blend {
                    mode: BlendMode::Add,
                    opacity: keyed(true, &[(0, 0.0), (2, 0.6), (4, 0.0)]),
                }),
                &[("background", core), ("foreground", halo)],
            );
            b.expose(core, "opacity", "Heat")?;
            b.expose(out, "opacity", "Glow")?;
            b.expose(halo, "radius", "Glow spread")?;
            out
        }
        "rust_spreading" => {
            let steel = look(b, source, "brushed_steel")?;
            let rust = b.add(
                "Rust spreading over steel",
                Operation::Patina(crate::nodes::Patina {
                    kind: crate::PatinaKind::Rust,
                    seed: 71,
                    amount: keyed(true, &[(0, 0.0), (2, 0.9), (4, 0.0)]),
                    dirt: 0.25.into(),
                    scale: 110.0.into(),
                    wear: 0.2.into(),
                    scratches: 0.1.into(),
                }),
                &[("image", steel)],
            );
            for (p, l) in [
                ("amount", "Rust"),
                ("dirt", "Dirt"),
                ("scale", "Patch size"),
            ] {
                b.expose(rust, p, l)?;
            }
            rust
        }
        _ => return Ok(None),
    };
    // Captured defaults describe the finished recipe.
    for (_, control) in &mut b.controls {
        control.default = control.value(&b.doc)?;
    }
    Ok(Some(output))
}
