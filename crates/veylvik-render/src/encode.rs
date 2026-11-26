//! SDR Rec.709 video boundary: GPU resize, matte, transfer and YUV packing.
use crate::{GpuFrame, Renderer, Result, error};
use veylvik_interop::{DmaFrame, DmaLayout};
enum Destination<'a, 'b> {
    Host(Option<DmaLayout>),
    Cuda(&'a mut veylvik_interop::CudaDestination<'b>),
    Dma(&'a std::path::Path, DmaLayout),
}
struct Prepared {
    bytes: Vec<u8>,
    dma: Option<DmaFrame>,
}

pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("video export colour"),
        source: wgpu::ShaderSource::Wgsl(include_str!("encode.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("video export colour"),
        layout: None,
        module: &module,
        entry_point: Some("encode"),
        compilation_options: Default::default(),
        cache: None,
    })
}
/// How exported video looks: the matte composited under transparent areas, and the SDR
/// display transform. A bare matte colour means the Standard transform.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub matte: [f32; 3],
    pub display: veylvik_document::media::DisplayTransform,
}
impl From<[f32; 3]> for Look {
    fn from(matte: [f32; 3]) -> Self {
        Self {
            matte,
            display: Default::default(),
        }
    }
}
impl Renderer {
    /// Return compact NV12/P010 for the staged hardware-encoder bridge. All pixel
    /// arithmetic runs on Vulkan; linear RGBA is never read back by this path.
    pub fn encode_video_planes(
        &self,
        frame: &GpuFrame,
        width: u32,
        height: u32,
        ten_bit: bool,
        look: impl Into<Look>,
    ) -> Result<Vec<u8>> {
        let look = look.into();
        self.retrying(|| {
            self.prepare_video_planes(frame, width, height, ten_bit, look, Destination::Host(None))
                .map(|p| p.bytes)
        })
    }
    /// GPU-only handoff to a uniquely writable CUDA encoder frame.
    pub fn encode_video_cuda(
        &self,
        frame: &GpuFrame,
        look: impl Into<Look>,
        destination: &mut veylvik_interop::CudaDestination<'_>,
    ) -> Result<()> {
        let [width, height] = destination.dimensions();
        let (look, ten_bit) = (look.into(), destination.sample_bytes() == 2);
        self.retrying(|| {
            self.prepare_video_planes(
                frame,
                width,
                height,
                ten_bit,
                look,
                Destination::Cuda(&mut *destination),
            )
            .map(|_| ())
        })
    }
    /// Linear, padded NV12/P010 for explicit diagnostics; production DMA export
    /// does not read back these pixels.
    pub fn encode_video_dma_planes(
        &self,
        frame: &GpuFrame,
        layout: DmaLayout,
        look: impl Into<Look>,
    ) -> Result<Vec<u8>> {
        layout.validate().map_err(error)?;
        self.prepare_video_planes(
            frame,
            layout.width,
            layout.height,
            layout.ten_bit,
            look.into(),
            Destination::Host(Some(layout)),
        )
        .map(|p| p.bytes)
    }
    pub fn encode_video_dma(
        &self,
        frame: &GpuFrame,
        width: u32,
        height: u32,
        ten_bit: bool,
        look: impl Into<Look>,
        render_node: &std::path::Path,
    ) -> Result<std::sync::Arc<DmaFrame>> {
        let layout = DmaLayout::new(width, height, ten_bit).map_err(error)?;
        let look = look.into();
        self.retrying(|| {
            self.prepare_video_planes(
                frame,
                width,
                height,
                ten_bit,
                look,
                Destination::Dma(render_node, layout),
            )
        })?
        .dma
        .map(std::sync::Arc::new)
        .ok_or_else(|| error("Missing DMA-BUF output"))
    }
    fn prepare_video_planes(
        &self,
        frame: &GpuFrame,
        width: u32,
        height: u32,
        ten_bit: bool,
        look: Look,
        destination: Destination<'_, '_>,
    ) -> Result<Prepared> {
        let matte = look.matte;
        if !self.owns_frame(frame)
            || frame.region.x != 0
            || frame.region.y != 0
            || frame.region.width != frame.width
            || frame.region.height != frame.height
        {
            return Err(error("video export needs this renderer's full frame"));
        }
        if width < 2
            || height < 2
            || width > 8192
            || height > 8192
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || matte
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(error(
                "video needs even dimensions in 2..8192 and a valid matte",
            ));
        }
        let layout = match &destination {
            Destination::Host(layout) => *layout,
            Destination::Dma(_, layout) => Some(*layout),
            Destination::Cuda(_) => None,
        };
        let host = matches!(destination, Destination::Host(_));
        let compact_bytes = u64::from(width) * u64::from(height) * 3 / if ten_bit { 1 } else { 2 };
        let bytes = layout.map_or(compact_bytes, |l| l.bytes);
        let size = bytes.div_ceil(4) * 4;
        if size > self.device.limits().max_storage_buffer_binding_size
            || size > self.device.limits().max_buffer_size
        {
            return Err(error("video output exceeds GPU limits"));
        }
        let reservation = if !host {
            size.div_ceil(65536) * 65536
        } else {
            0
        };
        let (mut pool, available) = self.admit(reservation)?;
        if size + size.max(reservation) + 48 > available {
            return Err(error("video output exceeds GPU budget"));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let mut interop_failed = false;
        let result = (|| {
            let mut resources = pool.frame(self.budget_bytes.saturating_sub(reservation));
            let packed = resources.take(
                &self.device,
                size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "encoder planes",
            )?;
            let read = if host {
                Some(resources.take(
                    &self.device,
                    size,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    "encoder bridge",
                )?)
            } else {
                None
            };
            let params = resources.take(
                &self.device,
                48,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "encoder colour",
            )?;
            let mut values = Vec::new();
            values.extend_from_slice(bytemuck::cast_slice(&[
                frame.width,
                frame.height,
                width,
                height,
            ]));
            let filmic = f32::from(u8::from(!look.display.is_standard()));
            values.extend_from_slice(bytemuck::cast_slice(&[
                matte[0], matte[1], matte[2], filmic,
            ]));
            values.extend_from_slice(bytemuck::cast_slice(&[
                if ten_bit { 10u32 } else { 8 },
                layout.map_or(0, |l| l.pitch / if ten_bit { 2 } else { 1 }),
                layout.map_or(0, |l| l.uv_offset / if ten_bit { 2 } else { 1 }),
                layout.map_or(0, |l| (l.bytes / if ten_bit { 2 } else { 1 }) as u32),
            ]));
            self.queue.write_buffer(&params, 0, &values);
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video export"),
                layout: &self.encode_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame.buffer().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: packed.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.encode_pipeline);
                pass.set_bind_group(0, &group, &[]);
                let groups = (size / 4).div_ceil(64) as u32;
                let columns = groups.min(65535);
                pass.dispatch_workgroups(columns, groups.div_ceil(columns), 1);
            }
            if let Some(read) = &read {
                encoder.copy_buffer_to_buffer(&packed, 0, read, 0, size);
            }
            resources.submitted();
            let submission = self.queue.submit([encoder.finish()]);
            let data = if let Some(read) = &read {
                let mut data: Vec<u8> = self.map_values(read, submission)?;
                data.truncate(bytes as usize);
                data
            } else {
                self.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(30)),
                    })
                    .map_err(error)?;
                Vec::new()
            };
            resources.completed();
            let mut dma = None;
            match destination {
                Destination::Cuda(destination) => {
                    destination
                        .copy_from_vulkan(&self.device, &self.queue, &packed, reservation)
                        .map_err(|e| {
                            interop_failed = e.is::<veylvik_interop::FatalInterop>();
                            error(e)
                        })?;
                }
                Destination::Dma(node, layout) => {
                    dma = Some(
                        DmaFrame::export(
                            &self.device,
                            &self.queue,
                            &packed,
                            layout,
                            node,
                            reservation,
                        )
                        .map_err(|e| {
                            interop_failed = e.is::<veylvik_interop::FatalInterop>();
                            error(e)
                        })?,
                    );
                }
                Destination::Host(_) => {}
            }
            Ok(Prepared { bytes: data, dma })
        })();
        let result = result.map(|mut prepared| {
            if let Some(dma) = &mut prepared.dma {
                dma.retain_accounting(std::sync::Arc::new(
                    pool.hold_external(dma.allocation_size()),
                ));
            }
            prepared
        });
        if interop_failed {
            crate::gpu_session::unusable(&mut pool);
        }
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = crate::gpu_session::caught(&mut pool, allocation_error, validation_error) {
            return Err(e);
        }
        result
    }
}
