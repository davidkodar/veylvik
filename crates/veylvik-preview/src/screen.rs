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
    /// A host's own display ([`HostEncode::identity`]) in place of the two above; 0 for
    /// Veylvik's.
    pub host: u64,
}
impl Look {
    /// The look to draw a restored picture with: everything is already in it.
    pub const NEUTRAL: Self = Self {
        exposure: 0.0,
        filmic: false,
        host: 0,
    };
}
/// A host's display: its own code turns the viewer's picture into what its screen shows,
/// so a page of the host shows a composition as the host's own viewer would.
pub trait HostEncode: Send + Sync {
    /// Says which display this is: the same number for the same display (working space,
    /// tone map, transfer, viewer LUT…), in any session; never 0.
    fn identity(&self) -> u64;
    /// `picture` (`width` x `height`, RGBA 32-bit float, scene-linear Rec.709,
    /// premultiplied, exposure applied) as the host's screen shows it: a texture of the
    /// same size in `Rgba8Unorm` or `Rgba8UnormSrgb`, usable as `TEXTURE_BINDING` and
    /// `COPY_SRC`, its colour premultiplied by its alpha. The work is submitted before
    /// returning; `picture` is not read after.
    fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        picture: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> std::result::Result<wgpu::Texture, String>;
}
pub struct Screen {
    pack: wgpu::ComputePipeline,
    unpack: wgpu::ComputePipeline,
    expose: wgpu::ComputePipeline,
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
            expose: pipeline("expose", text(include_str!("screen_expose.wgsl"))),
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
    /// `frame` with `exposure` as `host` shows it (see [`HostEncode::encode`]).
    pub fn host_texture(
        &self,
        renderer: &Renderer,
        frame: &GpuFrame,
        exposure: f32,
        host: &dyn HostEncode,
    ) -> Result<wgpu::Texture> {
        let (width, height) = (frame.width(), frame.height());
        let region = frame.region();
        if !renderer.owns(frame) || region.width != width || region.height != height {
            return Err(error(
                "only a whole frame of this renderer is shown by a host",
            ));
        }
        let look = Look {
            exposure,
            ..Look::NEUTRAL
        };
        renderer
            .compute(|scratch| {
                let exposed = scratch.buffer(
                    u64::from(width) * u64::from(height) * 16,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    "picture for the host's display",
                )?;
                let encoder = Self::run(
                    scratch,
                    &self.expose,
                    [width, height],
                    look,
                    frame.buffer(),
                    &exposed,
                );
                scratch.run(encoder)?;
                let texture = host
                    .encode(scratch.device(), scratch.queue(), &exposed, width, height)
                    .map_err(veylvik_render::Error::Message)?;
                let size = texture.size();
                if !matches!(
                    texture.format(),
                    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
                ) || [size.width, size.height] != [width, height]
                {
                    return Err(veylvik_render::Error::Message(format!(
                        "the host's display gave a {:?} picture of {} x {}, not Rgba8 of {width} x {height}",
                        texture.format(),
                        size.width,
                        size.height
                    )));
                }
                Ok(texture)
            })
            .map_err(error)
    }
    /// `frame` as the viewer shows it with `look`: four bytes a pixel, rows top first.
    /// A host's look is encoded by `host`, which must be the display it names.
    pub fn pack_with(
        &self,
        renderer: &Renderer,
        frame: &GpuFrame,
        look: Look,
        host: Option<&dyn HostEncode>,
    ) -> Result<Vec<u8>> {
        if look.host == 0 {
            return self.pack(renderer, frame, look);
        }
        let host = host
            .filter(|host| host.identity() == look.host)
            .ok_or_else(|| error("the host's display for this look is gone"))?;
        let texture = self.host_texture(renderer, frame, look.exposure, host)?;
        let (width, height) = (frame.width(), frame.height());
        // Rows of a texture copy are 256-byte aligned.
        let row = width * 4;
        let padded = row.div_ceil(256) * 256;
        renderer
            .compute(|scratch| {
                let readback = scratch.buffer(
                    u64::from(padded) * u64::from(height),
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    "host's screen picture read",
                )?;
                let mut encoder = scratch.device().create_command_encoder(&Default::default());
                encoder.copy_texture_to_buffer(
                    texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded),
                            rows_per_image: Some(height),
                        },
                    },
                    texture.size(),
                );
                let bytes = scratch.read::<u8>(encoder, &readback)?;
                // Rows copied whole: byte by byte, a 4K picture took about 29 ms.
                if padded == row {
                    return Ok(bytes);
                }
                let mut picture = Vec::with_capacity(row as usize * height as usize);
                for line in bytes.chunks(padded as usize) {
                    picture.extend_from_slice(&line[..row as usize]);
                }
                Ok(picture)
            })
            .map_err(error)
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
                host: 0,
            },
            Look {
                exposure: -0.5,
                filmic: true,
                host: 0,
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
            host: 0,
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
    /// A stand-in for a host's display: half the light, clamped, as eight bits.
    pub(crate) struct Halving(pub u64);
    impl HostEncode for Halving {
        fn identity(&self) -> u64 {
            self.0
        }
        fn encode(
            &self,
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            picture: &wgpu::Buffer,
            width: u32,
            height: u32,
        ) -> std::result::Result<wgpu::Texture, String> {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("a host's display"),
                source: wgpu::ShaderSource::Wgsl(
                    "@group(0) @binding(0) var<storage, read> picture: array<vec4<f32>>;
                     @group(0) @binding(1) var out: texture_storage_2d<rgba8unorm, write>;
                     @group(0) @binding(2) var<uniform> size: vec4<u32>;
                     @compute @workgroup_size(8, 8)
                     fn encode(@builtin(global_invocation_id) g: vec3<u32>) {
                         if g.x >= size.x || g.y >= size.y { return; }
                         let p = picture[g.y * size.x + g.x];
                         textureStore(out, vec2<i32>(g.xy), vec4<f32>(clamp(p.rgb * 0.5, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(p.a, 0.0, 1.0)));
                     }"
                    .into(),
                ),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &module,
                entry_point: Some("encode"),
                compilation_options: Default::default(),
                cache: None,
            });
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("the host's picture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            use wgpu::util::DeviceExt;
            let size = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&[width, height, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let view = texture.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: picture.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: size.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            queue.submit([encoder.finish()]);
            Ok(texture)
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_picture_kept_with_a_hosts_look_is_what_the_host_made_of_it() {
        let renderer = Renderer::new().unwrap();
        let screen = Screen::new(renderer.device());
        // 70 wide: rows are not a multiple of 256 bytes, as a texture copy aligns them.
        let (width, height) = (70u32, 9u32);
        let pixels: Vec<[f32; 4]> = (0..width * height)
            .map(|i| {
                [
                    (i % width) as f32 / 80.0,
                    0.4,
                    (i / width) as f32 / 9.0,
                    1.0,
                ]
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
        // One stop up, then the host halves: the source values, as eight bits.
        let look = Look {
            exposure: 1.0,
            filmic: true,
            host: 7,
        };
        let host = Halving(7);
        let packed = screen
            .pack_with(&renderer, &frame, look, Some(&host))
            .unwrap();
        assert_eq!(packed.len() as u32, width * height * 4);
        // Within one step of eight bits: a value exactly between two steps rounds as
        // the card rounds.
        for (i, p) in pixels.iter().enumerate() {
            let want = [p[0], p[1], p[2], 1.0].map(|v| (v * 255.0).round() as u8);
            let got = &packed[i * 4..i * 4 + 4];
            assert!(
                got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1),
                "pixel {i}: {got:?} != {want:?}"
            );
        }
        // Another display, or none, is refused rather than kept under the wrong look.
        assert!(
            screen
                .pack_with(&renderer, &frame, look, Some(&Halving(8)))
                .is_err()
        );
        assert!(screen.pack_with(&renderer, &frame, look, None).is_err());
    }
}
