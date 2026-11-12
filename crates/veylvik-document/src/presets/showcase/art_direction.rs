//! Art-directed, editable compositions. Particles support a subject, rather than
//! serving as the entire demonstration. No raster artwork or external fonts.
use super::*;
use crate::{BlendMode, GradientMode};
struct Art {
    doc: Composition,
}
impl Art {
    fn node(&mut self, name: &str, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
        add(&mut self.doc, name, op, inputs)
    }
    fn solid(&mut self, name: &str, color: [f32; 4]) -> NodeId {
        self.node(name, Operation::Solid { color }, &[])
    }
    fn path(&mut self, name: &str, commands: Vec<PathCommand>) -> NodeId {
        self.node(
            name,
            Operation::Path {
                geometry: PathMask {
                    fill_rule: FillRule::NonZero,
                    commands,
                },
            },
            &[],
        )
    }
    fn mask(&mut self, name: &str, image: NodeId, mask: NodeId) -> NodeId {
        self.node(
            name,
            Operation::Mask {},
            &[("image", image), ("mask", mask)],
        )
    }
    fn over(&mut self, name: &str, bg: NodeId, fg: NodeId) -> NodeId {
        self.node(
            name,
            Operation::Merge {},
            &[("background", bg), ("foreground", fg)],
        )
    }
    fn plus(&mut self, name: &str, bg: NodeId, fg: NodeId, amount: f32) -> NodeId {
        self.node(
            name,
            Operation::Blend {
                mode: BlendMode::Add,
                opacity: amount.into(),
            },
            &[("background", bg), ("foreground", fg)],
        )
    }
    fn blur(&mut self, image: NodeId, r: f32) -> NodeId {
        self.node(
            "Soft diffusion",
            Operation::Blur { radius: r.into() },
            &[("image", image)],
        )
    }
    fn glow(&mut self, image: NodeId, r: f32) -> NodeId {
        let a = self.blur(image, r);
        let b = self.blur(a, r * 0.5);
        self.plus("Radiant halo", image, b, 0.8)
    }
    fn tint(&mut self, name: &str, image: NodeId, low: [f32; 4], high: [f32; 4]) -> NodeId {
        self.node(name, Operation::Colorize { low, high }, &[("image", image)])
    }
    fn ellipse(
        &mut self,
        name: &str,
        x: Scalar,
        y: Scalar,
        rx: Scalar,
        ry: Scalar,
        f: f32,
    ) -> NodeId {
        self.node(
            name,
            Operation::Ellipse {
                x,
                y,
                radius_x: rx,
                radius_y: ry,
                feather: f.into(),
            },
            &[],
        )
    }
    fn gradient(&mut self, name: &str, start: f32, end: f32) -> NodeId {
        self.node(
            name,
            Operation::Gradient {
                mode: GradientMode::Linear,
                start_x: 0.0.into(),
                start_y: start.into(),
                end_x: 0.0.into(),
                end_y: end.into(),
            },
            &[],
        )
    }
    fn palette(
        &mut self,
        name: &str,
        start: f32,
        end: f32,
        low: [f32; 4],
        high: [f32; 4],
    ) -> NodeId {
        let g = self.gradient(name, start, end);
        self.tint(name, g, low, high)
    }
    fn move_art(
        &mut self,
        name: &str,
        image: NodeId,
        offset: [Scalar; 2],
        scale: [Scalar; 2],
        angle: Scalar,
        pivot: [f32; 2],
    ) -> NodeId {
        self.node(
            name,
            Operation::Affine {
                x: offset[0].clone(),
                y: offset[1].clone(),
                scale_x: scale[0].clone(),
                scale_y: scale[1].clone(),
                rotation: angle,
                pivot_x: pivot[0].into(),
                pivot_y: pivot[1].into(),
            },
            &[("image", image)],
        )
    }
    fn noise(&mut self, scale: f32, seed: u32) -> NodeId {
        self.node(
            "Animated material turbulence",
            Operation::FractalNoise {
                seed,
                scale: scale.into(),
                octaves: 4.0.into(),
                roughness: 0.58.into(),
                evolution: cycle(0.0, 1.8),
            },
            &[],
        )
    }
    fn displace(&mut self, name: &str, image: NodeId, map: NodeId, x: f32, y: f32) -> NodeId {
        self.node(
            name,
            Operation::Displace {
                x: x.into(),
                y: y.into(),
            },
            &[("image", image), ("map", map)],
        )
    }
    fn text(&mut self, word: &str) -> NodeId {
        self.node(
            &format!("{word} · editable lettering"),
            Operation::Path {
                geometry: lettering(word),
            },
            &[],
        )
    }
    fn atmosphere(
        &mut self,
        bg: NodeId,
        color: [f32; 4],
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
    ) -> NodeId {
        let ink = self.solid("Atmosphere color", color);
        let shape = self.ellipse(
            "Atmosphere falloff",
            x.into(),
            y.into(),
            rx.into(),
            ry.into(),
            rx * 0.7,
        );
        let light = self.mask("Atmosphere", ink, shape);
        self.plus("Atmospheric light", bg, light, 0.6)
    }
    fn material(&mut self, mask: NodeId, palette: NodeId) -> NodeId {
        self.mask("Material inside lettering", palette, mask)
    }
}
fn wave(a: f32, b: f32, phase: usize) -> Scalar {
    Scalar::Smooth {
        keys: (0..=8)
            .map(|i| {
                let t = ((i + phase) % 8) as f32 / 8.0;
                Key {
                    time: Time {
                        ticks: i as i64,
                        rate: 2,
                    },
                    value: a + (b - a) * (0.5 - 0.5 * (t * std::f32::consts::TAU).cos()),
                    segment: None,
                }
            })
            .collect(),
    }
}
fn outline(a: &mut Art, mask: NodeId, dx: f32, dy: f32, color: [f32; 4]) -> NodeId {
    let shifted = a.node(
        "Bevel offset",
        Operation::Transform {
            x: dx.into(),
            y: dy.into(),
        },
        &[("image", mask)],
    );
    let diff = a.node(
        "Bevel edge",
        Operation::CombineMask {
            mode: crate::MaskMode::Subtract,
        },
        &[("mask", mask), ("other", shifted)],
    );
    let ink = a.solid("Bevel light", color);
    a.mask("Polished edge", ink, diff)
}
fn gold_fire(a: &mut Art, bg: NodeId) -> NodeId {
    let mask = a.text("GOLD");
    let bg = a.atmosphere(bg, [0.12, 0.012, 0.001, 1.0], 480.0, 300.0, 320.0, 130.0);
    // Two gradients meet at a sharp polished-metal reflection band.
    let top = a.palette(
        "Gold upper reflection",
        200.0,
        270.0,
        [1.5, 0.7, 0.12, 1.0],
        [0.12, 0.028, 0.002, 1.0],
    );
    let bottom = a.palette(
        "Gold lower reflection",
        270.0,
        340.0,
        [2.8, 1.7, 0.42, 1.0],
        [0.38, 0.11, 0.006, 1.0],
    );
    let mut commands = vec![];
    polygon(
        &mut commands,
        &[[0.0, 270.0], [960.0, 270.0], [960.0, 540.0], [0.0, 540.0]],
    );
    let lower = a.path("Metal reflection boundary", commands);
    let bottom = a.mask("Lower gold band", bottom, lower);
    let gold = a.over("Reflective gold", top, bottom);
    let face = a.material(mask, gold);
    let mut extrusion = a.solid("Transparent depth", [0.0; 4]);
    let bronze = a.solid("Bronze sides", [0.16, 0.045, 0.003, 1.0]);
    let side = a.mask("Metal side face", bronze, mask);
    for i in (1..=5).rev() {
        let shifted = a.node(
            "Extruded gold depth",
            Operation::Transform {
                x: (i as f32 * 1.5).into(),
                y: (i as f32 * 2.0).into(),
            },
            &[("image", side)],
        );
        extrusion = a.over("Layered depth", extrusion, shifted);
    }
    let fire = a.node(
        "Fire · rising turbulent volume",
        Operation::Fire {
            seed: 297,
            x: 480.0.into(),
            y: 285.0.into(),
            width: 660.0.into(),
            height: 245.0.into(),
            evolution: Scalar::Linear {
                keys: vec![
                    Key {
                        time: Time { ticks: 0, rate: 1 },
                        value: 0.0,
                        segment: None,
                    },
                    Key {
                        time: Time { ticks: 4, rate: 1 },
                        value: 4.0,
                        segment: None,
                    },
                ],
            },
            intensity: 1.1.into(),
            turbulence: 1.4.into(),
        },
        &[],
    );
    let halo = a.blur(fire, 6.0);
    let fire = a.plus("Subtle fire scattering", fire, halo, 0.12);
    let bg = a.plus("Fire behind gold", bg, fire, 0.9);
    let depth = a.over("Gold depth over flame", bg, extrusion);
    let face = a.over("Gold face", depth, face);
    let rim = outline(a, mask, 2.0, 3.0, [3.0, 1.9, 0.55, 1.0]);
    let face = a.plus("Gold bevel highlights", face, rim, 0.8);
    // Ground reflection is deliberately dim and blurred, giving the title weight.
    let reflection = a.move_art(
        "Inverted reflection",
        face,
        [0.0.into(), 110.0.into()],
        [1.0.into(), (-0.35).into()],
        0.0.into(),
        [480.0, 340.0],
    );
    let reflection = a.blur(reflection, 2.0);
    a.plus("Smoky reflected light", face, reflection, 0.10)
}
fn dripping(a: &mut Art, bg: NodeId, pink: bool) -> NodeId {
    let letters = a.text("DRIP");
    let bg = a.atmosphere(
        bg,
        if pink {
            [0.16, 0.005, 0.16, 1.0]
        } else {
            [0.012, 0.1, 0.03, 1.0]
        },
        480.0,
        340.0,
        350.0,
        120.0,
    );
    let mut mask = letters;
    // Roots lie on the actual lower strokes of D, R, I and P.
    for (i, x) in [
        180.0, 206.0, 275.0, 338.0, 374.0, 438.0, 501.0, 540.0, 596.0, 660.0, 674.0,
    ]
    .into_iter()
    .enumerate()
    {
        let root = 330.0;
        let extent = 32.0 + (i % 3) as f32 * 12.0;
        let shape = a.ellipse(
            "Viscous hanging drip",
            x.into(),
            wave(root + 8.0, root + extent, i),
            (6.0 + (i % 3) as f32 * 2.0).into(),
            wave(8.0, extent, i),
            2.0,
        );
        mask = a.over("Liquid stretched from lettering", mask, shape);
        let phase = i * 19 % 96;
        let u = |f: usize| ((f + phase) % 96) as f32 / 96.0;
        let sampled = |f: &dyn Fn(usize) -> f32| Scalar::Linear {
            keys: (0..=96)
                .map(|frame| Key {
                    time: Time {
                        ticks: frame as i64,
                        rate: 24,
                    },
                    value: f(frame % 96),
                    segment: None,
                })
                .collect(),
        };
        let drop = a.ellipse(
            "Falling droplet",
            x.into(),
            sampled(&|f| root + 28.0 + u(f) * u(f) * (460.0 - root - 28.0)),
            (5.0 + (i % 2) as f32).into(),
            sampled(&|f| 6.0 + 5.0 * u(f)),
            2.0,
        );
        let fade = sampled(&|f| (u(f) / 0.1).min((1.0 - u(f)) / 0.12).clamp(0.0, 1.0));
        mask = a.node(
            "Droplet lifetime",
            Operation::Blend {
                mode: BlendMode::Normal,
                opacity: fade,
            },
            &[("background", mask), ("foreground", drop)],
        );
    }
    let noise = a.noise(36.0, 821);
    let mask = a.displace("Liquid surface tension", mask, noise, 4.0, 3.0);
    let (dark, light) = if pink {
        ([0.15, 0.001, 0.025, 1.0], [2.0, 0.12, 0.65, 1.0])
    } else {
        ([0.008, 0.12, 0.008, 1.0], [0.6, 1.4, 0.04, 1.0])
    };
    let ink = a.palette("Glossy liquid material", 185.0, 400.0, light, dark);
    let face = a.material(mask, ink);
    let edge = outline(
        a,
        mask,
        -3.0,
        -2.0,
        if pink {
            [2.0, 0.6, 1.2, 1.0]
        } else {
            [1.5, 2.0, 0.4, 1.0]
        },
    );
    let face = a.plus("Wet specular rim", face, edge, 0.8);
    let bg = a.over("Liquid title", bg, face);
    let puddle = a.ellipse(
        "Liquid pool",
        480.0.into(),
        465.0.into(),
        cycle(245.0, 275.0),
        12.0.into(),
        8.0,
    );
    let ink = a.solid("Pool tint", dark);
    let puddle = a.mask("Glossy floor pool", ink, puddle);
    a.plus("Pool under drips", bg, puddle, 0.6)
}
fn sand(a: &mut Art, bg: NodeId) -> NodeId {
    let mask = a.text("DUST");
    let noise = a.noise(3.5, 47);
    let grain = a.tint(
        "Sand grain color",
        noise,
        [0.07, 0.025, 0.006, 1.0],
        [1.3, 0.65, 0.18, 1.0],
    );
    let text = a.material(mask, grain);
    let rim = outline(a, mask, 2.0, 3.0, [1.8, 0.95, 0.3, 1.0]);
    let text = a.plus("Sandstone relief", text, rim, 0.6);
    let bg = a.atmosphere(bg, [0.13, 0.04, 0.007, 1.0], 480.0, 340.0, 350.0, 160.0);
    let bg = a.over("Carved sand title", bg, text);
    let dunes = a.ellipse(
        "Wind carved dune",
        490.0.into(),
        490.0.into(),
        520.0.into(),
        90.0.into(),
        25.0,
    );
    let ink = a.palette(
        "Dune material",
        400.0,
        540.0,
        [0.15, 0.06, 0.015, 1.0],
        [0.012, 0.004, 0.001, 1.0],
    );
    let dunes = a.mask("Sand dune", ink, dunes);
    a.over("Dune foreground", bg, dunes)
}
fn winter(a: &mut Art, bg: NodeId) -> NodeId {
    let bg = a.atmosphere(bg, [0.015, 0.12, 0.3, 1.0], 480.0, 270.0, 360.0, 220.0);
    let mut peaks = vec![];
    polygon(
        &mut peaks,
        &[
            [0.0, 470.0],
            [140.0, 340.0],
            [240.0, 420.0],
            [360.0, 320.0],
            [510.0, 440.0],
            [670.0, 335.0],
            [850.0, 425.0],
            [960.0, 360.0],
            [960.0, 540.0],
            [0.0, 540.0],
        ],
    );
    let shape = a.path("Distant mountain silhouettes", peaks);
    let ink = a.palette(
        "Cold mountains",
        320.0,
        540.0,
        [0.08, 0.17, 0.29, 1.0],
        [0.004, 0.015, 0.04, 1.0],
    );
    let mountains = a.mask("Mountain material", ink, shape);
    let bg = a.over("Winter landscape", bg, mountains);
    let mask = a.text("SNOW");
    let noise = a.noise(9.0, 982);
    let frost = a.tint(
        "Crystalline ice",
        noise,
        [0.025, 0.12, 0.3, 1.0],
        [0.45, 0.85, 1.3, 1.0],
    );
    let text = a.material(mask, frost);
    let edge = outline(a, mask, -2.0, 4.0, [1.5, 1.8, 2.0, 1.0]);
    let text = a.plus("Frosted bevel", text, edge, 0.8);
    a.over("Frozen title", bg, text)
}
fn nova(a: &mut Art, bg: NodeId) -> NodeId {
    let outer = a.ellipse(
        "Nova outer disk",
        480.0.into(),
        265.0.into(),
        155.0.into(),
        155.0.into(),
        5.0,
    );
    let inner = a.ellipse(
        "Nova eclipse",
        480.0.into(),
        265.0.into(),
        142.0.into(),
        142.0.into(),
        3.0,
    );
    let ring = a.node(
        "Plasma rim",
        Operation::CombineMask {
            mode: crate::MaskMode::Subtract,
        },
        &[("mask", outer), ("other", inner)],
    );
    let turbulence = a.noise(40.0, 71);
    let ring = a.displace("Unstable plasma", ring, turbulence, 14.0, 14.0);
    let fire = a.palette(
        "Cosmic temperature",
        100.0,
        430.0,
        [0.15, 0.4, 2.5, 1.0],
        [2.0, 0.03, 0.5, 1.0],
    );
    let ring = a.mask("Radiant plasma", fire, ring);
    let ring = a.glow(ring, 15.0);
    let bg = a.atmosphere(bg, [0.12, 0.007, 0.2, 1.0], 480.0, 265.0, 230.0, 200.0);
    let bg = a.plus("Nova corona", bg, ring, 1.0);
    let mask = a.text("NOVA");
    let ink = a.solid("Nova typography", [1.1, 1.3, 1.8, 1.0]);
    let text = a.material(mask, ink);
    let text = a.move_art(
        "Title across eclipse",
        text,
        [0.0.into(), 10.0.into()],
        [0.72.into(), 0.72.into()],
        0.0.into(),
        [480.0, 270.0],
    );
    a.over("Nova title", bg, text)
}
pub(super) fn build(id: &str) -> Result<Scene> {
    let mut doc = crate::blank(960, 540, Time::new(24, 1)?)?;
    doc.frame_count = 96;
    doc.nodes.clear();
    let mut a = Art { doc };
    let bg = a.solid("Deep cinematic background", [0.001, 0.002, 0.006, 1.0]);
    let before = match id {
        "embers" => gold_fire(&mut a, bg),
        "fireflies" => dripping(&mut a, bg, false),
        "liquid_shimmer" => dripping(&mut a, bg, true),
        "dust_motes" => sand(&mut a, bg),
        "snowfall" => winter(&mut a, bg),
        "starfield" => nova(&mut a, bg),
        _ => unreachable!(),
    };
    a.doc.output = before;
    let preset = super::super::builtin::build(id)?;
    let instance = preset.instantiate(
        &a.doc,
        &[("input_1".into(), before)].into(),
        Some(NodePosition { x: 0.0, y: 1500.0 }),
    )?;
    let document = Document::new(a.doc)?;
    let mut composition = document.prepare(0, &instance.commands)?;
    composition.output = instance.output;
    composition.validate()?;
    Ok(Scene {
        composition,
        before,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn droplets_fall_and_hide_the_loop_reset() {
        let scene = build("liquid_shimmer").unwrap();
        let doc = &scene.composition;
        let drop = doc
            .nodes
            .values()
            .find(|n| match &n.operation {
                Operation::Ellipse { x, .. } => {
                    n.label == "Falling droplet" && x.at(Time::new(0, 1).unwrap()).unwrap() == 180.0
                }
                _ => false,
            })
            .unwrap();
        let Operation::Ellipse { y, .. } = &drop.operation else {
            unreachable!()
        };
        let at = |frame| y.at(Time::new(frame, 24).unwrap()).unwrap();
        assert!(at(1) < at(48) && at(48) < at(80));
        assert_eq!(at(0), at(96));
        let lifetime = doc
            .nodes
            .values()
            .find(|n| n.label == "Droplet lifetime" && n.inputs.get("foreground") == Some(&drop.id))
            .unwrap();
        let Operation::Blend { opacity, .. } = &lifetime.operation else {
            unreachable!()
        };
        assert_eq!(opacity.at(Time::new(0, 1).unwrap()).unwrap(), 0.0);
        assert_eq!(opacity.at(Time::new(4, 1).unwrap()).unwrap(), 0.0);
        assert_eq!(opacity.at(Time::new(2, 1).unwrap()).unwrap(), 1.0);
    }
}
