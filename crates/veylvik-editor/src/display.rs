use crate::viewer::ViewTransform;
use eframe::{egui, egui_wgpu};
use std::sync::Arc;
use veylvik_render::{GpuFrame, Renderer};
use wgpu::util::DeviceExt;

/// What of the picture the viewer shows; anything but Colour is drawn as grey.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    #[default]
    Colour,
    Red,
    Green,
    Blue,
    Luminance,
}
impl Channel {
    pub const ALL: [Self; 5] = [
        Self::Colour,
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Luminance,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Colour => "Colour",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Luminance => "Luminance",
        }
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ViewSettings {
    pub exposure: f32,
    pub alpha: bool,
    pub fast_filter: bool,
    /// The composition's filmic display transform instead of clipping at white.
    pub filmic: bool,
    /// A single channel or the luminance as grey; coverage is `alpha`.
    pub channel: Channel,
    /// Behind transparent areas: 0 checkerboard, 1 black, 2 grey, 3 white.
    pub backdrop: u8,
    /// The host's display ([`veylvik_preview::screen::HostEncode::identity`]) in place of
    /// the clip, filmic or OCIO view; 0 for Veylvik's own.
    pub host: u64,
}
/// The host's picture of a frame, kept while the frame, the exposure and the display
/// stay the same: panning and zooming draw it again without asking the host.
type HostPicture = ((usize, u32, u64), wgpu::Texture);
pub struct Display {
    pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    host: Option<Arc<dyn crate::host::HostDisplay>>,
    /// Applies the exposure and asks the host for its picture.
    screen: veylvik_preview::screen::Screen,
    host_picture: std::sync::Mutex<Option<HostPicture>>,
    /// Bound where there is no host picture.
    nothing: wgpu::TextureView,
}
impl Display {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer display transform"),
            source: wgpu::ShaderSource::Wgsl(include_str!("display.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewer"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let nothing = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("no host picture"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            pipeline,
            format,
            host: None,
            screen: veylvik_preview::screen::Screen::new(device),
            host_picture: std::sync::Mutex::new(None),
            nothing,
        }
    }
    /// Draw with the host's display (see `Editor::set_host_display`).
    pub fn set_host(&mut self, host: Option<Arc<dyn crate::host::HostDisplay>>) {
        self.host = host;
        if let Ok(mut kept) = self.host_picture.lock() {
            *kept = None;
        }
    }
    /// The host's picture of `frame` with `exposure`, when the view asks for the host's
    /// display and it can give one; `None` draws with Veylvik's own.
    fn host_view(
        &self,
        renderer: &Renderer,
        frame: &Arc<GpuFrame>,
        exposure: f32,
        settings: &ViewSettings,
    ) -> Option<wgpu::Texture> {
        let host = self
            .host
            .as_ref()
            .filter(|h| settings.host != 0 && h.identity() == settings.host)?;
        let key = (
            Arc::as_ptr(frame) as usize,
            exposure.to_bits(),
            settings.host,
        );
        let mut kept = self.host_picture.lock().ok()?;
        if let Some((at, texture)) = kept.as_ref()
            && *at == key
        {
            return Some(texture.clone());
        }
        let texture = self
            .screen
            .host_texture(renderer, frame, exposure, host.as_ref())
            .ok()?;
        *kept = Some((key, texture.clone()));
        Some(texture)
    }
    pub fn frame(
        &self,
        renderer: &Renderer,
        frame: Arc<GpuFrame>,
        settings: ViewSettings,
        transform: ViewTransform,
    ) -> Arc<DisplayFrame> {
        self.frame_as(renderer, frame, settings, transform, false)
    }
    /// As [`Display::frame`]; `baked` says the frame is a played picture kept as the
    /// screen shows it, with exposure and the display transform already in it, so those
    /// two are not applied again. `settings` stay the viewer's own, for comparing.
    pub fn frame_as(
        &self,
        renderer: &Renderer,
        frame: Arc<GpuFrame>,
        asked: ViewSettings,
        transform: ViewTransform,
        baked: bool,
    ) -> Arc<DisplayFrame> {
        let settings = if baked {
            ViewSettings {
                exposure: 0.0,
                filmic: false,
                ..asked
            }
        } else {
            asked
        };
        let exposure = if settings.exposure.is_finite() {
            settings.exposure.clamp(-16.0, 16.0)
        } else {
            0.0
        };
        // A played picture already shows what the host's display made of it.
        let host_picture = if baked {
            None
        } else {
            self.host_view(renderer, &frame, exposure, &settings)
        };
        let host_flags = match &host_picture {
            Some(texture) => 512 | if texture.format().is_srgb() { 1024 } else { 0 },
            None => 0,
        };
        let uniform = renderer
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewer size"),
                contents: bytemuck::cast_slice(&[
                    frame.width(),
                    frame.height(),
                    u32::from(self.format.is_srgb())
                        | (u32::from(settings.alpha) << 1)
                        | (u32::from(settings.filmic) << 2)
                        | (u32::from(settings.fast_filter) << 3)
                        | ((settings.channel as u32) << 4)
                        | ((u32::from(settings.backdrop) & 3) << 7)
                        | host_flags,
                    exposure.to_bits(),
                    frame.region().x,
                    frame.region().y,
                    frame.region().width,
                    frame.region().height,
                    transform.0[0].to_bits(),
                    transform.0[1].to_bits(),
                    transform.0[2].to_bits(),
                    transform.0[3].to_bits(),
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let host_view = host_picture.as_ref().map_or_else(
            || self.nothing.clone(),
            |t| t.create_view(&Default::default()),
        );
        let bind = renderer
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewer frame"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame.buffer().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&host_view),
                    },
                ],
            });
        Arc::new(DisplayFrame {
            pipeline: self.pipeline.clone(),
            bind,
            frame,
            queue: renderer.queue().clone(),
            settings: asked,
            transform,
            baked,
        })
    }
}
pub struct DisplayFrame {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    pub frame: Arc<GpuFrame>,
    pub settings: ViewSettings,
    pub transform: ViewTransform,
    /// A played picture kept as the screen shows it: its values are not the renderer's.
    pub baked: bool,
    queue: wgpu::Queue,
}
impl Drop for DisplayFrame {
    fn drop(&mut self) {
        // The app retains this object across painting and releases it on a later
        // UI update, after eframe has submitted the previous frame's commands.
        let frame = self.frame.clone();
        self.queue.on_submitted_work_done(move || drop(frame));
    }
}
pub struct Paint(pub Arc<DisplayFrame>);
impl egui_wgpu::CallbackTrait for Paint {
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &egui_wgpu::CallbackResources,
    ) {
        pass.set_pipeline(&self.0.pipeline);
        pass.set_bind_group(0, &self.0.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Composition, Node, Operation, Time};
    use veylvik_render::{ImageInputs, Plan, RenderRequest};
    #[test]
    #[ignore = "requires Vulkan"]
    fn viewer_shader_applies_display_transform_on_shared_device() {
        let renderer = Renderer::new().unwrap();
        let node = Node::new(
            "red",
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [0.25, 0.0, 0.0, 1.0],
            }),
            &[],
        );
        let doc = Composition {
            published: Default::default(),
            markers: Default::default(),
            backdrops: Default::default(),
            ocio: Default::default(),
            working_space: Default::default(),
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 2,
            height: 2,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into_iter().collect(),
        };
        let frame = renderer
            .render_gpu(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap();
        let pixels = paint(
            &renderer,
            Arc::new(frame),
            2,
            2,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        assert!(pixels.iter().all(|pixel| pixel == &[137, 0, 0, 255]));
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn viewer_filters_linear_premultiplied_pixels_and_respects_crop_edges() {
        let renderer = Renderer::new().unwrap();
        let node = Node::new(
            "source",
            Operation::Image(veylvik_document::nodes::Image {
                asset: "source".into(),
            }),
            &[],
        );
        let doc = Composition {
            published: Default::default(),
            markers: Default::default(),
            backdrops: Default::default(),
            ocio: Default::default(),
            working_space: Default::default(),
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 4,
            height: 2,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into(),
        };
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "source",
            veylvik_render::Image::new(
                4,
                2,
                vec![
                    [1.0, 0.0, 1.0, 1.0],
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0; 4],
                    [1.0, 0.0, 1.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                ],
            )
            .unwrap()
            .into(),
        );
        let frame = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest {
                        time: Time::new(0, 1).unwrap(),
                        region: Some(veylvik_render::Region {
                            x: 1,
                            y: 0,
                            width: 2,
                            height: 1,
                        }),
                        scene_view: None,
                        scene_reflections: false,
                    },
                    &inputs,
                )
                .unwrap(),
        );
        let expected = veylvik_render::to_srgba8(&[
            [1.0, 0.0, 0.0, 1.0],
            [0.5375, 0.0375, 0.0375, 1.0],
            [0.075, 0.075, 0.075, 1.0],
        ]);
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let result = paint(&renderer, frame.clone(), 3, 1, format);
            for (actual, expected) in result.iter().flatten().zip(&expected) {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "{format:?}: {actual} != {expected}"
                );
            }
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn viewer_exposure_and_alpha_inspection_leave_hdr_frame_unchanged() {
        let renderer = Renderer::new().unwrap();
        let node = Node::new(
            "HDR",
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [4.0, 1.0, 0.0, 0.5],
            }),
            &[],
        );
        let doc = Composition {
            published: Default::default(),
            markers: Default::default(),
            backdrops: Default::default(),
            ocio: Default::default(),
            working_space: Default::default(),
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 1,
            height: 1,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into(),
        };
        let frame = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &ImageInputs::default(),
                )
                .unwrap(),
        );
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            for exposure in [-2.0, 0.0] {
                let actual = paint_with_settings(
                    &renderer,
                    frame.clone(),
                    1,
                    1,
                    format,
                    ViewSettings {
                        exposure,
                        alpha: false,
                        fast_filter: false,
                        filmic: false,
                        channel: Default::default(),
                        backdrop: 0,
                        host: 0,
                    },
                );
                let green = if exposure == -2.0 { 0.1625 } else { 0.5375 };
                let expected = veylvik_render::to_srgba8(&[[0.5375, green, 0.0375, 1.0]]);
                for (a, b) in actual[0].iter().zip(expected) {
                    assert!(
                        a.abs_diff(b) <= 1,
                        "{format:?} exposure={exposure}: {actual:?}"
                    );
                }
            }
            for exposure in [-2.0, 8.0] {
                let actual = paint_with_settings(
                    &renderer,
                    frame.clone(),
                    1,
                    1,
                    format,
                    ViewSettings {
                        exposure,
                        alpha: true,
                        fast_filter: false,
                        filmic: false,
                        channel: Default::default(),
                        backdrop: 0,
                        host: 0,
                    },
                );
                for value in &actual[0][..3] {
                    assert!(value.abs_diff(128) <= 1, "{actual:?}");
                }
                assert_eq!(actual[0][3], 255);
            }
        }
        assert_eq!(
            renderer.read_frame(&frame).unwrap().pixels,
            [[2.0, 0.5, 0.0, 0.5]]
        );
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn viewer_navigation_clips_without_stretching_or_reading_outside_image() {
        let renderer = Renderer::new().unwrap();
        let node = Node::new(
            "pixels",
            Operation::Image(veylvik_document::nodes::Image {
                asset: "pixels".into(),
            }),
            &[],
        );
        let doc = Composition {
            published: Default::default(),
            markers: Default::default(),
            backdrops: Default::default(),
            ocio: Default::default(),
            working_space: Default::default(),
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 2,
            height: 1,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into(),
        };
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "pixels",
            veylvik_render::Image::new(2, 1, vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]])
                .unwrap()
                .into(),
        );
        let frame = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &inputs,
                )
                .unwrap(),
        );
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let actual = paint_view(
                &renderer,
                frame.clone(),
                4,
                1,
                format,
                ViewSettings::default(),
                ViewTransform([0.25, 0.0, 0.5, 1.0]),
            );
            let expected = [
                [24u8, 27, 33, 255],
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [24, 27, 33, 255],
            ];
            for (a, b) in actual.iter().flatten().zip(expected.iter().flatten()) {
                assert!(a.abs_diff(*b) <= 1, "{actual:?}");
            }
            let actual = paint_view(
                &renderer,
                frame.clone(),
                4,
                1,
                format,
                ViewSettings::default(),
                ViewTransform([-0.5, 0.0, 2.0, 1.0]),
            );
            let expected = veylvik_render::to_srgba8(&[
                [0.875, 0.125, 0.0, 1.0],
                [0.625, 0.375, 0.0, 1.0],
                [0.375, 0.625, 0.0, 1.0],
                [0.125, 0.875, 0.0, 1.0],
            ]);
            for (a, b) in actual.iter().flatten().zip(expected) {
                assert!(a.abs_diff(b) <= 1, "{actual:?}");
            }
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn viewer_downscale_integrates_the_pixel_footprint_by_default() {
        let renderer = Renderer::new().unwrap();
        let mut doc = veylvik_document::blank(12, 12, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            Operation::Image(veylvik_document::nodes::Image {
                asset: "stripes".into(),
            });
        let mut inputs = veylvik_render::ImageInputs::default();
        inputs.insert(
            "stripes",
            veylvik_render::Image::new(
                12,
                12,
                (0..144)
                    .map(|i| {
                        if i % 3 == 1 {
                            [1.; 4]
                        } else {
                            [0., 0., 0., 1.]
                        }
                    })
                    .collect(),
            )
            .unwrap()
            .into(),
        );
        let frame = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &inputs,
                )
                .unwrap(),
        );
        let aa = paint(
            &renderer,
            frame.clone(),
            4,
            4,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let fast = paint_with_settings(
            &renderer,
            frame,
            4,
            4,
            wgpu::TextureFormat::Rgba8Unorm,
            ViewSettings {
                fast_filter: true,
                ..Default::default()
            },
        );
        let expected = veylvik_render::to_srgba8(&[[1. / 3., 1. / 3., 1. / 3., 1.]])[0];
        assert!(aa.iter().all(|p| p[0].abs_diff(expected) <= 1), "{aa:?}");
        assert!(
            fast.iter().all(|p| p[0] == 255),
            "legacy bilinear misses the stripe footprint: {fast:?}"
        );
    }
    /// A host's display that shows every picture as one colour, whatever it is given.
    struct Painted([u8; 4]);
    impl veylvik_preview::screen::HostEncode for Painted {
        fn identity(&self) -> u64 {
            42
        }
        fn encode(
            &self,
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            _: &wgpu::Buffer,
            width: u32,
            height: u32,
        ) -> std::result::Result<wgpu::Texture, String> {
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("the host's picture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let texels: Vec<u8> = (0..width * height).flat_map(|_| self.0).collect();
            queue.write_texture(
                texture.as_image_copy(),
                &texels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                size,
            );
            queue.submit([]);
            Ok(texture)
        }
    }
    impl crate::host::HostDisplay for Painted {
        fn name(&self) -> String {
            "a host's view".into()
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn the_hosts_display_draws_the_colour_view_and_the_channels_stay_the_editors() {
        let renderer = Renderer::new().unwrap();
        let node = Node::new(
            "Colour",
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [0.2, 0.5, 0.8, 1.0],
            }),
            &[],
        );
        let mut doc = veylvik_document::blank(2, 2, Time::new(24, 1).unwrap()).unwrap();
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        let frame = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &ImageInputs::default(),
                )
                .unwrap(),
        );
        let host: Arc<dyn crate::host::HostDisplay> = Arc::new(Painted([200, 40, 90, 255]));
        let hosted = ViewSettings {
            host: 42,
            ..Default::default()
        };
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let paint = |settings: ViewSettings,
                         host: Option<Arc<dyn crate::host::HostDisplay>>| {
                paint_hosted(
                    &renderer,
                    frame.clone(),
                    [2, 2],
                    format,
                    settings,
                    ViewTransform::default(),
                    host,
                )
            };
            // The colour view is the host's picture, as the host made it.
            for pixel in paint(hosted, Some(host.clone())) {
                for (a, b) in pixel.iter().zip([200u8, 40, 90, 255]) {
                    assert!(a.abs_diff(b) <= 1, "{format:?}: {pixel:?}");
                }
            }
            // A single channel is the editor's own view, as without a host.
            let red = ViewSettings {
                channel: Channel::Red,
                ..hosted
            };
            assert_eq!(
                paint(red, Some(host.clone())),
                paint(
                    ViewSettings {
                        channel: Channel::Red,
                        ..Default::default()
                    },
                    None
                )
            );
            // Without the display it names, the editor's own colour view.
            assert_eq!(paint(hosted, None), paint(ViewSettings::default(), None));
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_single_channel_or_the_luminance_shows_as_grey() {
        let renderer = Renderer::new().unwrap();
        let rgb = [0.2_f32, 0.5, 0.8];
        let node = Node::new(
            "Colour",
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [rgb[0], rgb[1], rgb[2], 1.0],
            }),
            &[],
        );
        let doc = Composition {
            published: Default::default(),
            markers: Default::default(),
            backdrops: Default::default(),
            ocio: Default::default(),
            working_space: Default::default(),
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 2,
            height: 2,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into(),
        };
        let frame = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &ImageInputs::default(),
                )
                .unwrap(),
        );
        let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        for (channel, value) in [
            (Channel::Red, rgb[0]),
            (Channel::Green, rgb[1]),
            (Channel::Blue, rgb[2]),
            (Channel::Luminance, luminance),
        ] {
            let shown = paint_with_settings(
                &renderer,
                frame.clone(),
                2,
                2,
                wgpu::TextureFormat::Rgba8Unorm,
                ViewSettings {
                    channel,
                    ..Default::default()
                },
            );
            let expected = veylvik_render::to_srgba8(&[[value, value, value, 1.]])[0];
            assert!(
                shown
                    .iter()
                    .all(|p| p[..3].iter().all(|c| c.abs_diff(expected) <= 1)),
                "{channel:?}: {shown:?} against {expected}"
            );
        }
        // A fully transparent picture shows the chosen backdrop: black, mid grey or white.
        let empty = Node::new(
            "Empty",
            Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
            &[],
        );
        let clear = Composition {
            output: empty.id,
            nodes: [(empty.id, empty)].into(),
            ..doc.clone()
        };
        let clear = Arc::new(
            renderer
                .render_gpu(
                    &Plan::compile(&clear).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &ImageInputs::default(),
                )
                .unwrap(),
        );
        for (backdrop, value) in [(1_u8, 0.0_f32), (2, 0.18), (3, 1.0)] {
            let shown = paint_with_settings(
                &renderer,
                clear.clone(),
                2,
                2,
                wgpu::TextureFormat::Rgba8Unorm,
                ViewSettings {
                    backdrop,
                    ..Default::default()
                },
            );
            let expected = veylvik_render::to_srgba8(&[[value, value, value, 1.]])[0];
            assert!(
                shown
                    .iter()
                    .all(|p| p[..3].iter().all(|c| c.abs_diff(expected) <= 1)),
                "{backdrop}: {shown:?}"
            );
        }
        // Colour is untouched by the new setting.
        let colour = paint(&renderer, frame, 2, 2, wgpu::TextureFormat::Rgba8Unorm);
        let expected = veylvik_render::to_srgba8(&[[rgb[0], rgb[1], rgb[2], 1.]]);
        assert!(
            colour
                .iter()
                .all(|p| (0..3).all(|c| p[c].abs_diff(expected[c]) <= 1))
        );
    }
    fn paint(
        renderer: &Renderer,
        frame: Arc<GpuFrame>,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Vec<[u8; 4]> {
        paint_with_settings(
            renderer,
            frame,
            width,
            height,
            format,
            ViewSettings::default(),
        )
    }
    fn paint_with_settings(
        renderer: &Renderer,
        frame: Arc<GpuFrame>,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        settings: ViewSettings,
    ) -> Vec<[u8; 4]> {
        paint_view(
            renderer,
            frame,
            width,
            height,
            format,
            settings,
            ViewTransform::default(),
        )
    }
    fn paint_view(
        renderer: &Renderer,
        frame: Arc<GpuFrame>,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        settings: ViewSettings,
        transform: ViewTransform,
    ) -> Vec<[u8; 4]> {
        paint_hosted(
            renderer,
            frame,
            [width, height],
            format,
            settings,
            transform,
            None,
        )
    }
    fn paint_hosted(
        renderer: &Renderer,
        frame: Arc<GpuFrame>,
        [width, height]: [u32; 2],
        format: wgpu::TextureFormat,
        settings: ViewSettings,
        transform: ViewTransform,
        host: Option<Arc<dyn crate::host::HostDisplay>>,
    ) -> Vec<[u8; 4]> {
        let device = renderer.device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut display = Display::new(device, format);
        display.set_host(host);
        let frame = display.frame(renderer, frame, settings, transform);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(height) * 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = target.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&frame.pipeline);
            pass.set_bind_group(0, &frame.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(height),
                },
            },
            target.size(),
        );
        let submission = renderer.queue().submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range();
        let mut pixels = Vec::new();
        for row in 0..height as usize {
            pixels.extend_from_slice(
                bytes[row * 256..row * 256 + width as usize * 4]
                    .as_chunks::<4>()
                    .0,
            );
        }
        drop(bytes);
        readback.unmap();
        assert!(pollster::block_on(scope.pop()).is_none());
        pixels
    }
}
