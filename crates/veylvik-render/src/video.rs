//! Host upload or direct CUDA planes, followed by Vulkan colour conversion.
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
enum Input<'a> {
    Host(&'a [u8]),
    Cuda(&'a veylvik_interop::CudaFrame),
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
        self.convert_video(Input::Host(bytes), encoding, lut, None)
    }
    /// Converts decoded planes, area-reducing to `target` (after rotation) when given. A
    /// target may not exceed the rotated source size.
    pub fn import_video_sized(
        &self,
        bytes: &[u8],
        encoding: VideoEncoding,
        lut: &[f32],
        target: Option<[u32; 2]>,
    ) -> Result<GpuFrame> {
        self.convert_video(Input::Host(bytes), encoding, lut, target)
    }
    pub fn import_cuda_video(
        &self,
        frame: &veylvik_interop::CudaFrame,
        encoding: VideoEncoding,
        lut: &[f32],
    ) -> Result<GpuFrame> {
        self.import_cuda_video_sized(frame, encoding, lut, None)
    }
    /// CUDA planes, optionally area-reduced to `target` as for [`Self::import_video_sized`].
    pub fn import_cuda_video_sized(
        &self,
        frame: &veylvik_interop::CudaFrame,
        encoding: VideoEncoding,
        lut: &[f32],
        target: Option<[u32; 2]>,
    ) -> Result<GpuFrame> {
        if frame.dimensions() != [encoding.width, encoding.height]
            || !matches!(
                (frame.sample_bytes(), encoding.format),
                (1, VideoFormat::Nv12) | (2, VideoFormat::P010)
            )
        {
            return Err(error("CUDA planes do not match video encoding"));
        }
        self.convert_video(Input::Cuda(frame), encoding, lut, target)
    }
    fn convert_video(
        &self,
        input: Input<'_>,
        encoding: VideoEncoding,
        lut: &[f32],
        target: Option<[u32; 2]>,
    ) -> Result<GpuFrame> {
        let byte_len = match input {
            Input::Host(bytes) => bytes.len() as u64,
            Input::Cuda(frame) => frame.byte_len(),
        };
        if encoding.byte_len()? as u64 != byte_len
            || lut.len() != 65536
            || lut
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=10000.0).contains(v))
        {
            return Err(error("invalid decoded video buffer or transfer table"));
        }
        let (full_width, full_height) = encoding.dimensions();
        let [width, height] = target.unwrap_or([full_width, full_height]);
        if width == 0 || height == 0 || width > full_width || height > full_height {
            return Err(error("video target size must be within the source size"));
        }
        let size = u64::from(width) * u64::from(height) * 16;
        let raw_size = byte_len.div_ceil(4) * 4;
        let external_reservation = if matches!(input, Input::Cuda(_)) {
            raw_size.div_ceil(65536) * 65536
        } else {
            0
        };
        let limits = self.device.limits();
        if size.max(raw_size) > limits.max_storage_buffer_binding_size
            || size.max(raw_size) > limits.max_buffer_size
        {
            return Err(error("video conversion exceeds adapter limits"));
        }
        let (mut pool, available) = self.admit(external_reservation)?;
        if size + raw_size.max(external_reservation) + 262144 + 112 > available {
            return Err(error("video conversion exceeds GPU budget"));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let mut interop_failed = false;
        let result = (|| {
            let mut resources = pool.frame(self.budget_bytes.saturating_sub(external_reservation));
            let raw = match input {
                Input::Cuda(frame) => frame
                    .into_vulkan(&self.device, &self.queue, external_reservation)
                    .map_err(|e| {
                        interop_failed = e.is::<veylvik_interop::FatalInterop>();
                        error(e)
                    })?,
                Input::Host(_) => resources.take(
                    &self.device,
                    raw_size,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    "decoded video planes",
                )?,
            };
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
                112,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "video colour encoding",
            )?;
            if let Input::Host(bytes) = input {
                let mut padded = bytes.to_vec();
                padded.resize(raw_size as usize, 0);
                self.queue.write_buffer(&raw, 0, &padded);
            }
            self.queue
                .write_buffer(&table, 0, bytemuck::cast_slice(lut));
            let mut params = Vec::with_capacity(112);
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
            params.extend_from_slice(bytemuck::cast_slice(&[width, height, 0, 0]));
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
                flow_report: None,
                identity: frame::next_identity()?,
                storage: std::sync::Arc::new(resources.pin(&out)),
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
