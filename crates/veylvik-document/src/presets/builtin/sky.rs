//! Lights of the sky and the lens as editable shaders: each is one Custom shader node laid over the source, its declared controls exposed under their names, so the code itself stays open to change.
use super::Builder;
use crate::{NodeId, Operation, Result};

/// The effect's shader node label and code.
fn source(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "spiral_galaxy" => ("Galaxy", include_str!("galaxy.wgsl")),
        "distant_sun" => ("Sun", include_str!("sun.wgsl")),
        "lens_flare" => ("Lens flare", include_str!("flare.wgsl")),
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
    let operation: Operation =
        serde_json::from_value(value).map_err(|e| crate::Error(e.to_string()))?;
    let node = b.add(label, operation, &[]);
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
    Ok(b.add(
        &format!("{label} over image"),
        Operation::Merge(crate::nodes::Merge {}),
        &[("background", image), ("foreground", node)],
    ))
}
