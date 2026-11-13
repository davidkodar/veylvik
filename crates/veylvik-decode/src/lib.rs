//! In-process hardware decoding. No raw FFmpeg handle escapes this crate.
//!
//! Device contexts are reference counted across clips. Frames retain FFmpeg's
//! hardware allocation until explicitly staged; this is not Vulkan interop yet.
use ff::{ffi, format::Pixel};
use ffmpeg_next as ff;
use std::{
    collections::HashMap,
    ffi::CString,
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum DeviceKind {
    Cuda,
    Vaapi(PathBuf),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneFormat {
    Nv12,
    P010,
}
impl PlaneFormat {
    fn pixel(self) -> Pixel {
        match self {
            Self::Nv12 => Pixel::NV12,
            Self::P010 => Pixel::P010LE,
        }
    }
}
struct Device {
    reference: *mut ffi::AVBufferRef,
    id: u64,
}
// SAFETY: The initialized hardware context is immutable. FFmpeg's AVBuffer
// refcount is thread-safe; codecs receive their own references. CUDA operations
// use FFmpeg's push/pop context handling, not a caller's thread-local context.
unsafe impl Send for Device {}
// SAFETY: Shared access only refs the immutable context; its sole owning ref
// is released after all Arc users and their codec references have gone away.
unsafe impl Sync for Device {}
impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: This is the one owned reference; unref nulls it and frees at zero.
        unsafe {
            ffi::av_buffer_unref(&mut self.reference);
        }
    }
}
impl Device {
    fn shared(kind: &DeviceKind) -> Result<Arc<Self>> {
        static DEVICES: OnceLock<Mutex<HashMap<DeviceKind, Weak<Device>>>> = OnceLock::new();
        static IDS: AtomicU64 = AtomicU64::new(1);
        let mut devices = DEVICES
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Hardware device registry unavailable")?;
        if let Some(device) = devices.get(kind).and_then(Weak::upgrade) {
            return Ok(device);
        }
        devices.retain(|_, d| d.strong_count() != 0);
        let path = match kind {
            DeviceKind::Cuda => None,
            DeviceKind::Vaapi(p) => Some(CString::new(p.as_os_str().as_encoded_bytes())?),
        };
        let ty = match kind {
            DeviceKind::Cuda => ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
            DeviceKind::Vaapi(_) => ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        };
        let mut reference = ptr::null_mut();
        let mut options = ff::Dictionary::new();
        if matches!(kind, DeviceKind::Cuda) {
            options.set("primary_ctx", "1");
        }
        // SAFETY: All pointers remain alive for the synchronous call. FFmpeg
        // returns one owned reference on success, transferred to Device below.
        let ret = unsafe {
            ffi::av_hwdevice_ctx_create(
                &mut reference,
                ty,
                path.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
                options.as_mut_ptr(),
                0,
            )
        };
        if ret < 0 {
            return Err(ff::Error::from(ret).into());
        }
        if reference.is_null() {
            return Err("Hardware device returned no context".into());
        }
        let device = Arc::new(Self {
            reference,
            id: IDS.fetch_add(1, Ordering::Relaxed),
        });
        devices.insert(kind.clone(), Arc::downgrade(&device));
        Ok(device)
    }
}
unsafe extern "C" fn hardware_format(
    ctx: *mut ffi::AVCodecContext,
    formats: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    // SAFETY: FFmpeg calls with its live context and a NONE-terminated format
    // array. We install this only after giving the codec a hardware device ref.
    unsafe {
        if ctx.is_null() || formats.is_null() || (*ctx).hw_device_ctx.is_null() {
            return ffi::AVPixelFormat::AV_PIX_FMT_NONE;
        }
        let device = &*((*(*ctx).hw_device_ctx)
            .data
            .cast::<ffi::AVHWDeviceContext>());
        let wanted = match device.type_ {
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA => ffi::AVPixelFormat::AV_PIX_FMT_CUDA,
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI => ffi::AVPixelFormat::AV_PIX_FMT_VAAPI,
            _ => return ffi::AVPixelFormat::AV_PIX_FMT_NONE,
        };
        let mut p = formats;
        while *p != ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == wanted {
                return wanted;
            }
            p = p.add(1);
        }
    }
    // Never accept a silent software fallback through this callback.
    ffi::AVPixelFormat::AV_PIX_FMT_NONE
}

/// Owns the hardware allocation. A decoder can seek/drop while this stays alive.
pub struct HardwareFrame {
    frame: ff::frame::Video,
    format: PlaneFormat,
    _device: Arc<Device>,
}
impl HardwareFrame {
    /// Explicit device-to-host transfer. Padding is stripped, never interpreted
    /// as pixels. Transfer/size/format failures return before exposing any bytes.
    pub fn stage(&self) -> Result<Vec<u8>> {
        let mut host = ff::frame::Video::empty();
        host.set_format(self.format.pixel());
        // SAFETY: Both AVFrames are live and distinct. FFmpeg allocates host
        // buffers and retains source ownership until the synchronous copy ends.
        let ret =
            unsafe { ffi::av_hwframe_transfer_data(host.as_mut_ptr(), self.frame.as_ptr(), 0) };
        if ret < 0 {
            return Err(ff::Error::from(ret).into());
        }
        if host.format() != self.format.pixel()
            || host.width() != self.frame.width()
            || host.height() != self.frame.height()
        {
            return Err("Hardware transfer changed frame format or dimensions".into());
        }
        let row = host.width() as usize
            * if self.format == PlaneFormat::P010 {
                2
            } else {
                1
            };
        let height = host.height() as usize;
        let mut bytes = Vec::with_capacity(row * height * 3 / 2);
        for (plane, rows) in [(0, height), (1, height / 2)] {
            // SAFETY: A successful FFmpeg transfer owns valid plane buffers.
            // Validate positive pitch before using ffmpeg-next's slice access.
            let pitch = unsafe { (*host.as_ptr()).linesize[plane] };
            if pitch < row as i32 {
                return Err("Invalid hardware transfer pitch".into());
            }
            let data = host.data(plane);
            for y in 0..rows {
                let start = y * pitch as usize;
                bytes.extend_from_slice(
                    data.get(start..start + row)
                        .ok_or("Truncated hardware plane")?,
                );
            }
        }
        Ok(bytes)
    }
}

// The interrupt allocation outlives the demuxer, including its destructor.
// Only FFmpeg's blocking demux calls are interruptible; a wedged GPU driver
// cannot be forcibly killed safely inside the process.
struct Deadline {
    origin: Instant,
    until_ms: AtomicU64,
}
impl Deadline {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            until_ms: AtomicU64::new(30_000),
        }
    }
    fn arm(&self) {
        self.until_ms.store(
            self.origin.elapsed().as_millis() as u64 + 30_000,
            Ordering::Relaxed,
        );
    }
    fn expired(&self) -> bool {
        self.origin.elapsed().as_millis() >= u128::from(self.until_ms.load(Ordering::Relaxed))
    }
}
unsafe extern "C" fn interrupted(opaque: *mut std::ffi::c_void) -> std::ffi::c_int {
    // SAFETY: The pointer is installed from the stable Box below and remains
    // alive until after avformat_close_input; the callback never mutates it.
    unsafe { i32::from((&*opaque.cast::<Deadline>()).expired()) }
}
fn open_input(path: &Path, deadline: &Deadline) -> Result<ff::format::context::Input> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())?;
    let mut options = ff::Dictionary::new();
    options.set("protocol_whitelist", "file");
    options.set("probesize", "33554432");
    options.set("analyzeduration", "10000000");
    // SAFETY: Context is fresh; options/path/callback storage remain alive
    // throughout open and probing. All failure paths close/free the context.
    // On success Input takes ownership, while Decoder retains the deadline.
    unsafe {
        let mut context = ffi::avformat_alloc_context();
        if context.is_null() {
            return Err("Cannot allocate video demuxer".into());
        }
        (*context).interrupt_callback = ffi::AVIOInterruptCB {
            callback: Some(interrupted),
            opaque: ptr::from_ref(deadline).cast_mut().cast(),
        };
        let mut opts = options.disown();
        let ret = ffi::avformat_open_input(&mut context, path.as_ptr(), ptr::null_mut(), &mut opts);
        let _remaining_options = ff::Dictionary::own(opts);
        if ret < 0 {
            if !context.is_null() {
                ffi::avformat_close_input(&mut context);
            }
            return Err(ff::Error::from(ret).into());
        }
        let ret = ffi::avformat_find_stream_info(context, ptr::null_mut());
        if ret < 0 {
            ffi::avformat_close_input(&mut context);
            return Err(ff::Error::from(ret).into());
        }
        Ok(ff::format::context::Input::wrap(context))
    }
}

/// Each decoder is exclusively accessed by its owner (the footage mutex).
/// Independent decoders share a device but never packet/frame state.
pub struct Decoder {
    input: ff::format::context::Input,
    codec: ff::decoder::Video,
    device: Arc<Device>,
    stream: usize,
    size: [u32; 2],
    format: PlaneFormat,
    hardware_pixel: Pixel,
    draining: bool,
    pub seeks: u64,
    // Declared after input: field drop order keeps callback storage alive.
    deadline: Box<Deadline>,
}
impl Decoder {
    pub fn open(
        path: &Path,
        kind: DeviceKind,
        codec_name: Option<&str>,
        size: [u32; 2],
        format: PlaneFormat,
    ) -> Result<Self> {
        if size.iter().any(|n| *n == 0 || *n > 8192 || n % 2 != 0) {
            return Err("Hardware decode requires even dimensions in 2..8192".into());
        }
        if !path.is_file() {
            return Err("Hardware decode requires a local file".into());
        }
        ff::init()?;
        let device = Device::shared(&kind)?;
        let deadline = Box::new(Deadline::new());
        let input = open_input(path, &deadline)?;
        let stream = input
            .streams()
            .find(|s| s.parameters().medium() == ff::media::Type::Video)
            .ok_or("No video stream")?;
        let index = stream.index();
        let mut context =
            ff::codec::context::Context::from_parameters(stream.parameters())?.decoder();
        context.set_packet_time_base(stream.time_base());
        // SAFETY: The codec context is uniquely borrowed and unopened. It owns
        // the new hardware reference and releases it even if codec open fails.
        unsafe {
            let reference = ffi::av_buffer_ref(device.reference);
            if reference.is_null() {
                return Err("Cannot retain hardware device".into());
            }
            (*context.as_mut_ptr()).hw_device_ctx = reference;
            (*context.as_mut_ptr()).get_format = Some(hardware_format);
            (*context.as_mut_ptr()).thread_count = 2;
        }
        let codec = match codec_name {
            Some(name) => context
                .open_as(ff::decoder::find_by_name(name).ok_or("Hardware codec unavailable")?)?
                .video()?,
            None => context.video()?,
        };
        Ok(Self {
            input,
            codec,
            device,
            stream: index,
            size,
            format,
            hardware_pixel: if matches!(kind, DeviceKind::Cuda) {
                Pixel::CUDA
            } else {
                Pixel::VAAPI
            },
            draining: false,
            seeks: 0,
            deadline,
        })
    }
    pub fn device_id(&self) -> u64 {
        self.device.id
    }
    /// Seek in the selected stream's exact time base, then decode from the
    /// preceding keyframe. Flushing preserves the shared device and codec.
    pub fn seek(&mut self, pts: i64) -> Result<()> {
        self.deadline.arm();
        // SAFETY: Exclusive input access, valid selected stream index. No frame
        // or stream borrow crosses this call. Timestamp units match that stream.
        let ret = unsafe {
            ffi::avformat_seek_file(
                self.input.as_mut_ptr(),
                self.stream as i32,
                i64::MIN,
                pts,
                pts,
                0,
            )
        };
        if ret < 0 {
            return Err(ff::Error::from(ret).into());
        }
        self.codec.flush();
        self.draining = false;
        self.seeks += 1;
        Ok(())
    }
    /// Decodes on the device, discarding preroll there. Only the exact requested
    /// presentation timestamp is returned; a missing frame is never substituted.
    pub fn frame_at(&mut self, pts: i64) -> Result<HardwareFrame> {
        self.deadline.arm();
        let started = Instant::now();
        loop {
            if started.elapsed() > Duration::from_secs(30) {
                return Err("Hardware decode exceeded 30 seconds".into());
            }
            let mut frame = ff::frame::Video::empty();
            match self.codec.receive_frame(&mut frame) {
                Ok(()) => {
                    if frame.format() != self.hardware_pixel
                        || [frame.width(), frame.height()] != self.size
                    {
                        return Err("Hardware decoder changed format or dimensions".into());
                    }
                    let actual = frame
                        .timestamp()
                        .or_else(|| frame.pts())
                        .ok_or("Decoded video frame has no timestamp")?;
                    if actual < pts {
                        continue;
                    }
                    if actual != pts {
                        return Err(format!(
                            "Hardware decoder skipped requested timestamp {pts} (returned {actual})"
                        )
                        .into());
                    }
                    return Ok(HardwareFrame {
                        frame,
                        format: self.format,
                        _device: self.device.clone(),
                    });
                }
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => {}
                Err(e) => return Err(e.into()),
            }
            if self.draining {
                return Err("Hardware decoder drained before requested frame".into());
            }
            loop {
                if started.elapsed() > Duration::from_secs(30) {
                    return Err("Hardware packet read exceeded 30 seconds".into());
                }
                let mut packet = ff::Packet::empty();
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() == self.stream => {
                        self.codec.send_packet(&packet)?;
                        break;
                    }
                    Ok(()) => {}
                    Err(ff::Error::Eof) => {
                        self.codec.send_eof()?;
                        self.draining = true;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
}
