//! The denoiser (denoise.wgsl): a path traced picture is gathered with its guides into a buffer of its own, then smoothed in five passes of widening reach straight into the picture. The buffers are kept for the next frame of the same size; a picture being gathered across a shutter's moments holds its buffers until its last moment.
use std::sync::Mutex;
use wgpu::util::DeviceExt;

/// Brightness may differ by this many standard deviations of the noise.
const BRIGHTNESS: f32 = 4.0;
/// How sharply neighbours' facing must agree (a power of the cosine between them).
const FACING: f32 = 128.0;
/// How far distance may differ, against how fast it changes there.
const DISTANCE: f32 = 1.0;
/// The passes' reach in pixels.
const REACHES: [u32; 5] = [1, 2, 4, 8, 16];
const UNIFORM_STEP: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Stage {
    size: [u32; 4],
    tune: [f32; 4],
}
/// One picture's buffers: what is gathered (two vec4s a pixel) and two to smooth between.
pub(crate) struct Work {
    pub(crate) gathered: wgpu::Buffer,
    ping: wgpu::Buffer,
    pong: wgpu::Buffer,
    pixels: u64,
}
impl Work {
    fn new(device: &wgpu::Device, pixels: u64) -> Self {
        let make = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        Self {
            gathered: make("path tracer gathered", pixels * 32),
            ping: make("denoiser ping", pixels * 16),
            pong: make("denoiser pong", pixels * 16),
            pixels,
        }
    }
    fn bytes(&self) -> u64 {
        self.pixels * 64
    }
}
pub(crate) struct Denoiser {
    begin: wgpu::ComputePipeline,
    widen: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Buffers kept for the next picture of the same size, and those whose pictures are
    /// still being gathered, by the picture they are for.
    idle: Mutex<Vec<Work>>,
    busy: Mutex<Vec<(wgpu::Buffer, Work)>>,
}
fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
impl Denoiser {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("denoiser"),
            source: wgpu::ShaderSource::Wgsl(include_str!("denoise.wgsl").into()),
        });
        let layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("denoiser"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: wgpu::BufferSize::new(
                                std::mem::size_of::<Stage>() as u64
                            ),
                        },
                        count: None,
                    },
                    storage(1, true),
                    storage(2, true),
                    storage(3, false),
                    storage(4, false),
                ],
            });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("denoiser"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("denoiser"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            begin: pipeline("begin"),
            widen: pipeline("widen"),
            layout,
            idle: Mutex::new(Vec::new()),
            busy: Mutex::new(Vec::new()),
        }
    }
    /// The buffers to gather `target`'s picture into: fresh ones when it starts (`first`),
    /// else the ones it has been gathered into so far.
    pub(crate) fn take(
        &self,
        device: &wgpu::Device,
        target: &wgpu::Buffer,
        pixels: u64,
        first: bool,
    ) -> Result<Work, String> {
        let mut busy = self.busy.lock().map_err(|_| "denoiser lock poisoned")?;
        let held = busy
            .iter()
            .position(|(t, _)| t == target)
            .map(|i| busy.swap_remove(i).1);
        if !first {
            return held
                .filter(|w| w.pixels == pixels)
                .ok_or_else(|| "the path traced picture lost what it had gathered".into());
        }
        drop(busy);
        let mut idle = self.idle.lock().map_err(|_| "denoiser lock poisoned")?;
        if let Some(i) = idle.iter().position(|w| w.pixels == pixels) {
            return Ok(idle.swap_remove(i));
        }
        Ok(Work::new(device, pixels))
    }
    /// Keeps `work` for `target`'s next moment.
    pub(crate) fn hold(&self, target: &wgpu::Buffer, work: Work) {
        if let Ok(mut busy) = self.busy.lock() {
            busy.push((target.clone(), work));
        }
    }
    /// Smooths what `work` gathered over `samples` paths a pixel into `target`, and keeps
    /// the buffers for the next picture (only one size is kept).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        work: Work,
        target: &wgpu::Buffer,
        width: u32,
        height: u32,
        samples: u32,
    ) {
        let tune = [samples as f32, BRIGHTNESS, FACING, DISTANCE];
        let stages: Vec<Stage> = std::iter::once(Stage {
            size: [width, height, 0, 0],
            tune,
        })
        .chain(REACHES.iter().enumerate().map(|(k, &reach)| Stage {
            size: [
                width,
                height,
                reach,
                if k + 1 == REACHES.len() { 2 } else { 1 },
            ],
            tune,
        }))
        .collect();
        let mut uniforms = vec![0u8; stages.len() * UNIFORM_STEP as usize];
        for (k, s) in stages.iter().enumerate() {
            let at = k * UNIFORM_STEP as usize;
            uniforms[at..at + std::mem::size_of::<Stage>()].copy_from_slice(bytemuck::bytes_of(s));
        }
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("denoiser passes"),
            contents: &uniforms,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pixels = u64::from(width) * u64::from(height);
        // Two ways round: smoothing from ping into pong, and back.
        let group = |source: &wgpu::Buffer, into: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("denoiser"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &uniform,
                            offset: 0,
                            size: wgpu::BufferSize::new(std::mem::size_of::<Stage>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: work.gathered.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: source.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: into.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: target,
                            offset: 0,
                            size: wgpu::BufferSize::new(pixels * 16),
                        }),
                    },
                ],
            })
        };
        let to_ping = group(&work.pong, &work.ping);
        let to_pong = group(&work.ping, &work.pong);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("denoising"),
            timestamp_writes: None,
        });
        let groups = [width.div_ceil(8), height.div_ceil(8)];
        for k in 0..stages.len() {
            pass.set_pipeline(if k == 0 { &self.begin } else { &self.widen });
            // The first pass fills ping; then each reads what the last one wrote.
            let bind = if k % 2 == 0 { &to_ping } else { &to_pong };
            pass.set_bind_group(0, bind, &[(k as u64 * UNIFORM_STEP) as u32]);
            pass.dispatch_workgroups(groups[0], groups[1], 1);
        }
        drop(pass);
        if let Ok(mut idle) = self.idle.lock() {
            idle.retain(|w| w.pixels == work.pixels);
            idle.truncate(1);
            idle.push(work);
        }
    }
    pub(crate) fn held_bytes(&self) -> u64 {
        let idle = self
            .idle
            .lock()
            .map_or(0, |w| w.iter().map(Work::bytes).sum());
        let busy = self
            .busy
            .lock()
            .map_or(0, |w| w.iter().map(|(_, w)| w.bytes()).sum());
        idle + busy
    }
    pub(crate) fn release(&self) {
        if let Ok(mut idle) = self.idle.lock() {
            idle.clear();
        }
    }
}
