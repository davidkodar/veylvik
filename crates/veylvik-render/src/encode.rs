//! SDR Rec.709 video boundary: GPU resize, matte, transfer and YUV packing.
use crate::{GpuFrame, Renderer, Result, error};
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
impl Renderer {
    /// Return compact NV12/P010 for the staged hardware-encoder bridge. All pixel
    /// arithmetic runs on Vulkan; linear RGBA is never read back by this path.
    pub fn encode_video_planes(
        &self,
        frame: &GpuFrame,
        width: u32,
        height: u32,
        ten_bit: bool,
        matte: [f32; 3],
    ) -> Result<Vec<u8>> {
        self.prepare_video_planes(frame, width, height, ten_bit, matte, None)
    }
    /// GPU-only handoff to a uniquely writable CUDA encoder frame.
    pub fn encode_video_cuda(
        &self,
        frame: &GpuFrame,
        matte: [f32; 3],
        destination: &mut veylvik_interop::CudaDestination<'_>,
    ) -> Result<()> {
        let [width, height] = destination.dimensions();
        self.prepare_video_planes(
            frame,
            width,
            height,
            destination.sample_bytes() == 2,
            matte,
            Some(destination),
        )
        .map(|_| ())
    }
    fn prepare_video_planes(
        &self,
        frame: &GpuFrame,
        width: u32,
        height: u32,
        ten_bit: bool,
        matte: [f32; 3],
        destination: Option<&mut veylvik_interop::CudaDestination<'_>>,
    ) -> Result<Vec<u8>> {
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
        let bytes = u64::from(width) * u64::from(height) * 3 / if ten_bit { 1 } else { 2 };
        let size = bytes.div_ceil(4) * 4;
        if size > self.device.limits().max_storage_buffer_binding_size
            || size > self.device.limits().max_buffer_size
        {
            return Err(error("video output exceeds GPU limits"));
        }
        let reservation = if destination.is_some() {
            size.div_ceil(65536) * 65536
        } else {
            0
        };
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("GPU pool unavailable"))?;
        if pool.failed {
            return Err(error("renderer unavailable after GPU failure"));
        }
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(
            self.pool_limit_bytes
                .min(available.saturating_sub(reservation)),
        );
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
            let read = if destination.is_none() {
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
            values.extend_from_slice(bytemuck::cast_slice(&[matte[0], matte[1], matte[2], 0.]));
            values.extend_from_slice(bytemuck::cast_slice(&[
                if ten_bit { 10u32 } else { 8 },
                0,
                0,
                0,
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
            if let Some(destination) = destination {
                destination
                    .copy_from_vulkan(&self.device, &self.queue, &packed, reservation)
                    .map_err(|e| {
                        interop_failed = e.is::<veylvik_interop::FatalInterop>();
                        error(e)
                    })?;
            }
            Ok(data)
        })();
        if interop_failed {
            pool.failed = true;
        }
        if let Some(e) =
            pollster::block_on(allocation.pop()).or(pollster::block_on(validation.pop()))
        {
            pool.failed = true;
            return Err(error(e));
        }
        result
    }
}
