//! Ready-made volume looks: each sets a Volume node's settings for a frame size, keeping its placing and seed, so haze and smoke start from something that works.
use veylvik_document::{
    Operation,
    nodes::{NodeOp, Volume3d, VolumeShape},
};

pub struct Look {
    pub name: &'static str,
    pub hint: &'static str,
    shape: VolumeShape,
    colour: [f32; 3],
    /// Lengths in pixels for a frame 1920 wide, then other settings as they are.
    lengths: &'static [(&'static str, f32)],
    values: &'static [(&'static str, f32)],
}
impl Look {
    /// The node's settings for this look in a frame `width` pixels wide; lengths scale with
    /// the frame's width; its placing and seed are kept.
    pub fn apply(&self, node: &mut Volume3d, width: u32) {
        let (placement, seed) = (node.placement.clone(), node.seed);
        let Ok(Operation::Volume3d(fresh)) =
            serde_json::from_value(veylvik_document::nodes::VOLUME_3D.default_operation())
        else {
            return;
        };
        *node = fresh;
        node.placement = placement;
        node.seed = seed;
        node.shape = self.shape;
        node.colour = self.colour.into();
        let scale = width as f32 / 1920.0;
        for (name, value) in self.lengths {
            if let Some(s) = node.scalar_mut(name) {
                *s = (value * scale).into();
            }
        }
        for (name, value) in self.values {
            if let Some(s) = node.scalar_mut(name) {
                *s = (*value).into();
            }
        }
    }
}
pub static LOOKS: [Look; 4] = [
    Look {
        name: "Ground mist",
        hint: "Low cool mist lying on the ground and thinning upwards, drifting slowly sideways",
        shape: VolumeShape::Ground,
        colour: [0.9, 0.93, 1.0],
        lengths: &[
            ("size_x", 4000.0),
            ("size_y", 300.0),
            ("size_z", 3000.0),
            ("noise_size", 400.0),
            ("drift_x", 30.0),
        ],
        values: &[
            ("density", 1.2),
            ("absorption", 0.0),
            ("forward", 0.3),
            ("noise", 0.7),
            ("detail", 4.0),
            ("softness", 0.4),
        ],
    },
    Look {
        name: "Sun-shaft haze",
        hint: "Thin warm haze filling the scene: light shafts where objects shade it, a glow looking towards the light",
        shape: VolumeShape::Box,
        colour: [1.0, 0.97, 0.9],
        lengths: &[
            ("size_x", 4000.0),
            ("size_y", 2500.0),
            ("size_z", 3000.0),
            ("noise_size", 600.0),
            ("drift_x", 10.0),
        ],
        values: &[
            ("density", 0.15),
            ("absorption", 0.0),
            ("forward", 0.75),
            ("noise", 0.3),
            ("detail", 3.0),
            ("softness", 0.2),
        ],
    },
    Look {
        name: "Smoke puff",
        hint: "A billowing grey puff of smoke rising slowly",
        shape: VolumeShape::Sphere,
        colour: [0.75, 0.75, 0.75],
        lengths: &[
            ("size_x", 500.0),
            ("size_y", 500.0),
            ("size_z", 500.0),
            ("noise_size", 120.0),
            ("drift_y", -60.0),
        ],
        values: &[
            ("density", 2.5),
            ("absorption", 0.3),
            ("forward", 0.2),
            ("noise", 0.9),
            ("detail", 5.0),
            ("softness", 0.8),
        ],
    },
    Look {
        name: "Dust",
        hint: "Fine warm dust hanging in the air, catching the light in specks and clumps",
        shape: VolumeShape::Box,
        colour: [1.0, 0.9, 0.75],
        lengths: &[
            ("size_x", 3000.0),
            ("size_y", 2000.0),
            ("size_z", 2000.0),
            ("noise_size", 60.0),
            ("drift_x", 15.0),
            ("drift_y", -5.0),
        ],
        values: &[
            ("density", 0.3),
            ("absorption", 0.1),
            ("forward", 0.6),
            ("noise", 0.85),
            ("detail", 4.0),
            ("softness", 0.3),
        ],
    },
];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_look_names_real_settings_within_their_limits() {
        let fresh =
            || match serde_json::from_value(veylvik_document::nodes::VOLUME_3D.default_operation())
            {
                Ok(Operation::Volume3d(v)) => v,
                _ => panic!("a fresh volume"),
            };
        for look in &LOOKS {
            for (name, _) in look.lengths.iter().chain(look.values) {
                assert!(
                    fresh().scalar(name).is_some(),
                    "{}: no setting {name}",
                    look.name
                );
            }
            let mut node = fresh();
            node.seed = 7;
            look.apply(&mut node, 3840);
            assert_eq!(node.seed, 7, "{}: keeps the seed", look.name);
            let op = Operation::Volume3d(node);
            assert!(
                op.validate().is_ok(),
                "{}: within limits at 4K: {:?}",
                look.name,
                op.validate()
            );
        }
    }
}
