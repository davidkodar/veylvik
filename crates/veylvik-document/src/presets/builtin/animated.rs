//! Animated material recipes: a catalogue look plus keyed effects, looping over four seconds.
use super::*;
use crate::MaskMode;

pub const IDS: [&str; 6] = [
    "chrome_shine_sweep",
    "glint_reveal",
    "heat_to_embers",
    "rust_spreading",
    "paper_burn",
    "wet_to_dry",
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
        "paper_burn" => paper_burn(b, source)?,
        "wet_to_dry" => {
            let stone = look(b, source, "sandstone")?;
            // Wet stone is darker and glossy, beaded with water; it dries matte.
            if let Operation::Material(m) = &mut b.doc.nodes.get_mut(&stone).unwrap().operation {
                m.roughness = keyed(true, &[(0, 0.06), (2, 0.85), (4, 0.06)]);
            }
            let darker = b.add(
                "Wet darkening",
                Operation::Exposure(crate::nodes::Exposure {
                    stops: keyed(true, &[(0, -0.9), (2, 0.0), (4, -0.9)]),
                }),
                &[("image", stone)],
            );
            let mut water: Operation =
                serde_json::from_value(crate::registry::CONDENSATION.default_operation())
                    .map_err(|e| crate::Error(e.to_string()))?;
            if let Operation::Condensation(c) = &mut water {
                c.amount = keyed(true, &[(0, 1.0), (2, 0.0), (4, 1.0)]);
            }
            let water = b.add("Water drying off", water, &[("image", darker)]);
            // The surface already exposes Roughness, which carries the drying gloss.
            b.expose(darker, "stops", "Wet darkening")?;
            b.expose(water, "amount", "Water")?;
            water
        }
        _ => return Ok(None),
    };
    // Captured defaults describe the finished recipe.
    for (_, control) in &mut b.controls {
        control.default = control.value(&b.doc)?;
    }
    Ok(Some(output))
}

/// Paper that burns away along a noise front: the burnt part is cut out, a charred
/// band sits just inside the edge, and a thin ember line glows on the edge itself.
/// One control, Burn progress, moves every band together (it scales the noise).
fn paper_burn(b: &mut Builder, source: NodeId) -> Result<NodeId> {
    let paper = look(b, source, "embossed_paper")?;
    let noise = b.add(
        "Burn pattern",
        Operation::FractalNoise(crate::nodes::FractalNoise {
            seed: 29,
            scale: 120.0.into(),
            octaves: 5.0.into(),
            roughness: 0.55.into(),
            evolution: 0.0.into(),
        }),
        &[],
    );
    let front = b.add(
        "Burn front",
        Operation::Exposure(crate::nodes::Exposure {
            // Gain 2.3 leaves all paper above the burn level; 0.35 takes it all.
            stops: keyed(true, &[(0, 1.2), (2, -1.5), (4, 1.2)]),
        }),
        &[("image", noise)],
    );
    let band = |b: &mut Builder, label: &str, low: f32, high: f32| {
        b.add(
            label,
            Operation::LumaKey(crate::nodes::LumaKey {
                low: low.into(),
                high: high.into(),
            }),
            &[("image", front)],
        )
    };
    // Narrow bands: the burn progress scales the noise, which widens them as it
    // advances, so the ember stays a line and the char a band.
    let unburnt = band(b, "Paper left", 0.3, 0.31);
    let edge = band(b, "Ember edge", 0.312, 0.322);
    let interior = band(b, "Untouched paper", 0.34, 0.37);
    let combine = |b: &mut Builder, label: &str, mode, mask, other| {
        b.add(
            label,
            Operation::CombineMask(crate::nodes::CombineMask { mode }),
            &[("mask", mask), ("other", other)],
        )
    };
    let char_band = combine(b, "Charred band", MaskMode::Subtract, unburnt, interior);
    let char_band = combine(
        b,
        "Charred band on paper",
        MaskMode::Intersect,
        char_band,
        paper,
    );
    let ember_band = combine(b, "Ember line", MaskMode::Subtract, unburnt, edge);
    let ember_band = combine(
        b,
        "Ember line on paper",
        MaskMode::Intersect,
        ember_band,
        paper,
    );
    let left = b.add(
        "Paper not yet burnt",
        Operation::Mask(crate::nodes::Mask {}),
        &[("image", paper), ("mask", unburnt)],
    );
    let soot = b.add(
        "Char colour",
        Operation::Solid(crate::nodes::Solid {
            color: [0.05, 0.025, 0.012, 1.0],
        }),
        &[],
    );
    let soot = b.add(
        "Char",
        Operation::Mask(crate::nodes::Mask {}),
        &[("image", soot), ("mask", char_band)],
    );
    let charred = b.add(
        "Charred paper",
        Operation::Blend(crate::nodes::Blend {
            mode: BlendMode::Normal,
            opacity: 0.85.into(),
        }),
        &[("background", left), ("foreground", soot)],
    );
    let fire = b.add(
        "Ember colour",
        Operation::Solid(crate::nodes::Solid {
            color: [4.0, 1.1, 0.15, 1.0],
        }),
        &[],
    );
    let fire = b.add(
        "Embers",
        Operation::Mask(crate::nodes::Mask {}),
        &[("image", fire), ("mask", ember_band)],
    );
    let glow = b.add(
        "Ember glow",
        Operation::Blur(crate::nodes::Blur {
            radius: 4.0.into(),
            filter: crate::nodes::BlurFilter::Gaussian,
        }),
        &[("image", fire)],
    );
    let out = b.add(
        "Burning paper",
        Operation::Blend(crate::nodes::Blend {
            mode: BlendMode::Add,
            opacity: 1.0.into(),
        }),
        &[("background", charred), ("foreground", glow)],
    );
    b.expose(front, "stops", "Burn progress")?;
    b.expose(noise, "scale", "Burn pattern size")?;
    b.expose(glow, "radius", "Ember glow")?;
    Ok(out)
}
