//! Bounded decoded-plane upload and Vulkan colour conversion. No linear pixel readback.
use crate::{GpuFrame, Region, Renderer, Result, error, frame};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VideoFormat {
    Rgba64 = 0,
    Nv12 = 1,
    P010 = 2,
}
#[derive(Clone, Copy)]
pub struct VideoEncoding {
    pub width: u32,
    pub height: u32,
    pub format: VideoFormat,
    pub rotation: u32,
    pub kr: f32,
    pub kb: f32,
    pub full_range: bool,
    pub chroma_location: [f32; 2],
    pub primaries: [[f32; 3]; 3],
}
impl VideoEncoding {
    pub fn byte_len(self) -> Result<usize> {
        if self.width == 0
            || self.height == 0
            || self.width > 8192
            || self.height > 8192
            || ![0, 90, 180, 270].contains(&self.rotation)
            || (self.format != VideoFormat::Rgba64
                && (!self.width.is_multiple_of(2) || !self.height.is_multiple_of(2)))
            || !self.kr.is_finite()
            || !self.kb.is_finite()
            || self.kr <= 0.0
            || self.kb <= 0.0
            || self.kr + self.kb >= 1.0
            || self
                .primaries
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 16.0)
            || self
                .chroma_location
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(error("invalid decoded video geometry or colour encoding"));
        }
        let pixels = self.width as usize * self.height as usize;
        Ok(match self.format {
            VideoFormat::Rgba64 => pixels * 8,
            VideoFormat::Nv12 => pixels * 3 / 2,
            VideoFormat::P010 => pixels * 3,
        })
    }
    pub fn dimensions(self) -> (u32, u32) {
        if self.rotation == 90 || self.rotation == 270 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}
pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("decoded video conversion"),
        source: wgpu::ShaderSource::Wgsl(include_str!("video.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("decoded video conversion"),
        layout: None,
        module: &module,
        entry_point: Some("convert"),
        compilation_options: Default::default(),
        cache: None,
    })
}
impl Renderer {
    pub fn owns_frame(&self, frame: &GpuFrame) -> bool {
        std::sync::Arc::ptr_eq(&self.owner, &frame.owner)
    }
    pub fn import_video(
        &self,
        bytes: &[u8],
        encoding: VideoEncoding,
        lut: &[f32],
    ) -> Result<GpuFrame> {
        if encoding.byte_len()? != bytes.len()
            || lut.len() != 65536
            || lut
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=10000.0).contains(v))
        {
            return Err(error("invalid decoded video buffer or transfer table"));
        }
        let (width, height) = encoding.dimensions();
        let size = u64::from(width) * u64::from(height) * 16;
        let raw_size = (bytes.len() as u64).div_ceil(4) * 4;
        let limits = self.device.limits();
        if size.max(raw_size) > limits.max_storage_buffer_binding_size
            || size.max(raw_size) > limits.max_buffer_size
        {
            return Err(error("video conversion exceeds adapter limits"));
        }
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        if pool.failed {
            return Err(error("renderer unavailable after a GPU failure"));
        }
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
        if size + raw_size + 262144 + 96 > available {
            return Err(error("video conversion exceeds GPU budget"));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = (|| {
            let mut resources = pool.frame(self.budget_bytes);
            let raw = resources.take(
                &self.device,
                raw_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                "decoded video planes",
            )?;
            let table = resources.take(
                &self.device,
                262144,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                "video transfer LUT",
            )?;
            let out = resources.take(
                &self.device,
                size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "linear video frame",
            )?;
            let uniform = resources.take(
                &self.device,
                96,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "video colour encoding",
            )?;
            let mut padded = bytes.to_vec();
            padded.resize(raw_size as usize, 0);
            self.queue.write_buffer(&raw, 0, &padded);
            self.queue
                .write_buffer(&table, 0, bytemuck::cast_slice(lut));
            let mut params = Vec::with_capacity(96);
            params.extend_from_slice(bytemuck::cast_slice(&[
                encoding.width,
                encoding.height,
                encoding.format as u32,
                encoding.rotation,
            ]));
            params.extend_from_slice(bytemuck::cast_slice(&[
                encoding.kr,
                encoding.kb,
                if encoding.full_range { 1.0 } else { 0.0 },
                0.0,
            ]));
            for row in encoding.primaries {
                params.extend_from_slice(bytemuck::cast_slice(&[row[0], row[1], row[2], 0.0]));
            }
            params.extend_from_slice(bytemuck::cast_slice(&[
                encoding.chroma_location[0],
                encoding.chroma_location[1],
                0.0,
                0.0,
            ]));
            self.queue.write_buffer(&uniform, 0, &params);
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("decoded video"),
                layout: &self.video_pipeline.get_bind_group_layout(0),
                entries: &[&raw, &table, &out, &uniform]
                    .into_iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&self.video_pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            resources.submitted();
            let submission = self.queue.submit([encoder.finish()]);
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(std::time::Duration::from_secs(30)),
                })
                .map_err(error)?;
            resources.completed();
            Ok(GpuFrame {
                identity: frame::next_identity()?,
                storage: resources.pin(&out),
                owner: self.owner.clone(),
                width,
                height,
                region: Region {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
            })
        })();
        let failure = pollster::block_on(allocation.pop()).or(pollster::block_on(validation.pop()));
        if let Some(e) = failure {
            pool.failed = true;
            return Err(error(e));
        }
        result
    }
}
