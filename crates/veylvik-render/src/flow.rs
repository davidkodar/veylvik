//! Bidirectional pyramidal Lucas–Kanade interpolation with conservative fallback.
//! Flow fields, pyramids and warped pixels stay on Vulkan. Only three counters are read.
use crate::{GpuFrame, Renderer, Result, error};
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct FlowReport {
    pub analysis_width: u32,
    pub analysis_height: u32,
    /// Estimated fraction on a 4×4 sampling grid, not a guarantee of correctness.
    pub fallback_fraction: f32,
    pub scene_cut: bool,
    pub cache_hit: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct FlowSettings {
    pub fraction: f32,
    /// Long-edge limit for motion analysis. Output always retains native dimensions.
    pub analysis_edge: u32,
    pub confidence: f32,
    pub diagnostic: bool,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    size: [u32; 4],
    other: [u32; 4],
    values: [f32; 4],
}
struct Cached {
    key: (u64, u64, u32),
    forward: GpuFrame,
    backward: GpuFrame,
}
pub(crate) struct Engine {
    layout: wgpu::BindGroupLayout,
    pyramid: wgpu::ComputePipeline,
    refine: wgpu::ComputePipeline,
    interpolate: wgpu::ComputePipeline,
    carry: wgpu::ComputePipeline,
    cache: Mutex<Option<Cached>>,
}
impl Engine {
    pub(crate) fn clear_cache(&self) -> Result<()> {
        *self
            .cache
            .lock()
            .map_err(|_| error("flow cache poisoned"))? = None;
        Ok(())
    }
    fn new(renderer: &Renderer) -> Result<Self> {
        renderer.compute(|scratch| {
            let scope = scratch
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let entries: Vec<_> = (0..7)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    count: None,
                    ty: wgpu::BindingType::Buffer {
                        ty: if binding == 0 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage {
                                read_only: binding != 4 && binding != 6,
                            }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(if binding == 0 {
                            48
                        } else {
                            16
                        }),
                    },
                })
                .collect();
            let layout =
                scratch
                    .device()
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("Optical flow"),
                        entries: &entries,
                    });
            let pipeline_layout =
                scratch
                    .device()
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("Optical flow"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
            let shader = scratch
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Optical flow"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("flow.wgsl").into()),
                });
            let pipeline = |entry| {
                scratch
                    .device()
                    .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&pipeline_layout),
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    })
            };
            let pyramid = pipeline("pyramid");
            let refine = pipeline("refine");
            let interpolate = pipeline("interpolate");
            let carry = pipeline("carry");
            if let Some(e) = pollster::block_on(scope.pop()) {
                return Err(error(e));
            }
            Ok(Self {
                layout,
                pyramid,
                refine,
                interpolate,
                carry,
                cache: Mutex::new(None),
            })
        })
    }
    fn run(
        &self,
        renderer: &Renderer,
        pipeline: &wgpu::ComputePipeline,
        p: Params,
        images: [Option<&GpuFrame>; 4],
        counters: bool,
    ) -> Result<(GpuFrame, [u32; 4])> {
        renderer.compute(|scratch| {
            let output = scratch.buffer(
                u64::from(p.size[0]) * u64::from(p.size[1]) * 16,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "Flow output",
            )?;
            let dummy = scratch.buffer(
                16,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                "Flow empty input",
            )?;
            scratch.queue().write_buffer(&dummy, 0, &[0; 16]);
            let stats = scratch.buffer(
                16,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                "Flow confidence counters",
            )?;
            scratch.queue().write_buffer(&stats, 0, &[0; 16]);
            let uniform = scratch.buffer(
                48,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "Flow parameters",
            )?;
            scratch
                .queue()
                .write_buffer(&uniform, 0, bytemuck::bytes_of(&p));
            let source = |i: usize| images[i].map_or(&dummy, |frame| frame.buffer());
            let buffers = [
                &uniform,
                source(0),
                source(1),
                source(2),
                &output,
                source(3),
                &stats,
            ];
            let entries: Vec<_> = buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            let bind = scratch
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Optical flow"),
                    layout: &self.layout,
                    entries: &entries,
                });
            let mut encoder =
                scratch
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Optical flow"),
                    });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(p.size[0].div_ceil(8), p.size[1].div_ceil(8), 1);
            }
            let mut status = [0; 4];
            if counters {
                let readback = scratch.buffer(
                    16,
                    wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    "Flow status",
                )?;
                encoder.copy_buffer_to_buffer(&stats, 0, &readback, 0, 16);
                status.copy_from_slice(&scratch.read::<u32>(encoder, &readback)?);
            } else {
                scratch.run(encoder)?;
            }
            Ok((scratch.frame(&output, p.size[0], p.size[1])?, status))
        })
    }
    fn pyramid(
        &self,
        renderer: &Renderer,
        input: &GpuFrame,
        edge: u32,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Vec<GpuFrame>>> {
        let mut current = input.clone();
        let mut levels = Vec::new();
        loop {
            if cancelled() {
                return Ok(None);
            }
            if current.width.max(current.height) <= edge {
                levels.push(current.clone());
            }
            if current.width.min(current.height) <= 24 && current.width.max(current.height) <= edge
            {
                break;
            }
            let (width, height) = (current.width.div_ceil(2), current.height.div_ceil(2));
            let p = Params {
                size: [width, height, current.width, current.height],
                other: [1; 4],
                values: [0.; 4],
            };
            current = self
                .run(
                    renderer,
                    &self.pyramid,
                    p,
                    [Some(&current), None, None, None],
                    false,
                )?
                .0;
        }
        // Narrow/very small images still have a valid analysis level.
        if levels.is_empty() {
            levels.push(current);
        }
        Ok(Some(levels))
    }
    fn estimate(
        &self,
        renderer: &Renderer,
        a: &[GpuFrame],
        b: &[GpuFrame],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Option<GpuFrame>> {
        let mut flow: Option<GpuFrame> = None;
        for (a, b) in a.iter().zip(b).rev() {
            for _ in 0..5 {
                if cancelled() {
                    return Ok(None);
                }
                let (w, h) = (a.width, a.height);
                let p = Params {
                    size: [w, h, w, h],
                    other: [
                        w,
                        h,
                        flow.as_ref().map_or(1, |f| f.width),
                        flow.as_ref().map_or(1, |f| f.height),
                    ],
                    values: [0.; 4],
                };
                flow = Some(
                    self.run(
                        renderer,
                        &self.refine,
                        p,
                        [Some(a), Some(b), flow.as_ref(), None],
                        false,
                    )?
                    .0,
                );
            }
        }
        Ok(flow)
    }
}
impl Renderer {
    /// A picture that belongs to frame `b` (`payload`, row-major, the size of the frames)
    /// as frame `a` sees it: each pixel of `a` fetches the payload's alpha along the
    /// motion from `a` to `b`. Returns, for every pixel, that alpha and how far the
    /// motion can be trusted there (0 where the two directions disagree or it leaves
    /// the frame). For steadying something made frame by frame, such as a matte.
    pub fn carry_along_motion(
        &self,
        a: &GpuFrame,
        b: &GpuFrame,
        payload: &[f32],
        analysis_edge: u32,
    ) -> Result<Vec<[f32; 2]>> {
        if !self.owns(a) || !self.owns(b) || a.width != b.width || a.height != b.height {
            return Err(error("Motion needs equal-sized frames on this device"));
        }
        if payload.len() != a.width as usize * a.height as usize
            || !(64..=1536).contains(&analysis_edge)
        {
            return Err(error("Invalid picture or analysis size for motion"));
        }
        let engine = {
            let mut engine = self
                .flow_engine
                .lock()
                .map_err(|_| error("Flow engine unavailable"))?;
            if engine.is_none() {
                *engine = Some(Arc::new(Engine::new(self)?));
            }
            engine.as_ref().unwrap().clone()
        };
        let mut never = || false;
        let missing = || error("Motion analysis stopped");
        let ap = engine
            .pyramid(self, a, analysis_edge, &mut never)?
            .ok_or_else(missing)?;
        let bp = engine
            .pyramid(self, b, analysis_edge, &mut never)?
            .ok_or_else(missing)?;
        let forward = engine
            .estimate(self, &ap, &bp, &mut never)?
            .ok_or_else(missing)?;
        let backward = engine
            .estimate(self, &bp, &ap, &mut never)?
            .ok_or_else(missing)?;
        let picture = self.compute(|scratch| {
            let bytes: Vec<f32> = payload.iter().flat_map(|v| [*v, *v, *v, *v]).collect();
            let buffer = scratch.buffer(
                bytes.len() as u64 * 4,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                "Carried picture",
            )?;
            scratch
                .queue()
                .write_buffer(&buffer, 0, bytemuck::cast_slice(&bytes));
            scratch.frame(&buffer, a.width, a.height)
        })?;
        let p = Params {
            size: [a.width, a.height, a.width, a.height],
            other: [b.width, b.height, forward.width, forward.height],
            values: [0.; 4],
        };
        let (frame, _) = engine.run(
            self,
            &engine.carry,
            p,
            [None, Some(&picture), Some(&forward), Some(&backward)],
            false,
        )?;
        Ok(self
            .read_frame(&frame)?
            .pixels
            .iter()
            .map(|pixel| [pixel[0], pixel[1]])
            .collect())
    }
    /// Interpolate two immutable frames. Uncertain regions use the nearest source;
    /// the diagnostic view visualises reliability. This is not a vector motion blur.
    pub fn interpolate_flow(
        &self,
        a: &GpuFrame,
        b: &GpuFrame,
        settings: FlowSettings,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<GpuFrame>> {
        if !self.owns(a) || !self.owns(b) || a.width != b.width || a.height != b.height {
            return Err(error(
                "Optical flow needs equal-sized frames on this device",
            ));
        }
        if !settings.fraction.is_finite()
            || !(0.0..=1.0).contains(&settings.fraction)
            || !(64..=1536).contains(&settings.analysis_edge)
            || !settings.confidence.is_finite()
            || !(0.01..=1.0).contains(&settings.confidence)
        {
            return Err(error("Invalid optical-flow settings"));
        }
        if cancelled() {
            return Ok(None);
        }
        if settings.fraction == 0. && !settings.diagnostic {
            return Ok(Some(a.clone()));
        }
        if settings.fraction == 1. && !settings.diagnostic {
            return Ok(Some(b.clone()));
        }
        let engine = {
            let mut engine = self
                .flow_engine
                .lock()
                .map_err(|_| error("Flow engine unavailable"))?;
            if engine.is_none() {
                *engine = Some(Arc::new(Engine::new(self)?));
            }
            engine.as_ref().unwrap().clone()
        };
        let key = (a.identity, b.identity, settings.analysis_edge);
        let cached = engine
            .cache
            .lock()
            .map_err(|_| error("Flow cache unavailable"))?
            .as_ref()
            .filter(|c| c.key == key)
            .map(|c| (c.forward.clone(), c.backward.clone()));
        let cache_hit = cached.is_some();
        let (forward, backward) = if let Some(pair) = cached {
            pair
        } else {
            let Some(ap) = engine.pyramid(self, a, settings.analysis_edge, &mut cancelled)? else {
                return Ok(None);
            };
            let Some(bp) = engine.pyramid(self, b, settings.analysis_edge, &mut cancelled)? else {
                return Ok(None);
            };
            let Some(forward) = engine.estimate(self, &ap, &bp, &mut cancelled)? else {
                return Ok(None);
            };
            let Some(backward) = engine.estimate(self, &bp, &ap, &mut cancelled)? else {
                return Ok(None);
            };
            if cancelled() {
                return Ok(None);
            }
            *engine
                .cache
                .lock()
                .map_err(|_| error("Flow cache unavailable"))? = Some(Cached {
                key,
                forward: forward.clone(),
                backward: backward.clone(),
            });
            (forward, backward)
        };
        if cancelled() {
            return Ok(None);
        }
        let mut p = Params {
            size: [a.width, a.height, a.width, a.height],
            other: [b.width, b.height, forward.width, forward.height],
            values: [
                settings.fraction,
                settings.confidence,
                f32::from(u8::from(settings.diagnostic)),
                0.,
            ],
        };
        let (mut frame, stats) = engine.run(
            self,
            &engine.interpolate,
            p,
            [Some(a), Some(b), Some(&forward), Some(&backward)],
            true,
        )?;
        let measured = (stats[0] + stats[1]).max(1) as f32;
        let fallback = stats[1] as f32 / measured;
        let scene_cut = fallback > 0.95 && stats[2] as f32 / (measured * 16.) > 0.2;
        if scene_cut && !settings.diagnostic {
            if cancelled() {
                return Ok(None);
            }
            p.values[3] = 1.;
            frame = engine
                .run(
                    self,
                    &engine.interpolate,
                    p,
                    [Some(a), Some(b), Some(&forward), Some(&backward)],
                    false,
                )?
                .0;
        }
        frame.flow_report = Some(FlowReport {
            analysis_width: forward.width,
            analysis_height: forward.height,
            fallback_fraction: if scene_cut { 1. } else { fallback },
            scene_cut,
            cache_hit,
        });
        Ok((!cancelled()).then_some(frame))
    }
}
