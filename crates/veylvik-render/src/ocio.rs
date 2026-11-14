//! OCIO's generated transforms run in full precision on the compositor device.
use crate::{GpuFrame, Renderer, Result, compute::DeviceKey, error};
pub struct OcioProcessor {
    device: DeviceKey,
    pipeline: wgpu::ComputePipeline,
    luts: Vec<GpuFrame>,
}
impl OcioProcessor {
    pub fn new(renderer: &Renderer, shader: &veylvik_color::Shader) -> Result<Self> {
        renderer.compute(|scratch| {
            if shader.textures.len() as u32 + 2
                > scratch
                    .device()
                    .limits()
                    .max_storage_buffers_per_shader_stage
            {
                return Err(error(
                    "OCIO processor exceeds this device's storage binding limit",
                ));
            }
            let scope = scratch
                .device()
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let module = scratch
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("OpenColorIO"),
                    source: wgpu::ShaderSource::Wgsl(shader.wgsl.clone().into()),
                });
            let pipeline =
                scratch
                    .device()
                    .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some("OpenColorIO"),
                        layout: None,
                        module: &module,
                        entry_point: Some("main"),
                        compilation_options: Default::default(),
                        cache: None,
                    });
            if let Some(e) = pollster::block_on(scope.pop()) {
                return Err(error(format!("OCIO GPU shader: {e}")));
            }
            let mut luts = Vec::new();
            for texture in &shader.textures {
                let buffer = scratch.buffer(
                    texture.pixels.len() as u64 * 16,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    "OCIO LUT",
                )?;
                scratch
                    .queue()
                    .write_buffer(&buffer, 0, bytemuck::cast_slice(&texture.pixels));
                luts.push(scratch.frame(&buffer, texture.pixels.len() as u32, 1)?);
            }
            Ok(Self {
                device: renderer.device_key(),
                pipeline,
                luts,
            })
        })
    }
    /// Unpremultiplies before colour processing, then restores the original alpha.
    /// No clamping, display encoding or CPU pixel fallback is implicitly added.
    pub fn apply(&self, renderer: &Renderer, input: &GpuFrame) -> Result<GpuFrame> {
        if self.device != renderer.device_key() || !renderer.owns(input) {
            return Err(error("OCIO resources belong to a different GPU"));
        }
        renderer.compute(|scratch| {
            let count = input.width * input.height;
            let output = scratch.buffer(
                u64::from(count) * 16,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "OCIO output",
            )?;
            let size = scratch.buffer(
                16,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "OCIO dimensions",
            )?;
            scratch.queue().write_buffer(
                &size,
                0,
                bytemuck::cast_slice(&[count, input.width, input.height, 0]),
            );
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: size.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: input.buffer().as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ];
            entries.extend(
                self.luts
                    .iter()
                    .enumerate()
                    .map(|(i, lut)| wgpu::BindGroupEntry {
                        binding: 3 + i as u32,
                        resource: lut.buffer().as_entire_binding(),
                    }),
            );
            let binding = scratch
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("OCIO transform"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
            let mut encoder =
                scratch
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("OCIO transform"),
                    });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("OCIO transform"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &binding, &[]);
                pass.dispatch_workgroups(input.width.div_ceil(8), input.height.div_ceil(8), 1);
            }
            let flag = scratch.buffer(
                4,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                "OCIO finite flag",
            )?;
            scratch.queue().write_buffer(&flag, 0, &[0; 4]);
            let validation = scratch
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("OCIO finite validation"),
                    layout: &renderer.validation_pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: output.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: flag.as_entire_binding(),
                        },
                    ],
                });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&renderer.validation_pipeline);
                pass.set_bind_group(0, &validation, &[]);
                pass.dispatch_workgroups(input.width.div_ceil(8), input.height.div_ceil(8), 1);
            }
            let readback = scratch.buffer(
                4,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                "OCIO finite status",
            )?;
            encoder.copy_buffer_to_buffer(&flag, 0, &readback, 0, 4);
            if scratch.read::<u32>(encoder, &readback)?[0] != 0 {
                return Err(error("OCIO transform produced non-finite pixels"));
            }
            let mut frame = scratch.frame(&output, input.width, input.height)?;
            frame.region = input.region;
            Ok(frame)
        })
    }
}

type Stamp = (std::path::PathBuf, u64, std::time::SystemTime);
pub(crate) struct CachedProcessor {
    key: String,
    files: Vec<Stamp>,
    processor: std::sync::Arc<OcioProcessor>,
}
fn stamp(path: std::path::PathBuf) -> Result<Stamp> {
    let metadata = std::fs::metadata(&path)
        .map_err(|e| error(format!("OCIO dependency {}: {e}", path.display())))?;
    let modified = metadata.modified().map_err(error)?;
    Ok((path, metadata.len(), modified))
}
impl Renderer {
    pub(crate) fn ocio_transform(
        &self,
        settings: &veylvik_document::color_management::OcioTransform,
        inputs: &crate::ImageInputs,
        frame: &GpuFrame,
    ) -> Result<GpuFrame> {
        let mut settings = settings.clone();
        settings.config = inputs.resolve_config(&settings.config);
        let key = serde_json::to_string(&settings).map_err(error)?;
        let cached = {
            let cache = self
                .ocio_cache
                .lock()
                .map_err(|_| error("OCIO cache unavailable"))?;
            cache
                .iter()
                .find(|entry| {
                    entry.key == key
                        && entry
                            .files
                            .iter()
                            .all(|file| stamp(file.0.clone()).is_ok_and(|now| now == *file))
                })
                .map(|entry| entry.processor.clone())
        };
        let processor = if let Some(processor) = cached {
            processor
        } else {
            let config = veylvik_color::Config::load(&settings.config).map_err(error)?;
            let program = config
                .processor(
                    &settings.source,
                    &settings.destination,
                    &settings.display,
                    &settings.view,
                )
                .map_err(error)?;
            let mut files = Vec::new();
            if !settings.config.starts_with("ocio://") {
                files.push(stamp(std::path::PathBuf::from(&settings.config))?);
            }
            let base = std::path::Path::new(&settings.config)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            for path in program.files() {
                files.push(stamp(base.join(path))?);
            }
            let processor =
                std::sync::Arc::new(OcioProcessor::new(self, &program.shader().map_err(error)?)?);
            let mut cache = self
                .ocio_cache
                .lock()
                .map_err(|_| error("OCIO cache unavailable"))?;
            cache.retain(|entry| entry.key != key);
            while cache.len() >= 4 {
                cache.pop_front();
            }
            cache.push_back(CachedProcessor {
                key,
                files,
                processor: processor.clone(),
            });
            processor
        };
        processor.apply(self, frame)
    }
}

impl Renderer {
    pub fn apply_ocio_view(
        &self,
        plan: &crate::Plan,
        inputs: &crate::ImageInputs,
        frame: std::sync::Arc<GpuFrame>,
    ) -> Result<std::sync::Arc<GpuFrame>> {
        match &plan.ocio.view {
            Some(settings) => self
                .ocio_transform(settings, inputs, &frame)
                .map(std::sync::Arc::new),
            None => Ok(frame),
        }
    }
    pub fn render_gpu_export(
        &self,
        plan: &crate::Plan,
        request: crate::RenderRequest,
        inputs: &crate::ImageInputs,
    ) -> Result<std::sync::Arc<GpuFrame>> {
        let frame = self.render_gpu_cached(plan, request, inputs)?;
        match &plan.ocio.output {
            Some(settings) => self
                .ocio_transform(settings, inputs, &frame)
                .map(std::sync::Arc::new),
            None => Ok(frame),
        }
    }
    pub fn render_frame_export(
        &self,
        plan: &crate::Plan,
        request: crate::RenderRequest,
        inputs: &crate::ImageInputs,
    ) -> Result<crate::RenderedFrame> {
        self.read_frame(self.render_gpu_export(plan, request, inputs)?.as_ref())
    }
}
