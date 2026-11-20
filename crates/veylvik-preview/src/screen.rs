//! A finished frame as the viewer shows it: after exposure and the display transform, eight bits a channel, a quarter of the frame's size. Drawn again with a neutral look it is the picture that was on the screen: the same value where the frame is opaque and shown at its own size, within one step of eight bits elsewhere.
use crate::{Result, error};
use veylvik_render::{GpuFrame, Renderer};

/// What the viewer does to a frame before it reaches the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Stops of exposure.
    pub exposure: f32,
    /// The filmic display transform; otherwise colours clip at white.
    pub filmic: bool,
}
impl Look {
    /// The look to draw a restored picture with: everything is already in it.
    pub const NEUTRAL: Self = Self {
        exposure: 0.0,
        filmic: false,
    };
}
pub struct Screen {
    pack: wgpu::ComputePipeline,
    unpack: wgpu::ComputePipeline,
}
impl Screen {
    pub fn new(device: &wgpu::Device) -> Self {
        // One module for each direction: they bind different buffers at the same places.
        let text = |part: &str| format!("{}\n{part}", include_str!("screen.wgsl"));
        let pipeline = |entry: &str, source: String| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("screen picture"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("screen picture"),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            pack: pipeline("pack", text(include_str!("screen_pack.wgsl"))),
            unpack: pipeline("unpack", text(include_str!("screen_unpack.wgsl"))),
        }
    }
    fn run(
        scratch: &mut veylvik_render::compute::Scratch<'_, '_>,
        pipeline: &wgpu::ComputePipeline,
        [width, height]: [u32; 2],
        look: Look,
        from: &wgpu::Buffer,
        to: &wgpu::Buffer,
    ) -> wgpu::CommandEncoder {
        let device = scratch.device();
        let mut words = Vec::with_capacity(32);
        for v in [width, height, 0, 0] {
            words.extend_from_slice(&v.to_le_bytes());
        }
        for v in [
            look.exposure.exp2(),
            f32::from(u8::from(look.filmic)),
            0.0,
            0.0,
        ] {
            words.extend_from_slice(&v.to_le_bytes());
        }
        let settings = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen picture"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        scratch.queue().write_buffer(&settings, 0, &words);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen picture"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: settings.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: from.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: to.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder
    }
    /// `frame` as the viewer shows it with `look`: four bytes a pixel, rows top first.
    pub fn pack(&self, renderer: &Renderer, frame: &GpuFrame, look: Look) -> Result<Vec<u8>> {
        let (width, height) = (frame.width(), frame.height());
        let region = frame.region();
        if !renderer.owns(frame) || region.width != width || region.height != height {
            return Err(error(
                "only a whole frame of this renderer is kept for the screen",
            ));
        }
        let bytes = u64::from(width) * u64::from(height) * 4;
        renderer
            .compute(|scratch| {
                let packed = scratch.buffer(
                    bytes,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    "screen picture",
                )?;
                let readback = scratch.buffer(
                    bytes,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    "screen picture read",
                )?;
                let mut encoder = Self::run(
                    scratch,
                    &self.pack,
                    [width, height],
                    look,
                    frame.buffer(),
                    &packed,
                );
                encoder.copy_buffer_to_buffer(&packed, 0, &readback, 0, bytes);
                scratch.read::<u8>(encoder, &readback)
            })
            .map_err(error)
    }
    /// A frame from what [`Screen::pack`] gave, to be drawn with [`Look::NEUTRAL`].
    pub fn unpack(
        &self,
        renderer: &Renderer,
        picture: &[u8],
        width: u32,
        height: u32,
    ) -> Result<GpuFrame> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 || picture.len() as u64 != pixels * 4 {
            return Err(error("a screen picture does not match its size"));
        }
        renderer
            .compute(|scratch| {
                let words = scratch.buffer(
                    pixels * 4,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    "screen picture",
                )?;
                scratch.queue().write_buffer(&words, 0, picture);
                let restored = scratch.buffer(
                    pixels * 16,
                    wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                    "screen picture restored",
                )?;
                let encoder = Self::run(
                    scratch,
                    &self.unpack,
                    [width, height],
                    Look::NEUTRAL,
                    &words,
                    &restored,
                );
                scratch.run(encoder)?;
                scratch.frame(&restored, width, height)
            })
            .map_err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use veylvik_document::Time;
    use veylvik_render::{Image, ImageInputs, Plan, RenderRequest};

    /// What the viewer puts on the screen for one pixel at its own size, over black.
    fn shown(pixel: [f32; 4], look: Look) -> [u8; 3] {
        let alpha = pixel[3].clamp(0.0, 1.0);
        let gain = look.exposure.exp2();
        let mut colour = [0.0_f32; 3];
        if alpha > 0.0 {
            let straight = [0, 1, 2].map(|c| pixel[c] * gain / alpha);
            let mapped = if look.filmic {
                veylvik_render::display::filmic(straight)
            } else {
                straight.map(|v| v.clamp(0.0, 1.0))
            };
            colour = mapped.map(|v| v * alpha);
        }
        colour.map(|v| {
            let encoded = if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            (encoded * 255.0).round() as u8
        })
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_kept_picture_is_what_was_on_the_screen() {
        let renderer = Renderer::new().unwrap();
        let screen = Screen::new(renderer.device());
        let (width, height) = (64u32, 32u32);
        // Opaque pixels across the range, light far brighter than white included.
        let pixels: Vec<[f32; 4]> = (0..width * height)
            .map(|i| {
                let (x, y) = (
                    (i % width) as f32 / width as f32,
                    (i / width) as f32 / height as f32,
                );
                [x * x * 6.0, y, (1.0 - x) * 0.3, 1.0]
            })
            .collect();
        let mut doc = veylvik_document::blank(width, height, Time::new(24, 1).unwrap()).unwrap();
        let node = veylvik_document::Node::new(
            "picture",
            veylvik_document::Operation::Image(veylvik_document::nodes::Image {
                asset: "p".into(),
            }),
            &[],
        );
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "p",
            Arc::new(Image::new(width, height, pixels.clone()).unwrap()),
        );
        let frame = renderer
            .render_gpu(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        for look in [
            Look::NEUTRAL,
            Look {
                exposure: 1.5,
                filmic: false,
            },
            Look {
                exposure: -0.5,
                filmic: true,
            },
        ] {
            let packed = screen.pack(&renderer, &frame, look).unwrap();
            assert_eq!(packed.len() as u32, width * height * 4);
            let restored = screen.unpack(&renderer, &packed, width, height).unwrap();
            let back = renderer.read_frame(&restored).unwrap().pixels;
            let mut off = 0;
            for (i, pixel) in pixels.iter().enumerate() {
                // The screen's value for the frame itself, and for the kept picture drawn neutrally.
                let (before, after) = (shown(*pixel, look), shown(back[i], Look::NEUTRAL));
                let worst = (0..3).map(|c| before[c].abs_diff(after[c])).max().unwrap();
                // The shader and this test round a few values on the edge of a step differently.
                assert!(
                    worst <= 1,
                    "{look:?}: {pixel:?} was {before:?}, is {after:?}"
                );
                off += usize::from(worst != 0);
                assert_eq!(packed[i * 4 + 3], 255);
            }
            println!("{look:?}: {off} of {} pixels one step off", pixels.len());
            assert!(off * 100 <= pixels.len(), "{look:?}: {off} pixels differ");
        }
    }
    #[test]
    #[ignore = "requires Vulkan; prints timings"]
    fn how_long_a_4k_picture_takes_to_keep_and_to_bring_back() {
        let mut renderer = Renderer::new().unwrap();
        renderer.budget_bytes = 2 << 30;
        let screen = Screen::new(renderer.device());
        let (width, height) = (3840u32, 2160u32);
        let doc = veylvik_document::blank(width, height, Time::new(24, 1).unwrap()).unwrap();
        let frame = renderer
            .render_gpu(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap();
        let look = Look {
            exposure: 0.0,
            filmic: true,
        };
        let mut packed = Vec::new();
        for round in 0..5 {
            let start = std::time::Instant::now();
            packed = screen.pack(&renderer, &frame, look).unwrap();
            let kept = start.elapsed();
            let start = std::time::Instant::now();
            let restored = screen.unpack(&renderer, &packed, width, height).unwrap();
            println!(
                "round {round}: keep {:.1} ms, bring back {:.1} ms",
                kept.as_secs_f64() * 1e3,
                start.elapsed().as_secs_f64() * 1e3
            );
            drop(restored);
        }
        assert_eq!(packed.len(), 3840 * 2160 * 4);
    }
}
