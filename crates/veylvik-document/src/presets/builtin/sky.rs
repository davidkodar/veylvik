//! Effects that are editable shaders (lights of the sky and the lens, a television switching off): each is one Custom shader node, laid over the source or working on it, its declared controls exposed under their names, so the code itself stays open to change.
use super::Builder;
use crate::{NodeId, Operation, Result};

/// The effect's shader node label and code.
fn source(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "spiral_galaxy" => ("Galaxy", include_str!("galaxy.wgsl")),
        "distant_sun" => ("Sun", include_str!("sun.wgsl")),
        "lens_flare" => ("Lens flare", include_str!("flare.wgsl")),
        "deep_sky" => ("Sky", include_str!("deep_sky.wgsl")),
        "switch_off" => ("Switch off", include_str!("switch_off.wgsl")),
        "chromatic_fringe" => ("Fringe", include_str!("fringe.wgsl")),
        _ => return None,
    })
}
pub(super) fn is(id: &str) -> bool {
    source(id).is_some()
}
pub(super) fn build(id: &str, b: &mut Builder, image: NodeId) -> Result<NodeId> {
    let (label, code) = source(id).ok_or_else(|| crate::Error("unknown starter effect".into()))?;
    let controls = veylvik_shader::controls(code).map_err(crate::Error)?;
    let mut value = crate::nodes::SHADER.default_operation();
    value["code"] = code.into();
    for (slot, default) in veylvik_shader::defaults(&controls).into_iter().enumerate() {
        value[format!("p{}", slot + 1)] = serde_json::json!({"kind": "constant", "value": default});
    }
    // The switch-off and the fringe work on the image itself (the others are laid over
    // it). The switch-off is thrown, stays dark and comes back within four seconds, so
    // the effect shows what it does as a loop; move the keys to where the picture should
    // go off, and drop the last two to stay dark.
    let alters = id == "switch_off" || id == "chromatic_fringe";
    if id == "switch_off" {
        value["p1"] = serde_json::json!({"kind": "linear", "keys": [
            {"time": {"ticks": 7, "rate": 5}, "value": 0.0},
            {"time": {"ticks": 12, "rate": 5}, "value": 1.0},
            {"time": {"ticks": 17, "rate": 5}, "value": 1.0},
            {"time": {"ticks": 19, "rate": 5}, "value": 0.0}]});
    }
    let operation: Operation =
        serde_json::from_value(value).map_err(|e| crate::Error(e.to_string()))?;
    let wired: &[(&str, NodeId)] = if alters { &[("image", image)] } else { &[] };
    let node = b.add(label, operation, wired);
    let named = |name: &str| {
        let mut text = name.replace('_', " ");
        if let Some(first) = text.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        text
    };
    for control in &controls {
        match control {
            veylvik_shader::Control::Slider { name, slot, .. } => {
                b.expose(node, &format!("p{}", slot + 1), &named(name))?;
            }
            veylvik_shader::Control::Colour { name, slot, .. } => {
                for (k, part) in ["red", "green", "blue"].into_iter().enumerate() {
                    b.expose(
                        node,
                        &format!("p{}", slot + k + 1),
                        &format!("{} {part}", named(name)),
                    )?;
                }
            }
        }
    }
    if alters {
        return Ok(node);
    }
    // A whole sky is opaque, so its light is added to the image; the others are laid
    // over it.
    let operation = if id == "deep_sky" {
        Operation::Blend(crate::nodes::Blend {
            mode: crate::BlendMode::Add,
            opacity: 1.0.into(),
        })
    } else {
        Operation::Merge(crate::nodes::Merge {})
    };
    Ok(b.add(
        &format!("{label} over image"),
        operation,
        &[("background", image), ("foreground", node)],
    ))
}
