//! Additional presets are ordinary editable graphs, including deterministic particle overlays.
use super::Builder;
use crate::{
    BlendMode, Key, MaskChannel, NodeId, Operation, Result, Scalar, Time,
    vector::{FillRule, PathCommand, PathMask},
};
fn cycle(a: f32, b: f32) -> Scalar {
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
fn sampled(f: impl Fn(usize) -> f32) -> Scalar {
    Scalar::Linear {
        keys: (0..=96)
            .map(|i| Key {
                time: Time {
                    ticks: i as i64,
                    rate: 24,
                },
                value: f(i % 96),
                segment: None,
            })
            .collect(),
    }
}
impl Builder {
    fn opaque_field(&mut self, image: NodeId) -> NodeId {
        let black = self.add(
            "Opaque field background",
            Operation::Solid(crate::nodes::Solid {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
            &[],
        );
        self.blend(black, image, BlendMode::Normal, 1.0.into())
    }

    fn blur(&mut self, image: NodeId, radius: f32) -> NodeId {
        self.add(
            "Diffusion",
            Operation::Blur(crate::nodes::Blur {
                radius: radius.into(),
                filter: Default::default(),
            }),
            &[("image", image)],
        )
    }
    pub(super) fn tint(&mut self, image: NodeId, low: [f32; 4], high: [f32; 4]) -> NodeId {
        self.add(
            "Palette",
            Operation::Colorize(crate::nodes::Colorize { low, high }),
            &[("image", image)],
        )
    }
    pub(super) fn blend(
        &mut self,
        bg: NodeId,
        fg: NodeId,
        mode: BlendMode,
        opacity: Scalar,
    ) -> NodeId {
        self.add(
            "Layer mix",
            Operation::Blend(crate::nodes::Blend { mode, opacity }),
            &[("background", bg), ("foreground", fg)],
        )
    }
    fn shift(&mut self, image: NodeId, x: Scalar, y: Scalar) -> NodeId {
        self.add(
            "Offset",
            Operation::Transform(crate::nodes::Transform { x, y }),
            &[("image", image)],
        )
    }
    fn mask(&mut self, image: NodeId, mask: NodeId) -> NodeId {
        self.add(
            "Masked detail",
            Operation::Mask(crate::nodes::Mask {}),
            &[("image", image), ("mask", mask)],
        )
    }
    fn affine(&mut self, image: NodeId, scale: Scalar, angle: Scalar) -> NodeId {
        self.add(
            "Echo transform",
            Operation::Affine(crate::nodes::Affine {
                x: 0.0.into(),
                y: 0.0.into(),
                scale_x: scale.clone(),
                scale_y: scale,
                rotation: angle,
                pivot_x: 480.0.into(),
                pivot_y: 270.0.into(),
            }),
            &[("image", image)],
        )
    }
    fn mix_control(&mut self, id: NodeId) -> Result<()> {
        self.expose(id, "opacity", "Amount")
    }
    fn edge(&mut self, source: NodeId) -> NodeId {
        let shift = self.shift(source, 3.0.into(), 3.0.into());
        let diff = self.blend(source, shift, BlendMode::Difference, 1.0.into());
        let valid = self.mask(diff, shift);
        self.add(
            "Edge luminance",
            Operation::ChannelMask(crate::nodes::ChannelMask {
                channel: MaskChannel::Luminance,
            }),
            &[("image", valid)],
        )
    }
    fn bands(&mut self, spacing: usize, height: f32) -> NodeId {
        let mut commands = Vec::new();
        for y in (0..540).step_by(spacing) {
            let y = y as f32;
            commands.extend([
                PathCommand::Move { to: [0.0, y] },
                PathCommand::Line { to: [960.0, y] },
                PathCommand::Line {
                    to: [960.0, y + height],
                },
                PathCommand::Line {
                    to: [0.0, y + height],
                },
                PathCommand::Close {},
            ]);
        }
        self.add(
            "Horizontal bands",
            Operation::Path(crate::nodes::Path {
                geometry: PathMask {
                    keys: vec![],
                    fill_rule: FillRule::NonZero,
                    commands,
                },
            }),
            &[],
        )
    }
    fn flow(&mut self, source: NodeId, scale: f32, x: f32, y: f32) -> Result<NodeId> {
        let noise = self.add(
            "Flow field",
            Operation::FractalNoise(crate::nodes::FractalNoise {
                seed: 83,
                scale: scale.into(),
                octaves: 3.0.into(),
                roughness: 0.5.into(),
                evolution: cycle(0.0, 0.8),
            }),
            &[],
        );
        let solid = self.add(
            "Frame coverage",
            Operation::Solid(crate::nodes::Solid { color: [1.0; 4] }),
            &[],
        );
        let inset = self.add(
            "Protected boundary",
            Operation::Morphology(crate::nodes::Morphology {
                radius: (-32.0).into(),
            }),
            &[("image", solid)],
        );
        let soft = self.blur(inset, 6.0);
        let field = self.mask(noise, soft);
        let out = self.add(
            "Refraction",
            Operation::Displace(crate::nodes::Displace {
                x: x.into(),
                y: y.into(),
            }),
            &[("image", source), ("map", field)],
        );
        self.expose(out, "x", "Horizontal refraction")?;
        self.expose(out, "y", "Vertical refraction")?;
        self.expose(noise, "scale", "Texture size")?;
        self.expose(noise, "evolution", "Flow animation")?;
        Ok(out)
    }
}
pub(super) fn build(id: &str, b: &mut Builder, source: NodeId) -> Result<NodeId> {
    let out = match id {
        "halation" => {
            // Film's red halo: light passing the emulsion, bouncing off the film's back
            // and coming back red round bright highlights.
            let halo = b.add(
                "Red halo",
                Operation::Bloom(crate::nodes::Bloom {
                    threshold: 0.7.into(),
                    knee: 0.5.into(),
                    intensity: 0.45.into(),
                    radius: 28.0.into(),
                    streak: 0.0.into(),
                    tint: [1.0, 0.3, 0.12, 1.0],
                }),
                &[("image", source)],
            );
            b.expose(halo, "threshold", "From brightness")?;
            b.expose(halo, "intensity", "Strength")?;
            b.expose(halo, "radius", "Spread")?;
            // Bloom gives the picture with its halo: Mix fades between that and the picture.
            b.blend(source, halo, BlendMode::Normal, 1.0.into())
        }
        "soft_bloom" | "dream_focus" => {
            let soft = b.blur(source, if id == "soft_bloom" { 22.0 } else { 12.0 });
            b.expose(soft, "radius", "Softness")?;
            b.blend(
                source,
                soft,
                if id == "soft_bloom" {
                    BlendMode::Add
                } else {
                    BlendMode::Screen
                },
                if id == "soft_bloom" {
                    0.85.into()
                } else {
                    0.7.into()
                },
            )
        }
        "anamorphic_streaks" => {
            // Compress before blur, then expand: anisotropic diffusion without a new kernel.
            let compress = b.add(
                "Compress highlights",
                Operation::Affine(crate::nodes::Affine {
                    x: 0.0.into(),
                    y: 0.0.into(),
                    scale_x: 0.1.into(),
                    scale_y: 1.0.into(),
                    rotation: 0.0.into(),
                    pivot_x: 480.0.into(),
                    pivot_y: 270.0.into(),
                }),
                &[("image", source)],
            );
            let soft = b.blur(compress, 10.0);
            let stretch = b.add(
                "Stretch light",
                Operation::Affine(crate::nodes::Affine {
                    x: 0.0.into(),
                    y: 0.0.into(),
                    scale_x: 10.0.into(),
                    scale_y: 1.0.into(),
                    rotation: 0.0.into(),
                    pivot_x: 480.0.into(),
                    pivot_y: 270.0.into(),
                }),
                &[("image", soft)],
            );
            let color = b.tint(stretch, [0.0; 4], [0.2, 0.7, 1.8, 1.0]);
            b.expose(color, "high", "Streak color")?;
            b.expose(soft, "radius", "Streak width")?;
            b.blend(source, color, BlendMode::Add, 0.8.into())
        }
        "edge_engrave" | "silver_relief" => {
            let edges = b.edge(source);
            let edges = b.opaque_field(edges);
            let color = b.tint(
                edges,
                if id == "silver_relief" {
                    [0.04, 0.05, 0.07, 1.0]
                } else {
                    [0.002, 0.008, 0.018, 1.0]
                },
                if id == "silver_relief" {
                    [2.0, 2.1, 2.3, 1.0]
                } else {
                    [0.25, 1.2, 1.5, 1.0]
                },
            );
            b.expose(color, "high", "Edge color")?;
            b.blend(
                source,
                color,
                BlendMode::Normal,
                if id == "silver_relief" {
                    0.8.into()
                } else {
                    1.0.into()
                },
            )
        }
        "chromatic_echo" | "orbit_echo" => {
            let mut out = source;
            for (i, color) in [[0.05, 0.8, 1.5, 1.0], [1.5, 0.06, 0.35, 1.0]]
                .into_iter()
                .enumerate()
            {
                let sign = if i == 0 { 1.0 } else { -1.0 };
                let tinted = b.tint(source, [0.0; 4], color);
                let (x, y) = if id == "orbit_echo" {
                    (
                        sampled(|f| sign * 32.0 * (f as f32 * std::f32::consts::TAU / 96.0).cos()),
                        sampled(|f| sign * 22.0 * (f as f32 * std::f32::consts::TAU / 96.0).sin()),
                    )
                } else {
                    (cycle(sign * 5.0, sign * 25.0), 0.0.into())
                };
                let shifted = b.shift(tinted, x, y);
                out = b.blend(out, shifted, BlendMode::Add, 0.45.into());
                if i == 0 {
                    b.expose(shifted, "x", "Echo horizontal motion")?;
                    b.expose(shifted, "y", "Echo vertical motion")?;
                }
            }
            out
        }
        "radial_echo" => {
            let mut echo = source;
            for i in 1..=3 {
                let copy = b.affine(
                    source,
                    cycle(1.0 + i as f32 * 0.08, 1.0 + i as f32 * 0.24),
                    0.0.into(),
                );
                echo = b.blend(echo, copy, BlendMode::Add, (0.24 / i as f32).into());
                if i == 1 {
                    b.expose(copy, "scale_x", "Echo width")?;
                    b.expose(copy, "scale_y", "Echo height")?;
                }
            }
            echo
        }
        "duotone_print" => {
            let color = b.tint(source, [0.025, 0.008, 0.07, 1.0], [1.0, 0.38, 0.12, 1.0]);
            b.expose(color, "low", "Shadow ink")?;
            b.expose(color, "high", "Highlight ink")?;
            b.blend(source, color, BlendMode::Normal, 1.0.into())
        }
        "ink_bleed" => {
            let luminance = b.add(
                "Ink coverage",
                Operation::ChannelMask(crate::nodes::ChannelMask {
                    channel: MaskChannel::Luminance,
                }),
                &[("image", source)],
            );
            let grow = b.add(
                "Spread ink",
                Operation::Morphology(crate::nodes::Morphology { radius: 7.0.into() }),
                &[("image", luminance)],
            );
            let soft = b.blur(grow, 3.0);
            let field = b.opaque_field(soft);
            let color = b.tint(field, [0.8, 0.72, 0.5, 1.0], [0.006, 0.015, 0.025, 1.0]);
            b.expose(grow, "radius", "Ink spread")?;
            b.expose(soft, "radius", "Paper softness")?;
            b.blend(source, color, BlendMode::Normal, 1.0.into())
        }
        "light_sweep" | "soft_vignette" | "spotlight" => {
            let (x, y, rx, ry, feather) = match id {
                "light_sweep" => (cycle(80.0, 880.0), 270.0.into(), 80.0, 500.0, 60.0),
                "spotlight" => (
                    cycle(240.0, 720.0),
                    cycle(200.0, 340.0),
                    260.0,
                    210.0,
                    120.0,
                ),
                _ => (480.0.into(), 270.0.into(), 320.0, 200.0, 160.0),
            };
            let shape = b.add(
                "Light shape",
                Operation::Ellipse(crate::nodes::Ellipse {
                    x,
                    y,
                    radius_x: rx.into(),
                    radius_y: ry.into(),
                    feather: feather.into(),
                }),
                &[],
            );
            b.expose(shape, "x", "Light center X")?;
            b.expose(shape, "y", "Light center Y")?;
            b.expose(shape, "feather", "Feather")?;
            if id == "light_sweep" {
                let lit = b.mask(source, shape);
                b.blend(source, lit, BlendMode::Add, 1.0.into())
            } else {
                let shape = b.opaque_field(shape);
                let field = b.tint(shape, [0.04, 0.04, 0.04, 1.0], [1.0; 4]);
                b.blend(source, field, BlendMode::Multiply, 0.9.into())
            }
        }
        "film_grain" => {
            let noise = b.add(
                "Fine grain",
                Operation::FractalNoise(crate::nodes::FractalNoise {
                    seed: 127,
                    scale: 2.0.into(),
                    octaves: 2.0.into(),
                    roughness: 0.65.into(),
                    evolution: cycle(0.0, 3.0),
                }),
                &[],
            );
            let color = b.tint(noise, [0.0, 0.0, 0.0, 1.0], [0.3, 0.3, 0.3, 1.0]);
            b.expose(noise, "scale", "Grain size")?;
            b.blend(source, color, BlendMode::Screen, 0.08.into())
        }
        "frosted_glass" | "heat_haze" | "underwater" => {
            let (scale, x, y) = match id {
                "frosted_glass" => (8.0, 6.0, 6.0),
                "heat_haze" => (55.0, 5.0, 28.0),
                _ => (130.0, 24.0, 14.0),
            };
            let distorted = b.flow(source, scale, x, y)?;
            let treated = if id == "frosted_glass" {
                let soft = b.blur(distorted, 2.0);
                b.expose(soft, "radius", "Glass diffusion")?;
                soft
            } else if id == "underwater" {
                b.tint(distorted, [0.0, 0.008, 0.018, 1.0], [0.15, 0.85, 1.3, 1.0])
            } else {
                distorted
            };
            b.blend(source, treated, BlendMode::Normal, 1.0.into())
        }
        "scanlines" | "hologram" => {
            let bands = b.bands(8, 2.0);
            let field = b.tint(bands, [1.0; 4], [0.05, 0.05, 0.05, 1.0]);
            let input = if id == "hologram" {
                b.tint(source, [0.0, 0.004, 0.014, 1.0], [0.1, 1.0, 1.5, 1.0])
            } else {
                source
            };
            let scan = b.blend(input, field, BlendMode::Multiply, 0.65.into());
            if id == "hologram" {
                let ghost = b.shift(scan, cycle(-6.0, 10.0), 0.0.into());
                b.blend(scan, ghost, BlendMode::Add, 0.35.into())
            } else {
                scan
            }
        }
        "glitch_slices" => {
            let bands = b.bands(72, 22.0);
            let slice = b.mask(source, bands);
            let shifted = b.shift(slice, cycle(-28.0, 35.0), 0.0.into());
            b.expose(shifted, "x", "Slice motion")?;
            b.blend(source, shifted, BlendMode::Normal, 1.0.into())
        }
        "dust_motes" | "fireflies" | "embers" | "snowfall" | "starfield" => {
            return particles(id, b, source);
        }
        _ => return Err(crate::Error("unknown starter effect".into())),
    };
    b.mix_control(out)?;
    Ok(out)
}
fn particles(id: &str, b: &mut Builder, source: NodeId) -> Result<NodeId> {
    if id == "dust_motes" {
        let dust = b.add(
            "Depth-layered airborne dust",
            Operation::Dust(crate::nodes::Dust {
                seed: 47,
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
                density: 0.72.into(),
                size: 1.25.into(),
                drift: 1.0.into(),
            }),
            &[],
        );
        for (parameter, label) in [
            ("density", "Dust density"),
            ("size", "Particle size"),
            ("drift", "Air movement"),
            ("evolution", "Evolution"),
        ] {
            b.expose(dust, parameter, label)?;
        }
        let out = b.blend(source, dust, BlendMode::Normal, 1.0.into());
        b.mix_control(out)?;
        return Ok(out);
    }

    let mut layer = b.add(
        "Transparent particle layer",
        Operation::Solid(crate::nodes::Solid { color: [0.0; 4] }),
        &[],
    );
    for i in 0..18 {
        let phase = i * 37 % 96;
        let u = |frame: usize| ((frame + phase) % 96) as f32 / 96.0;
        let px = 70.0 + (i * 137 % 820) as f32;
        let py = 50.0 + (i * 83 % 440) as f32;
        let angle = i as f32 * 2.399963;
        let x = sampled(|f| match id {
            "starfield" => 480.0 + angle.cos() * (20.0 + u(f) * 520.0),
            "embers" | "snowfall" => px + (u(f) * std::f32::consts::TAU).sin() * 25.0,
            _ => px + (u(f) * std::f32::consts::TAU).cos() * 30.0,
        });
        let y = sampled(|f| match id {
            "starfield" => 270.0 + angle.sin() * (10.0 + u(f) * 300.0),
            "embers" => 570.0 - u(f) * 600.0,
            "snowfall" => -30.0 + u(f) * 600.0,
            _ => py + (u(f) * std::f32::consts::TAU).sin() * 20.0,
        });
        let radius = match id {
            "dust_motes" => 4.0 + (i % 3) as f32 * 2.0,
            "snowfall" => 2.5 + (i % 3) as f32,
            "embers" => 0.45 + (i % 4) as f32 * 0.24,
            _ => 2.5,
        };
        let dot = b.add(
            &format!("Particle {:02} · path", i + 1),
            Operation::Ellipse(crate::nodes::Ellipse {
                x,
                y,
                radius_x: radius.into(),
                radius_y: (if id == "embers" { radius * 2.0 } else { radius }).into(),
                feather: (radius * 0.8).into(),
            }),
            &[],
        );
        // Fade to zero at each wrap. This hides respawns and makes frame 0 == 96.
        let fade = sampled(|f| {
            let t = u(f);
            ((t / 0.15).min((1.0 - t) / 0.15).min(1.0)).powi(2)
                * if id == "dust_motes" { 0.3 } else { 0.9 }
        });
        layer = b.blend(layer, dot, BlendMode::Normal, fade);
    }
    let color = match id {
        "dust_motes" => [0.7, 0.5, 0.3, 1.0],
        "fireflies" => [1.0, 1.6, 0.12, 1.0],
        "embers" => [2.5, 0.3, 0.025, 1.0],
        "snowfall" => [0.7, 0.85, 1.1, 1.0],
        _ => [0.4, 0.65, 1.8, 1.0],
    };
    let tinted = b.tint(layer, [0.0; 4], color);
    b.expose(tinted, "high", "Particle color")?;
    let halo = b.blur(tinted, if id == "fireflies" { 9.0 } else { 1.5 });
    b.expose(halo, "radius", "Glow spread")?;
    let halo = b.blur(halo, 4.0);
    let halo = b.blur(halo, 2.0);
    let glow = b.blend(
        tinted,
        halo,
        BlendMode::Add,
        (if id == "embers" { 0.18 } else { 1.0 }).into(),
    );
    b.expose(glow, "opacity", "Glow strength")?;
    let out = b.blend(source, glow, BlendMode::Add, 1.0.into());
    b.mix_control(out)?;
    Ok(out)
}
