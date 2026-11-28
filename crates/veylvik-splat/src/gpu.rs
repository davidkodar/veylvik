//! Splats on the GPU: uploaded once, then for each view projected, sorted back to front and drawn into a render pass whose depth the scene's solids filled.
use crate::{STRIDE, Splats};
use wgpu::util::DeviceExt;

/// The programs: preprocess, one step of the sort, and the draw for one target format.
pub struct Renderer {
    preprocess: wgpu::ComputePipeline,
    sort: wgpu::ComputePipeline,
    draw: wgpu::RenderPipeline,
}
/// A set of splats on the GPU, with room to sort them.
pub struct Uploaded {
    count: u32,
    padded: u32,
    degree: u32,
    coefficients: u32,
    splats: wgpu::Buffer,
    rest: wgpu::Buffer,
    keys: wgpu::Buffer,
    values: wgpu::Buffer,
    projected: wgpu::Buffer,
    /// One uniform a sort step: its `step` and `block`, 256 bytes apart.
    steps: wgpu::Buffer,
    passes: Vec<(u32, u32)>,
}
impl Uploaded {
    /// The splats (STRIDE floats each) and their spherical harmonics past the first, as
    /// uploaded, for another renderer to read (the path tracer); the rest buffer holds at
    /// least one value.
    pub fn buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer) {
        (&self.splats, &self.rest)
    }
    /// Bytes it holds on the GPU.
    pub fn bytes(&self) -> u64 {
        [
            &self.splats,
            &self.rest,
            &self.keys,
            &self.values,
            &self.projected,
            &self.steps,
        ]
        .iter()
        .map(|b| b.size())
        .sum()
    }
}
/// One view of a set of splats: matrices row by row (as `[row][column]`), the frame's
/// size, the camera's focal length in pixels and place, and a brightness.
pub struct Draw {
    pub view: [[f32; 4]; 4],
    pub projection: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub width: u32,
    pub height: u32,
    pub focal: f32,
    pub eye: [f32; 3],
    pub brightness: f32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Frame {
    view: [[f32; 4]; 4],
    projection: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    viewport: [f32; 4],
    eye: [f32; 4],
    params: [f32; 4],
}
fn columns(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| m[r][c]))
}
/// A view prepared for drawing: its bind group.
pub struct Prepared {
    group: wgpu::BindGroup,
    count: u32,
}
impl Renderer {
    /// Programs that draw into targets of `colour` and `depth` with `samples` samples a
    /// pixel; the depth is reversed (nearer is larger) and left as the solids wrote it.
    pub fn new(
        device: &wgpu::Device,
        colour: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("splats"),
            source: wgpu::ShaderSource::Wgsl(include_str!("splat.wgsl").into()),
        });
        let compute = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let premultiplied = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("splats"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: colour,
                    blend: Some(wgpu::BlendState {
                        color: premultiplied,
                        alpha: premultiplied,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            preprocess: compute("preprocess"),
            sort: compute("sort"),
            draw,
        }
    }
    /// The splats on the GPU, with room to sort them.
    pub fn upload(&self, device: &wgpu::Device, splats: &Splats) -> Uploaded {
        let count = splats.count.max(1) as u32;
        let padded = count.next_power_of_two().max(2);
        // Readable by other renderers too: the path tracer copies sets together.
        let storage = |label, bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            })
        };
        let mut data = splats.data.clone();
        data.resize(count as usize * STRIDE, 0.0);
        let mut rest = splats.rest.clone();
        rest.push(0.0);
        let room = |label, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        // The sorting network: for each block size, each step down to 1.
        let mut passes = Vec::new();
        let mut block = 2;
        while block <= padded {
            let mut step = block / 2;
            while step > 0 {
                passes.push((step, block));
                step /= 2;
            }
            block *= 2;
        }
        let mut steps = vec![0u8; passes.len() * 256];
        for (k, (step, block)) in passes.iter().enumerate() {
            steps[k * 256..k * 256 + 4].copy_from_slice(&step.to_le_bytes());
            steps[k * 256 + 4..k * 256 + 8].copy_from_slice(&block.to_le_bytes());
        }
        Uploaded {
            count: splats.count as u32,
            padded,
            degree: splats.degree,
            coefficients: splats.coefficients() as u32,
            splats: storage("splats", bytemuck::cast_slice(&data)),
            rest: storage("splat colours", bytemuck::cast_slice(&rest)),
            keys: room("splat keys", u64::from(padded) * 4),
            values: room("splat order", u64::from(padded) * 4),
            projected: room("splats on screen", u64::from(padded) * 48),
            steps: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("splat sort steps"),
                contents: &steps,
                usage: wgpu::BufferUsages::UNIFORM,
            }),
            passes,
        }
    }
    /// Projects and sorts `splats` for `view`, into `encoder` (outside any render
    /// pass); then `draw` draws them.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        splats: &Uploaded,
        view: &Draw,
    ) -> Prepared {
        let frame = Frame {
            view: columns(&view.view),
            projection: columns(&view.projection),
            model: columns(&view.model),
            viewport: [
                view.width as f32,
                view.height as f32,
                view.focal,
                splats.count as f32,
            ],
            eye: [view.eye[0], view.eye[1], view.eye[2], splats.degree as f32],
            params: [
                view.brightness,
                splats.padded as f32,
                splats.coefficients as f32,
                0.0,
            ],
        };
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("splat view"),
            contents: bytemuck::bytes_of(&frame),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = |pipeline: &wgpu::BindGroupLayout,
                     entries: &[(u32, wgpu::BindingResource<'_>)]| {
            let entries: Vec<wgpu::BindGroupEntry<'_>> = entries
                .iter()
                .map(|(binding, resource)| wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: resource.clone(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("splats"),
                layout: pipeline,
                entries: &entries,
            })
        };
        let pre = group(
            &self.preprocess.get_bind_group_layout(0),
            &[
                (0, whole(&uniform)),
                (1, whole(&splats.splats)),
                (2, whole(&splats.rest)),
                (3, whole(&splats.keys)),
                (4, whole(&splats.values)),
                (5, whole(&splats.projected)),
            ],
        );
        let layout = self.sort.get_bind_group_layout(0);
        let steps: Vec<wgpu::BindGroup> = (0..splats.passes.len())
            .map(|k| {
                group(
                    &layout,
                    &[
                        (0, whole(&uniform)),
                        (3, whole(&splats.keys)),
                        (4, whole(&splats.values)),
                        (
                            6,
                            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &splats.steps,
                                offset: k as u64 * 256,
                                size: wgpu::BufferSize::new(8),
                            }),
                        ),
                    ],
                )
            })
            .collect();
        let groups = splats.padded.div_ceil(256);
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.preprocess);
            pass.set_bind_group(0, &pre, &[]);
            pass.dispatch_workgroups(groups, 1, 1);
            pass.set_pipeline(&self.sort);
            for step in &steps {
                pass.set_bind_group(0, step, &[]);
                pass.dispatch_workgroups(groups, 1, 1);
            }
        }
        Prepared {
            group: group(
                &self.draw.get_bind_group_layout(0),
                &[
                    (0, whole(&uniform)),
                    (7, whole(&splats.values)),
                    (8, whole(&splats.projected)),
                ],
            ),
            count: splats.count,
        }
    }
    /// Draws prepared splats into `pass` (whose depth holds the solids), back to front.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, prepared: &Prepared) {
        if prepared.count == 0 {
            return;
        }
        pass.set_pipeline(&self.draw);
        pass.set_bind_group(0, &prepared.group, &[]);
        pass.draw(0..6, 0..prepared.count);
    }
}
fn whole(buffer: &wgpu::Buffer) -> wgpu::BindingResource<'_> {
    buffer.as_entire_binding()
}
