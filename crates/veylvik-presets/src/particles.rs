//! Ready-made particle looks: each sets a Particles node's settings for a frame size, keeping its seed, so effects start from something that works.
use veylvik_document::{
    Operation,
    nodes::{NodeOp, Particles},
    registry,
};

/// What a look needs connected to do anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    Emitter,
    Target,
}
pub struct Look {
    pub name: &'static str,
    pub hint: &'static str,
    pub needs: Needs,
    apply: fn(&mut Particles, f32, f32),
}
impl Look {
    /// The node's settings for this look in a frame of `width` x `height` pixels.
    /// Lengths scale with the frame's width; the seed is kept.
    pub fn apply(&self, node: &mut Particles, width: u32, height: u32) {
        let seed = node.seed;
        let (x, y) = (node.x.clone(), node.y.clone());
        let Ok(Operation::Particles(fresh)) =
            serde_json::from_value(registry::PARTICLES.default_operation())
        else {
            return;
        };
        *node = fresh;
        node.seed = seed;
        // The emitter stays where the user put it unless the look places it.
        node.x = x;
        node.y = y;
        (self.apply)(node, width as f32, height as f32);
    }
}
fn set(node: &mut Particles, values: &[(&str, f32)]) {
    for (name, value) in values {
        if let Some(scalar) = node.scalar_mut(name) {
            *scalar = (*value).into();
        }
    }
}
pub static LOOKS: [Look; 11] = [
    Look {
        name: "Sparks",
        hint: "A shower of hot streaks thrown up from the emitter and falling back",
        needs: Needs::Nothing,
        apply: |n, w, _| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 1500.0),
                    ("life", 1.1),
                    ("width", 30.0 * s),
                    ("height", 4.0 * s),
                    ("speed", 1200.0 * s),
                    ("spread", 70.0),
                    ("gravity", 2200.0 * s),
                    ("size", 1.8 * s),
                    ("fade", 0.6),
                    ("variation", 0.8),
                    ("streak", 0.05),
                ],
            );
            n.colour = [9.0, 5.0, 1.6, 1.0];
            n.end_colour = [5.0, 0.9, 0.1, 1.0];
        },
    },
    Look {
        name: "Embers",
        hint: "Glowing specks drifting up from the bottom of the frame",
        needs: Needs::Nothing,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 520.0),
                    ("life", 4.0),
                    ("x", w / 2.0),
                    ("y", h),
                    ("width", w),
                    ("height", 20.0 * s),
                    ("speed", 170.0 * s),
                    ("spread", 80.0),
                    ("gravity", -160.0 * s),
                    ("turbulence", 200.0 * s),
                    ("turbulence_scale", 300.0 * s),
                    ("size", 2.8 * s),
                    ("fade", 0.5),
                    ("variation", 0.8),
                ],
            );
            n.colour = [6.0, 2.2, 0.5, 1.0];
            n.end_colour = [2.5, 0.3, 0.05, 1.0];
        },
    },
    Look {
        name: "Snow",
        hint: "Soft flakes falling from the top of the frame, drifting as they go",
        needs: Needs::Nothing,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 240.0),
                    ("life", 9.0),
                    ("x", w / 2.0),
                    ("y", -10.0 * s),
                    ("width", w * 1.2),
                    ("height", 10.0 * s),
                    ("speed", h / 8.0),
                    ("direction", 270.0),
                    ("spread", 30.0),
                    ("gravity", 6.0 * s),
                    ("turbulence", 70.0 * s),
                    ("turbulence_scale", 420.0 * s),
                    ("size", 3.2 * s),
                    ("fade", 0.1),
                    ("variation", 0.8),
                    ("glow", 0.0),
                ],
            );
            n.colour = [1.0, 1.0, 1.0, 0.9];
            n.end_colour = [1.0, 1.0, 1.0, 0.9];
        },
    },
    Look {
        name: "Rain",
        hint: "Fast thin streaks from the top of the frame",
        needs: Needs::Nothing,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 1800.0),
                    ("life", 0.9),
                    ("x", w / 2.0),
                    ("y", -10.0 * s),
                    ("width", w * 1.3),
                    ("height", 10.0 * s),
                    ("speed", h * 1.3),
                    ("direction", 264.0),
                    ("spread", 3.0),
                    ("gravity", 0.0),
                    ("size", 1.2 * s),
                    ("fade", 0.1),
                    ("variation", 0.4),
                    ("streak", 0.035),
                    ("glow", 0.0),
                ],
            );
            n.colour = [0.75, 0.85, 1.0, 0.45];
            n.end_colour = [0.75, 0.85, 1.0, 0.45];
        },
    },
    Look {
        name: "Confetti",
        hint: "Paper of every colour thrown up and fluttering down. Connect Sprite to a small shape for real pieces",
        needs: Needs::Nothing,
        apply: |n, w, _| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 500.0),
                    ("life", 3.5),
                    ("width", 200.0 * s),
                    ("height", 10.0 * s),
                    ("speed", 1300.0 * s),
                    ("spread", 60.0),
                    ("gravity", 1100.0 * s),
                    ("turbulence", 260.0 * s),
                    ("turbulence_scale", 240.0 * s),
                    ("size", 6.0 * s),
                    ("fade", 0.15),
                    ("variation", 0.6),
                    ("glow", 0.0),
                    ("colour_variation", 1.0),
                    ("spin", 400.0),
                ],
            );
            n.colour = [1.0, 0.25, 0.2, 1.0];
            n.end_colour = [1.0, 0.25, 0.2, 1.0];
        },
    },
    Look {
        name: "Magic dust",
        hint: "A slow swirling cloud of glowing motes round the emitter",
        needs: Needs::Nothing,
        apply: |n, w, _| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 2600.0),
                    ("life", 2.6),
                    ("width", 260.0 * s),
                    ("height", 260.0 * s),
                    ("speed", 110.0 * s),
                    ("spread", 360.0),
                    ("gravity", -40.0 * s),
                    ("turbulence", 320.0 * s),
                    ("turbulence_scale", 200.0 * s),
                    ("size", 2.2 * s),
                    ("fade", 0.8),
                    ("variation", 0.8),
                ],
            );
            n.colour = [2.6, 2.3, 3.6, 1.0];
            n.end_colour = [0.8, 0.4, 2.6, 1.0];
        },
    },
    Look {
        name: "Whirlpool",
        hint: "Particles drawn in and swung round the pull point",
        needs: Needs::Nothing,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 7000.0),
                    ("life", 4.5),
                    ("x", w / 2.0),
                    ("y", h / 2.0),
                    ("width", w),
                    ("height", h),
                    ("speed", 0.0),
                    ("gravity", 0.0),
                    ("pull_x", w / 2.0),
                    ("pull_y", h / 2.0),
                    ("pull", 520.0 * s),
                    ("vortex", 380.0 * s),
                    ("size", 1.5 * s),
                    ("fade", 0.3),
                    ("variation", 0.4),
                    ("streak", 0.06),
                ],
            );
            n.colour = [0.25, 0.7, 1.4, 1.0];
            n.end_colour = [1.1, 1.2, 1.4, 1.0];
        },
    },
    Look {
        name: "Disintegrate",
        hint: "The emitter's picture blows away as dust of its own colours. Connect Emitter",
        needs: Needs::Emitter,
        apply: |n, w, _| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("density", 2500.0),
                    ("life", 2.2),
                    ("speed", 60.0 * s),
                    ("spread", 360.0),
                    ("gravity", -90.0 * s),
                    ("wind", 140.0 * s),
                    ("turbulence", 240.0 * s),
                    ("turbulence_scale", 270.0 * s),
                    ("size", 1.9 * s),
                    ("inherit", 1.0),
                    ("fade", 0.7),
                    ("variation", 0.7),
                ],
            );
            n.colour = [1.6, 1.6, 1.6, 1.0];
            n.end_colour = [1.0, 1.0, 1.0, 1.0];
        },
    },
    Look {
        name: "Assemble",
        hint: "A burst of scattered particles gathers into the target's picture over two seconds. Connect Target. The burst holds Rate times Duration particles (48,000): raise Rate, up to 2,000,000, for a denser picture",
        needs: Needs::Target,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 400_000.0),
                    ("life", 2.0),
                    ("duration", 0.12),
                    ("x", w / 2.0),
                    ("y", h / 2.0),
                    ("width", w),
                    ("height", h),
                    ("speed", 180.0 * s),
                    ("spread", 360.0),
                    ("gravity", 0.0),
                    ("turbulence", 390.0 * s),
                    ("turbulence_scale", 390.0 * s),
                    ("size", 1.5 * s),
                    ("inherit", 1.0),
                    ("homing", 0.8),
                    ("fade", 0.0),
                    ("variation", 0.0),
                    ("streak", 0.03),
                    ("glow", 0.0),
                ],
            );
            n.colour = [1.0, 1.0, 1.0, 1.0];
            n.end_colour = [1.0, 1.0, 1.0, 1.0];
        },
    },
    Look {
        name: "Fireworks",
        hint: "Shells rise from the bottom of the frame and burst at the top of their climb into sparks trailing curved ribbons, each shell its own colour, that fall away. Each shell bursts where its life ends: Life sets the height, Burst the sparks in each",
        needs: Needs::Nothing,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 3.0),
                    ("life", 2.0),
                    ("x", w / 2.0),
                    ("y", h),
                    ("width", w * 0.6),
                    ("height", 0.0),
                    ("speed", 700.0 * s),
                    ("direction", 90.0),
                    ("spread", 16.0),
                    ("gravity", 330.0 * s),
                    ("size", 2.2 * s),
                    ("fade", 0.15),
                    ("variation", 0.3),
                    ("trail", 0.35),
                    ("trail_width", 0.8),
                    ("colour_variation", 0.9),
                    ("burst", 48.0),
                    ("burst_speed", 360.0 * s),
                    ("burst_spread", 360.0),
                    ("burst_life", 1.6),
                    ("burst_size", 1.0),
                ],
            );
            n.colour = [1.2, 0.9, 0.6, 1.0];
            n.end_colour = [1.2, 0.7, 0.4, 1.0];
            n.burst_colour = [1.6, 0.6, 0.25, 1.0];
        },
    },
    Look {
        name: "Network",
        hint: "Points drifting slowly across the frame, joined by lines wherever they come near one another: a web that forms and breaks as they move",
        needs: Needs::Nothing,
        apply: |n, w, h| {
            let s = w / 1920.0;
            set(
                n,
                &[
                    ("rate", 30.0),
                    ("life", 12.0),
                    ("start", -12.0),
                    ("x", w / 2.0),
                    ("y", h / 2.0),
                    ("width", w * 1.1),
                    ("height", h * 1.1),
                    ("speed", 25.0 * s),
                    ("spread", 360.0),
                    ("gravity", 0.0),
                    ("turbulence", 12.0 * s),
                    ("turbulence_scale", 600.0 * s),
                    ("size", 2.5 * s),
                    ("fade", 0.15),
                    ("variation", 0.5),
                    ("link_distance", 170.0 * s),
                    ("link_opacity", 0.55),
                    ("link_width", 1.2 * s),
                ],
            );
            n.colour = [0.55, 0.85, 1.0, 1.0];
            n.end_colour = [0.55, 0.85, 1.0, 1.0];
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_look_gives_a_valid_node_and_scales_with_the_frame() {
        for look in &LOOKS {
            for (w, h) in [(1920, 1080), (640, 360), (3840, 2160)] {
                let Ok(Operation::Particles(mut node)) =
                    serde_json::from_value(registry::PARTICLES.default_operation())
                else {
                    unreachable!()
                };
                node.seed = 7;
                look.apply(&mut node, w, h);
                assert_eq!(node.seed, 7);
                Operation::Particles(node)
                    .validate()
                    .unwrap_or_else(|e| panic!("{} at {w}x{h}: {e:?}", look.name));
            }
        }
        let size = |w: u32| {
            let Ok(Operation::Particles(mut node)) =
                serde_json::from_value(registry::PARTICLES.default_operation())
            else {
                unreachable!()
            };
            LOOKS[0].apply(&mut node, w, w * 9 / 16);
            node.size
                .at(veylvik_document::Time::new(0, 1).unwrap())
                .unwrap()
        };
        assert!((size(3840) - 2.0 * size(1920)).abs() < 1e-4);
    }
}
