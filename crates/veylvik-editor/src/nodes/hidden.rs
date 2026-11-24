//! Which registered settings a node's inspector leaves out, given the node and what is wired to it: a setting that does nothing as things are (a force's direction for turbulence, an emitter box when an image is the emitter) is not offered. One place for these per-node rules beside the nodes' own controls.
use veylvik_document::Operation;

/// Whether `name` is left out of `operation`'s settings; `wired` says whether a port is
/// connected.
pub(crate) fn hidden(operation: &Operation, name: &str, wired: &dyn Fn(&str) -> bool) -> bool {
    match operation {
        // Animated, the light shows in the animation list; constant, in the node's controls.
        Operation::Shadow(s) => {
            name == "light" && matches!(s.light, veylvik_document::Scalar::Constant { .. })
        }
        // A 3D surface with a look is shaded by it, not these.
        Operation::Shape3d(s) => s.look.is_some() && matches!(name, "roughness" | "metallic"),
        Operation::Text3d(t) => t.look.is_some() && matches!(name, "roughness" | "metallic"),
        // A projection's hold time only with its camera held.
        Operation::CameraProjection(c) => name == "hold_time" && !c.hold,
        // A force shows the settings its kind uses.
        Operation::Force(force) => {
            use veylvik_document::nodes::ForceKind::*;
            match name {
                "direction" | "lean" => !matches!(force.kind, Wind | Vortex),
                "scale" => force.kind != Turbulence,
                _ => false,
            }
        }
        // Map controls appear with their map.
        Operation::Material(_) => match name {
            "shot" => !wired("environment"),
            "normal_strength" => !wired("normal_map"),
            "height_depth" => !wired("height_map"),
            _ => false,
        },
        // Particles show the settings that apply to how they are wired.
        Operation::Particles(_) => particles(operation, name, wired),
        _ => false,
    }
}
fn particles(operation: &Operation, name: &str, wired: &dyn Fn(&str) -> bool) -> bool {
    match name {
        "density" => !wired("emitter"),
        "rate" | "x" | "y" | "width" | "height" => wired("emitter"),
        "homing" => !wired("target"),
        "inherit" => !wired("emitter") && !wired("target"),
        "field_push" | "field_flow" => !wired("field"),
        "bounce" => {
            !wired("obstacle")
                && !matches!(*operation, Operation::Particles(ref n) if n.collide && wired("scene"))
        }
        "spin" => !wired("sprite"),
        // The sheet is set in the node's own controls; its rate when it plays.
        "sprite_columns" | "sprite_rows" | "sprite_play" => true,
        "sprite_rate" => {
            !wired("sprite")
                || !matches!(*operation, Operation::Particles(ref n) if n.sprite_play == veylvik_document::nodes::SpritePlay::AtRate)
        }
        "camera" => !wired("scene"),
        // A flock's own settings once it is one.
        "cohesion" | "separation" | "alignment" | "top_speed" => {
            matches!(*operation, Operation::Particles(ref n) if matches!(n.flock_radius, veylvik_document::Scalar::Constant { value } if value <= 0.0))
        }
        // Offered as a choice by the node's own inspector.
        "history" => true,
        // A burst's own settings once it has particles.
        "burst_speed" | "burst_spread" | "burst_life" | "burst_size" => {
            matches!(*operation, Operation::Particles(ref n) if matches!(n.burst, veylvik_document::Scalar::Constant { value } if value < 0.5))
        }
        // A trail's width and the lines' look once there is a trail or lines.
        "trail_width" => {
            matches!(*operation, Operation::Particles(ref n) if matches!(n.trail, veylvik_document::Scalar::Constant { value } if value <= 0.0))
        }
        // Lighting comes from a scene; its details once particles are lit.
        "lit" => !wired("scene"),
        "ambient" | "scatter" => {
            !wired("scene")
                || matches!(*operation, Operation::Particles(ref n) if matches!(n.lit, veylvik_document::Scalar::Constant { value } if value <= 0.0))
        }
        "link_opacity" | "link_width" => {
            matches!(*operation, Operation::Particles(ref n) if matches!(n.link_distance, veylvik_document::Scalar::Constant { value } if value <= 0.0))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn op(kind: &veylvik_document::registry::NodeType) -> Operation {
        serde_json::from_value(kind.default_operation()).unwrap()
    }
    #[test]
    fn settings_that_do_nothing_as_wired_are_left_out() {
        let particles = op(&veylvik_document::registry::PARTICLES);
        let none = |_: &str| false;
        let emitter = |port: &str| port == "emitter";
        // A box when nothing is the emitter, the density over an image when one is.
        assert!(!hidden(&particles, "width", &none) && hidden(&particles, "density", &none));
        assert!(hidden(&particles, "width", &emitter) && !hidden(&particles, "density", &emitter));
        assert!(hidden(&particles, "camera", &none));
        let force = op(&veylvik_document::registry::FORCE);
        assert!(!hidden(&force, "strength", &none));
    }
}
