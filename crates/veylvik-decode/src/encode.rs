//! In-process NVENC/CUDA and VA-API/DMA-BUF encoding without host pixels.
use super::*;

#[derive(Clone, Copy)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: [i32; 2],
    pub ten_bit: bool,
    pub quality: u32,
    pub fast: bool,
}
struct Frames(*mut ffi::AVBufferRef);
impl Drop for Frames {
    fn drop(&mut self) {
        // SAFETY: Exactly one owned reference; codecs/frames retain their own refs.
        unsafe {
            ffi::av_buffer_unref(&mut self.0);
        }
    }
}
/// An unsubmitted frame, exclusively writable until consumed by Encoder::send.
pub struct Frame {
    frame: Arc<HardwareFrame>,
    owner: Arc<dyn Send + Sync>,
    initialized: bool,
}
impl Frame {
    fn destination(&mut self) -> Result<veylvik_interop::CudaDestination<'_>> {
        if self.frame.frame.format() != Pixel::CUDA {
            return Err("CUDA destination requires a CUDA frame".into());
        }
        // SAFETY: This newly allocated frame has not been submitted. No other API
        // exposes its planes or mutates it. The mutable borrow prevents submit
        // until the destination is dropped; owner retains the allocation/device.
        unsafe {
            let device = &*(*self.frame._device.reference)
                .data
                .cast::<ffi::AVHWDeviceContext>();
            let cuda = &*device.hwctx.cast::<ffi::AVCUDADeviceContext>();
            let frame = &*self.frame.frame.as_ptr();
            if frame.linesize[0] <= 0 || frame.linesize[1] <= 0 {
                return Err("Invalid encoder CUDA frame pitch".into());
            }
            veylvik_interop::CudaDestination::from_raw(
                cuda.cuda_ctx.cast(),
                cuda.stream.cast(),
                [
                    (frame.data[0] as u64, frame.linesize[0] as usize),
                    (frame.data[1] as u64, frame.linesize[1] as usize),
                ],
                [self.frame.frame.width(), self.frame.frame.height()],
                if self.frame.format == PlaneFormat::P010 {
                    2
                } else {
                    1
                },
                &mut self.owner,
            )
        }
    }
    /// Fill completely before this frame can be staged or submitted. The closure
    /// cannot retain a destination or submit the frame while it is writable.
    pub fn fill(
        &mut self,
        prepare: impl FnOnce(&mut veylvik_interop::CudaDestination<'_>) -> Result<()>,
    ) -> Result<()> {
        self.initialized = false;
        let mut destination = self.destination()?;
        prepare(&mut destination)?;
        if !destination.is_written() {
            return Err("Encoder frame was not initialized by a GPU transfer".into());
        }
        drop(destination);
        self.initialized = true;
        Ok(())
    }
    /// Explicit diagnostic readback, never called by the export pipeline.
    pub fn stage(&self) -> Result<Vec<u8>> {
        if !self.initialized {
            return Err("Encoder frame is not initialized".into());
        }
        self.frame.stage()
    }
}
pub struct Encoder {
    codec: ff::encoder::video::Encoder,
    output: ff::format::context::Output,
    frames: Frames,
    device: Arc<Device>,
    settings: Settings,
    kind: DeviceKind,
    time_base: ff::Rational,
    stream_time_base: ff::Rational,
    submitted: i64,
    packets: u64,
}
impl Encoder {
    pub fn open(path: &Path, s: Settings) -> Result<Self> {
        Self::open_on(path, s, DeviceKind::Cuda)
    }
    pub fn open_on(path: &Path, s: Settings, kind: DeviceKind) -> Result<Self> {
        if s.width < 2
            || s.height < 2
            || s.width > 8192
            || s.height > 8192
            || !s.width.is_multiple_of(2)
            || !s.height.is_multiple_of(2)
            || s.fps[0] <= 0
            || s.fps[1] <= 0
            || s.fps[0] > 240_000
            || s.fps[1] > 100_000
            || s.quality > 51
        {
            return Err("Invalid native encoder settings".into());
        }
        ff::init()?;
        let device = Device::shared(&kind)?;
        // SAFETY: Valid retained hardware device. Frames takes ownership immediately.
        let frames = Frames(unsafe { ffi::av_hwframe_ctx_alloc(device.reference) });
        if frames.0.is_null() {
            return Err("Cannot allocate encoder frame pool".into());
        }
        // SAFETY: Exclusive, uninitialized pool context with the matching hardware format.
        unsafe {
            let pool = &mut *(*frames.0).data.cast::<ffi::AVHWFramesContext>();
            pool.format = if matches!(kind, DeviceKind::Cuda) {
                ffi::AVPixelFormat::AV_PIX_FMT_CUDA
            } else {
                ffi::AVPixelFormat::AV_PIX_FMT_VAAPI
            };
            pool.sw_format = if s.ten_bit {
                ffi::AVPixelFormat::AV_PIX_FMT_P010LE
            } else {
                ffi::AVPixelFormat::AV_PIX_FMT_NV12
            };
            pool.width = s.width as i32;
            pool.height = s.height as i32;
            pool.initial_pool_size = 0;
            let ret = ffi::av_hwframe_ctx_init(frames.0);
            if ret < 0 {
                return Err(ff::Error::from(ret).into());
            }
        }
        let codec_name = match (&kind, s.ten_bit) {
            (DeviceKind::Cuda, false) => "h264_nvenc",
            (DeviceKind::Cuda, true) => "hevc_nvenc",
            (DeviceKind::Vaapi(_), false) => "h264_vaapi",
            (DeviceKind::Vaapi(_), true) => "hevc_vaapi",
        };
        let codec = ff::encoder::find_by_name(codec_name).ok_or("Hardware encoder unavailable")?;
        let mut context = ff::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        let time_base = ff::Rational(s.fps[1], s.fps[0]);
        context.set_width(s.width);
        context.set_height(s.height);
        context.set_format(if matches!(kind, DeviceKind::Cuda) {
            Pixel::CUDA
        } else {
            Pixel::VAAPI
        });
        context.set_time_base(time_base);
        context.set_frame_rate(Some(ff::Rational(s.fps[0], s.fps[1])));
        context.set_aspect_ratio(ff::Rational(1, 1));
        context.set_flags(ff::codec::Flags::GLOBAL_HEADER);
        context.set_max_b_frames(0);
        context.set_bit_rate(0);
        // SAFETY: Exclusive unopened codec context. It owns the checked new pool
        // reference and releases it on every failure path through its destructor.
        unsafe {
            let reference = ffi::av_buffer_ref(frames.0);
            if reference.is_null() {
                return Err("Cannot retain encoder pool".into());
            }
            let ctx = &mut *context.as_mut_ptr();
            ctx.hw_frames_ctx = reference;
            ctx.color_primaries = ffi::AVColorPrimaries::AVCOL_PRI_BT709;
            ctx.color_trc = ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            ctx.colorspace = ffi::AVColorSpace::AVCOL_SPC_BT709;
            ctx.color_range = ffi::AVColorRange::AVCOL_RANGE_MPEG;
            ctx.chroma_sample_location = ffi::AVChromaLocation::AVCHROMA_LOC_CENTER;
        }
        let mut options = ff::Dictionary::new();
        options.set("profile", if s.ten_bit { "main10" } else { "high" });
        match &kind {
            DeviceKind::Cuda => {
                options.set("preset", if s.fast { "p2" } else { "p5" });
                options.set("rc", "vbr");
                options.set("cq", &s.quality.to_string());
                options.set("rc-lookahead", "0");
                options.set("surfaces", "4");
                options.set("delay", "0");
                options.set("zerolatency", "1");
            }
            DeviceKind::Vaapi(_) => {
                options.set("rc_mode", "CQP");
                options.set("qp", &s.quality.to_string());
                options.set("async_depth", "1");
            }
        }
        let codec = context.open_with(options)?;
        let mut output = ff::format::output_as(path, "mp4")?;
        {
            let mut stream = output.add_stream(None)?;
            stream.set_time_base(time_base);
            stream.set_rate(ff::Rational(s.fps[0], s.fps[1]));
            stream.set_avg_frame_rate(ff::Rational(s.fps[0], s.fps[1]));
            stream.set_parameters(&codec);
            if s.ten_bit {
                // SAFETY: Mutable stream belongs to this output; only codec tag
                // is adjusted to identify parameter sets stored in MP4 headers.
                unsafe {
                    (*stream.parameters().as_mut_ptr()).codec_tag = u32::from_le_bytes(*b"hvc1");
                }
            }
        }
        let mut mux = ff::Dictionary::new();
        mux.set("movflags", "+faststart");
        output.write_header_with(mux)?;
        let stream_time_base = output.stream(0).ok_or("Missing output stream")?.time_base();
        Ok(Self {
            codec,
            output,
            frames,
            device,
            settings: s,
            kind,
            time_base,
            stream_time_base,
            submitted: 0,
            packets: 0,
        })
    }
    pub fn frame(&self) -> Result<Frame> {
        let mut frame = ff::frame::Video::empty();
        // SAFETY: Initialized retained pool and a fresh AVFrame. Success retains
        // its own pool/allocation refs; errors are handled by frame's destructor.
        unsafe {
            let ret = ffi::av_hwframe_get_buffer(self.frames.0, frame.as_mut_ptr(), 0);
            if ret < 0 {
                return Err(ff::Error::from(ret).into());
            }
        }
        frame.set_pts(Some(self.submitted));
        let frame = Arc::new(HardwareFrame {
            frame,
            format: if self.settings.ten_bit {
                PlaneFormat::P010
            } else {
                PlaneFormat::Nv12
            },
            _device: self.device.clone(),
        });
        Ok(Frame {
            initialized: false,
            owner: frame.clone(),
            frame,
        })
    }
    /// Import immutable linear DMA-BUF planes without uploading or mapping pixels
    /// on the CPU. FFmpeg retains the DRM frame and its Vulkan owner until done.
    pub fn map_dma(&self, source: Arc<veylvik_interop::DmaFrame>) -> Result<Frame> {
        use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
        let DeviceKind::Vaapi(node) = &self.kind else {
            return Err("DMA-BUF requires a VA-API encoder".into());
        };
        let layout = source.layout();
        layout.validate()?;
        if layout.width != self.settings.width
            || layout.height != self.settings.height
            || layout.ten_bit != self.settings.ten_bit
            || std::fs::metadata(node)?.rdev() != source.render_device()
        {
            return Err("DMA-BUF does not match the encoder geometry, format or device".into());
        }
        // SAFETY: All-zero C descriptor has no invalid enum/pointer values. Its
        // fully populated plane/object entries are checked by DmaLayout::validate.
        let mut descriptor: ffi::AVDRMFrameDescriptor = unsafe { std::mem::zeroed() };
        descriptor.nb_objects = 1;
        descriptor.nb_layers = 1;
        descriptor.objects[0].fd = source.fd().as_raw_fd();
        descriptor.objects[0].size = source.allocation_size() as usize;
        descriptor.objects[0].format_modifier = 0; // DRM_FORMAT_MOD_LINEAR, never guessed for tiled data.
        descriptor.layers[0].format =
            u32::from_le_bytes(if layout.ten_bit { *b"P010" } else { *b"NV12" });
        descriptor.layers[0].nb_planes = 2;
        for (i, offset) in [0, layout.uv_offset].into_iter().enumerate() {
            descriptor.layers[0].planes[i].object_index = 0;
            descriptor.layers[0].planes[i].offset = offset as isize;
            descriptor.layers[0].planes[i].pitch = layout.pitch as isize;
        }
        let mut owner = Box::new(DrmOwner {
            descriptor,
            _source: source,
        });
        let data = ptr::from_mut(&mut owner.descriptor).cast::<u8>();
        let owner = Box::into_raw(owner);
        let mut drm = ff::frame::Video::empty();
        drm.set_format(Pixel::DRM_PRIME);
        drm.set_width(layout.width);
        drm.set_height(layout.height);
        let mut mapped = ff::frame::Video::empty();
        mapped.set_format(Pixel::VAAPI);
        // SAFETY: The AVBuffer callback owns the stable Box, including descriptor,
        // FD, and Vulkan allocation. av_hwframe_map retains source buffer refs.
        // On failure both AVFrames unref normally; no borrowed FD is closed here.
        unsafe {
            let buffer = ffi::av_buffer_create(
                data,
                std::mem::size_of::<ffi::AVDRMFrameDescriptor>(),
                Some(drop_drm),
                owner.cast(),
                0,
            );
            if buffer.is_null() {
                drop(Box::from_raw(owner));
                return Err("Cannot retain DMA-BUF descriptor".into());
            }
            (*drm.as_mut_ptr()).buf[0] = buffer;
            (*drm.as_mut_ptr()).data[0] = data;
            (*mapped.as_mut_ptr()).hw_frames_ctx = ffi::av_buffer_ref(self.frames.0);
            if (*mapped.as_ptr()).hw_frames_ctx.is_null() {
                return Err("Cannot retain VA-API frame pool".into());
            }
            let ret = ffi::av_hwframe_map(
                mapped.as_mut_ptr(),
                drm.as_ptr(),
                ffi::AV_HWFRAME_MAP_READ as i32 | ffi::AV_HWFRAME_MAP_DIRECT as i32,
            );
            if ret < 0 {
                return Err(ff::Error::from(ret).into());
            }
        }
        if mapped.format() != Pixel::VAAPI
            || mapped.width() != layout.width
            || mapped.height() != layout.height
        {
            return Err("VA-API DMA-BUF mapping changed geometry or format".into());
        }
        mapped.set_pts(Some(self.submitted));
        let frame = Arc::new(HardwareFrame {
            frame: mapped,
            format: if layout.ten_bit {
                PlaneFormat::P010
            } else {
                PlaneFormat::Nv12
            },
            _device: self.device.clone(),
        });
        Ok(Frame {
            initialized: true,
            owner: frame.clone(),
            frame,
        })
    }
    pub fn send(&mut self, frame: Frame) -> Result<()> {
        // SAFETY: A live hardware frame owns its frame-pool reference. Checking
        // identity rejects frames from other encoders, even on the same device.
        let same_pool = unsafe {
            let reference = (*frame.frame.frame.as_ptr()).hw_frames_ctx;
            !reference.is_null() && (*reference).data == (*self.frames.0).data
        };
        if !frame.initialized || !same_pool || frame.frame.frame.pts() != Some(self.submitted) {
            return Err("Encoder frame belongs to another stream or timestamp".into());
        }
        self.codec.send_frame(&frame.frame.frame)?;
        self.submitted += 1;
        self.drain(false)?;
        Ok(())
    }
    fn drain(&mut self, finishing: bool) -> Result<()> {
        loop {
            let mut packet = ff::Packet::empty();
            match self.codec.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(0);
                    packet.rescale_ts(self.time_base, self.stream_time_base);
                    packet.write_interleaved(&mut self.output)?;
                    self.packets += 1;
                }
                Err(ff::Error::Eof) if finishing => return Ok(()),
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN && !finishing => {
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    pub fn finish(mut self) -> Result<()> {
        self.codec.send_eof()?;
        self.drain(true)?;
        if self.packets != self.submitted as u64 {
            return Err("Encoder did not output every frame".into());
        }
        self.output.write_trailer()?;
        Ok(())
    }
}

struct DrmOwner {
    descriptor: ffi::AVDRMFrameDescriptor,
    _source: Arc<veylvik_interop::DmaFrame>,
}
unsafe extern "C" fn drop_drm(opaque: *mut std::ffi::c_void, _data: *mut u8) {
    // SAFETY: av_buffer_create takes exactly one Box ownership on success and
    // invokes this callback once, after all source/mapped frame refs are gone.
    unsafe {
        drop(Box::from_raw(opaque.cast::<DrmOwner>()));
    }
}
