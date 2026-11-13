//! Timestamp-indexed FFmpeg footage. Rust owns bounds, caching and source identity.
mod colour;
mod hardware;
pub(crate) mod process;
mod session;
use crate::Result;
use hardware::Backend;
pub use hardware::DecodePolicy;
use serde_json::Value;
use session::{Decoded, Session};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use veylvik_document::Time;
use veylvik_render::{FrameSource, Image, SourceInfo};
use veylvik_render::{GpuFrame, Renderer, VideoEncoding, VideoFormat};
struct State {
    cache: VecDeque<(usize, Arc<Image>)>,
    decoder: Option<Session>,
    backend: Backend,
    raw: VecDeque<(usize, Decoded)>,
    /// Last converted frame: index, requested size (None for full size), frame.
    gpu: Option<(usize, Option<[u32; 2]>, Arc<GpuFrame>)>,
    fallback: String,
    gpu_conversions: u64,
    cpu_realizations: u64,
    host_staging: bool,
    staged_frames: u64,
    direct_gpu_transfers: u64,
    interop_fallback: String,
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecodeDiagnostics {
    pub backend: String,
    pub hardware: bool,
    pub host_staging: bool,
    pub staged_frames: u64,
    pub direct_gpu_transfers: u64,
    pub interop_fallback: String,
    pub in_process: bool,
    pub device_context_id: Option<u64>,
    pub seeks: u64,
    pub fallback_reason: String,
    pub gpu_conversions: u64,
    pub cpu_realizations: u64,
}
pub struct Video {
    path: PathBuf,
    stamp: (u64, std::time::SystemTime),
    timestamps: Vec<i64>,
    time_base: Time,
    info: SourceInfo,
    filter: String,
    encoding: VideoEncoding,
    hardware_format: VideoFormat,
    policy: DecodePolicy,
    colour: colour::Transform,
    budget: u64,
    state: Mutex<State>,
    report: Mutex<DecodeDiagnostics>,
}
pub fn is_video(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["mp4", "mov", "m4v", "mkv", "webm", "avi", "mpg", "mpeg"]
            .iter()
            .any(|v| e.eq_ignore_ascii_case(v))
    })
}
fn rational(text: &str) -> Result<Time> {
    let (a, b) = text.split_once('/').ok_or("Invalid video time base")?;
    let time = Time::new(a.parse()?, b.parse()?)?;
    if time.ticks <= 0 || time.ticks > 1_000_000 || time.rate > 1_000_000_000 {
        return Err("Video time base must be positive".into());
    }
    Ok(time)
}
fn stamp(path: &Path) -> Result<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err("Video source must be a local regular file".into());
    }
    Ok((meta.len(), meta.modified()?))
}
fn scaled(ticks: i64, base: Time) -> Result<Time> {
    let ticks = ticks
        .checked_mul(base.ticks)
        .ok_or("Video timestamp overflow")?;
    Ok(Time::new(ticks, base.rate)?)
}
impl Video {
    pub fn diagnostics(&self) -> Result<DecodeDiagnostics> {
        Ok(self
            .report
            .lock()
            .map_err(|_| "Video diagnostics unavailable")?
            .clone())
    }
    fn publish(&self, state: &State) {
        if let Ok(mut report) = self.report.lock() {
            *report = DecodeDiagnostics {
                backend: state.backend.label().into(),
                hardware: state.backend.hardware(),
                host_staging: state.host_staging,
                staged_frames: state.staged_frames,
                direct_gpu_transfers: state.direct_gpu_transfers,
                interop_fallback: state.interop_fallback.clone(),
                in_process: state
                    .decoder
                    .as_ref()
                    .is_some_and(|d| d.device_id().is_some()),
                device_context_id: state.decoder.as_ref().and_then(Session::device_id),
                seeks: state.decoder.as_ref().map_or(0, Session::seeks),
                fallback_reason: state.fallback.clone(),
                gpu_conversions: state.gpu_conversions,
                cpu_realizations: state.cpu_realizations,
            };
        }
    }
    pub fn open(path: &Path, budget: u64) -> Result<Self> {
        Self::open_with_policy(path, budget, DecodePolicy::Auto)
    }
    pub fn open_with_policy(path: &Path, budget: u64, policy: DecodePolicy) -> Result<Self> {
        let path = path.canonicalize()?;
        let original = stamp(&path)?;
        let bytes=process::capture(Command::new("ffprobe").args(["-v","error","-protocol_whitelist","file,pipe","-select_streams","v:0","-show_streams","-show_packets","-show_entries","stream=codec_name,width,height,time_base,avg_frame_rate,pix_fmt,color_range,color_space,color_transfer,color_primaries,sample_aspect_ratio,field_order,chroma_location:stream_side_data=rotation:stream_tags=alpha_mode:packet=pts,duration,flags","-of","json"]).arg(&path),32*1024*1024)?;
        let data: Value = serde_json::from_slice(&bytes)?;
        let stream = data["streams"]
            .as_array()
            .and_then(|s| s.first())
            .ok_or("No video stream found")?;
        if stream["tags"]["alpha_mode"].as_str() == Some("1") {
            return Err("Alpha WebM needs an alpha-aware codec path; use a PNG/EXR sequence or RGBA FFV1 source for now".into());
        }
        let width = u32::try_from(stream["width"].as_u64().ok_or("Missing video width")?)?;
        let height = u32::try_from(stream["height"].as_u64().ok_or("Missing video height")?)?;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Video dimensions must be in 1..8192".into());
        }
        if !matches!(
            stream["sample_aspect_ratio"].as_str(),
            None | Some("N/A" | "0:1" | "1:1")
        ) {
            return Err(
                "Anamorphic video is not supported yet; convert to square pixels first".into(),
            );
        }
        if !matches!(
            stream["field_order"].as_str(),
            None | Some("unknown" | "progressive")
        ) {
            return Err("Interlaced video needs deinterlacing before import".into());
        }
        let time_base = rational(
            stream["time_base"]
                .as_str()
                .ok_or("Missing video time base")?,
        )?;
        let rate = stream["avg_frame_rate"]
            .as_str()
            .and_then(|s| rational(s).ok());
        let mut frames = data["packets"]
            .as_array()
            .ok_or("No video packets found")?
            .iter()
            .filter(|p| !p["flags"].as_str().unwrap_or("").contains('D'))
            .collect::<Vec<_>>();
        frames.sort_by_key(|p| p["pts"].as_i64().unwrap_or(i64::MIN));
        if frames.is_empty() || frames.len() > 250_000 {
            return Err("Video needs 1..250000 frames; split longer footage into shots".into());
        }
        let timestamps: Vec<i64> = frames
            .iter()
            .map(|f| {
                f["pts"].as_i64().ok_or(
                    "Video packet has no presentation timestamp; remux the source before import",
                )
            })
            .collect::<std::result::Result<_, _>>()?;
        if timestamps.windows(2).any(|p| p[0] >= p[1]) {
            return Err("Video needs strictly increasing presentation timestamps".into());
        }
        let first = timestamps[0];
        let last = *timestamps.last().unwrap();
        let tail = frames.last().unwrap()["duration"]
            .as_i64()
            .filter(|d| *d > 0)
            .or_else(|| timestamps.windows(2).last().map(|p| p[1] - p[0]))
            .unwrap_or(1);
        let duration = scaled(
            last.checked_sub(first)
                .and_then(|v| v.checked_add(tail))
                .ok_or("Video duration overflow")?,
            time_base,
        )?;
        let rotation = stream["side_data_list"]
            .as_array()
            .and_then(|s| s.iter().find_map(|v| v["rotation"].as_i64()))
            .unwrap_or(0)
            .rem_euclid(360);
        let source_size = [width, height];
        let (width, height, rotate) = match rotation {
            0 => (width, height, ""),
            90 => (height, width, ",transpose=cclock"),
            180 => (width, height, ",hflip,vflip"),
            270 => (height, width, ",transpose=clock"),
            _ => {
                return Err(
                    "Unsupported video rotation; flatten the rotation before import".into(),
                );
            }
        };
        let frame_bytes = u64::from(width) * u64::from(height) * 16;
        if frame_bytes > budget.min(crate::MAX_IMAGE_BYTES) {
            return Err("Video frame exceeds the available source memory budget".into());
        }
        let pix = stream["pix_fmt"].as_str().unwrap_or("");
        let rgb = pix.starts_with("rgb") || pix.starts_with("bgr") || pix.starts_with("gbr");
        let known = |field: &str| {
            stream[field]
                .as_str()
                .filter(|s| !matches!(*s, "unknown" | "unspecified"))
        };
        let assumed = known("color_primaries").is_none()
            || known("color_transfer").is_none()
            || (!rgb && known("color_space").is_none());
        let primaries = match known("color_primaries").unwrap_or("bt709") {
            "bt709" => 1,
            "bt470m" => 4,
            "bt470bg" => 5,
            "smpte170m" => 6,
            "smpte240m" => 7,
            "bt2020" => 9,
            _ => return Err("Unsupported video colour primaries".into()),
        };
        let transfer =
            match known("color_transfer").unwrap_or(if rgb { "iec61966-2-1" } else { "bt709" }) {
                "bt709" => 1,
                "gamma22" => 4,
                "gamma28" => 5,
                "smpte170m" => 6,
                "smpte240m" => 7,
                "linear" => 8,
                "iec61966-2-1" => 13,
                "bt2020-10" => 14,
                "bt2020-12" => 15,
                "smpte2084" => 16,
                "arib-std-b67" => 18,
                _ => return Err("Unsupported video transfer function".into()),
            };
        let matrix = if rgb {
            0
        } else {
            match known("color_space").unwrap_or(if height >= 720 { "bt709" } else { "smpte170m" })
            {
                "bt709" => 1,
                "fcc" => 4,
                "bt470bg" => 5,
                "smpte170m" => 6,
                "smpte240m" => 7,
                "bt2020nc" => 9,
                "bt2020c" => {
                    return Err(
                        "Constant-luminance BT.2020 video needs conversion before import".into(),
                    );
                }
                _ => return Err("Unsupported video colour matrix".into()),
            }
        };
        let range = if rgb || pix.starts_with("yuvj") || known("color_range") == Some("pc") {
            "full"
        } else {
            "limited"
        };
        let matrix_name = match matrix {
            0 => "bt709",
            1 => "bt709",
            4 => "fcc",
            5 | 6 => "bt601",
            7 => "smpte240m",
            9 | 10 => "bt2020",
            _ => unreachable!(),
        };
        // swscale performs only YUV matrix/range conversion here. Transfer and
        // primaries are explicit Rust transforms; no float-format negotiation.
        let filter = format!(
            "scale=in_color_matrix={matrix_name}:out_color_matrix={matrix_name}:in_range={range}:out_range=full,format=rgba64le{rotate}"
        );
        let colour = colour::Transform::new(primaries, transfer)?;
        let (kr, kb) = match matrix {
            1 | 0 => (0.2126, 0.0722),
            4 => (0.30, 0.11),
            5 | 6 => (0.299, 0.114),
            7 => (0.212, 0.087),
            9 => (0.2627, 0.0593),
            _ => unreachable!(),
        };
        let chroma_location = match stream["chroma_location"].as_str().unwrap_or("left") {
            "center" => [0.5, 0.5],
            "topleft" => [0.0, 0.0],
            "top" => [0.5, 0.0],
            "bottomleft" => [0.0, 1.0],
            "bottom" => [0.5, 1.0],
            _ => [0.0, 0.5],
        };
        let (hardware_format, candidates) = hardware::candidates(
            stream["codec_name"].as_str().unwrap_or(""),
            pix,
            source_size[0],
            source_size[1],
        );
        let encoding = VideoEncoding {
            width: source_size[0],
            height: source_size[1],
            format: hardware_format,
            rotation: rotation as u32,
            kr,
            kb,
            full_range: range == "full",
            chroma_location,
            primaries: colour.matrix,
        };
        let info = SourceInfo {
            width,
            height,
            duration: Some(duration),
            frame_rate: rate,
            frames: Some(timestamps.len() as u32),
            description: format!(
                "Video · {}{}",
                if assumed {
                    "colour tags incomplete; standard defaults assumed"
                } else {
                    "colour-managed to linear Rec.709"
                },
                if matches!(transfer, 16 | 18) {
                    " · HDR, 100 nit reference"
                } else {
                    ""
                }
            ),
        };
        if stamp(&path)? != original {
            return Err("Video changed during indexing; import it again".into());
        }
        let video = Self {
            path,
            stamp: original,
            timestamps,
            time_base,
            info,
            filter,
            encoding,
            hardware_format,
            policy,
            colour,
            budget: (frame_bytes * 2).min(budget),
            report: Mutex::new(DecodeDiagnostics {
                backend: "Opening".into(),
                hardware: false,
                host_staging: true,
                staged_frames: 0,
                direct_gpu_transfers: 0,
                interop_fallback: String::new(),
                in_process: false,
                device_context_id: None,
                seeks: 0,
                fallback_reason: String::new(),
                gpu_conversions: 0,
                cpu_realizations: 0,
            }),
            state: Mutex::new(State {
                cache: VecDeque::new(),
                decoder: None,
                backend: Backend::Software,
                raw: VecDeque::new(),
                gpu: None,
                fallback: String::new(),
                gpu_conversions: 0,
                cpu_realizations: 0,
                host_staging: true,
                staged_frames: 0,
                direct_gpu_transfers: 0,
                interop_fallback: String::new(),
            }),
        };
        // Fail at import for unavailable decoders/colour transforms, not at first playback.
        video.initialize(candidates)?;
        if stamp(&video.path)? != video.stamp {
            return Err("Video changed during initialization; import it again".into());
        }
        Ok(video)
    }
    fn index(&self, time: Time) -> Result<usize> {
        time.seconds()?;
        let first = i128::from(self.timestamps[0]);
        let upper = self.timestamps.partition_point(|&pts| {
            (i128::from(pts) - first) * i128::from(self.time_base.ticks) * i128::from(time.rate)
                <= i128::from(time.ticks) * i128::from(self.time_base.rate)
        });
        Ok(upper.saturating_sub(1))
    }
    fn decoder(&self, index: usize, backend: &Backend) -> Result<Session> {
        if backend.hardware() {
            return Session::hardware(
                &self.path,
                backend,
                [self.encoding.width, self.encoding.height],
                self.hardware_format,
                index,
                self.timestamps[index],
            );
        }
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-v",
            "error",
            "-nostdin",
            "-threads",
            "2",
            "-filter_threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-noautorotate",
        ]);
        if index > 0 {
            // Seek strictly between presentation timestamps so rounding cannot drop
            // the requested frame. Accurate input seeking discards earlier frames.
            let midpoint = (self.timestamps[index - 1] as f64 + self.timestamps[index] as f64)
                * 0.5
                * self.time_base.seconds()?;
            cmd.args(["-seek_timestamp", "1", "-ss", &format!("{midpoint:.12}")]);
        }
        let filter = &self.filter;
        let format = "rgba64le";
        let bytes = self.info.width as usize * self.info.height as usize * 8;
        cmd.arg("-i").arg(&self.path).args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            filter,
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            format,
            "-f",
            "rawvideo",
            "pipe:1",
        ]);
        process::Decoder::new(&mut cmd, bytes, index).map(Session::Software)
    }
    fn initialize(&self, candidates: Vec<Backend>) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| "Video decoder unavailable")?;
        let mut failures = Vec::new();
        if self.policy != DecodePolicy::Software {
            for backend in candidates {
                let attempt = self
                    .decoder(0, &backend)
                    .and_then(|mut d| d.read(&self.timestamps).map(|bytes| (d, bytes)));
                match attempt {
                    Ok((decoder, bytes)) => {
                        state.backend = backend;
                        state.decoder = Some(decoder);
                        state.raw.push_back((0, bytes));
                        self.publish(&state);
                        return Ok(());
                    }
                    Err(e) => failures.push(format!("{}: {e}", backend.label())),
                }
            }
        }
        if self.policy == DecodePolicy::HardwareRequired {
            return Err(format!("No usable hardware decoder. {}", failures.join("; ")).into());
        }
        state.fallback = if self.policy == DecodePolicy::Software {
            "software explicitly selected".into()
        } else if failures.is_empty() {
            "codec or pixel format has no supported hardware path".into()
        } else {
            "hardware initialization failed".into()
        };
        let mut decoder = self.decoder(0, &Backend::Software)?;
        let bytes = decoder.read(&self.timestamps)?;
        state.decoder = Some(decoder);
        state.raw.push_back((0, bytes));
        self.publish(&state);
        Ok(())
    }
    fn decoded(&self, state: &mut State, index: usize) -> Result<(Decoded, VideoEncoding)> {
        if stamp(&self.path)? != self.stamp {
            return Err("Video changed on disk; reload sources before continuing".into());
        }
        let bytes = if let Some((_, bytes)) = state.raw.iter().find(|(n, _)| *n == index) {
            bytes.clone()
        } else {
            let attempt = (|| {
                // Native sessions discard skipped frames on the GPU and seek
                // without reopening. Software pipes drain small preview gaps.
                let positioned = if let Some(d) = state.decoder.as_mut() {
                    d.position(index, &self.timestamps)?
                } else {
                    false
                };
                if !positioned
                    && state
                        .decoder
                        .as_ref()
                        .is_none_or(|d| d.next() > index || index - d.next() > 8)
                {
                    state.decoder = None;
                    state.decoder = Some(self.decoder(index, &state.backend)?);
                }
                let decoder = state.decoder.as_mut().unwrap();
                while decoder.next() < index {
                    decoder.read(&self.timestamps)?;
                }
                decoder.read(&self.timestamps)
            })();
            let bytes = match attempt {
                Ok(bytes) => bytes,
                Err(e) => {
                    state.decoder = None;
                    if !state.backend.hardware() || self.policy == DecodePolicy::HardwareRequired {
                        return Err(e);
                    }
                    state.backend = Backend::Software;
                    state.fallback = "hardware decoder failed during playback".into();
                    self.publish(state);
                    state.raw.clear();
                    state.cache.clear();
                    state.gpu = None;
                    let mut decoder = self.decoder(index, &state.backend)?;
                    let bytes = decoder.read(&self.timestamps)?;
                    state.decoder = Some(decoder);
                    bytes
                }
            };
            if stamp(&self.path)? != self.stamp {
                state.decoder = None;
                return Err("Video changed during decoding; reload sources".into());
            }
            while state.raw.len() >= 2
                || state.raw.iter().map(|(_, b)| b.byte_len()).sum::<u64>() + bytes.byte_len()
                    > self.budget
            {
                if state.raw.pop_front().is_none() {
                    break;
                }
            }
            state.raw.push_back((index, bytes.clone()));
            bytes
        };
        let mut encoding = self.encoding;
        if !state.backend.hardware() {
            encoding.format = VideoFormat::Rgba64;
            encoding.width = self.info.width;
            encoding.height = self.info.height;
            encoding.rotation = 0;
        }
        Ok((bytes, encoding))
    }
}
impl FrameSource for Video {
    fn byte_budget(&self) -> u64 {
        self.budget
    }
    fn info(&self) -> Option<SourceInfo> {
        let mut info = self.info.clone();
        // Inspector reads must never wait for a seek or a GPU fence.
        if let Ok(report) = self.report.try_lock() {
            info.description.push_str(&format!(
                " · {} · Vulkan colour{}",
                report.backend,
                if report.hardware {
                    if report.in_process {
                        if report.host_staging {
                            " · in-process · staged planes"
                        } else {
                            " · CUDA to Vulkan · GPU transfer"
                        }
                    } else {
                        " · staged planes"
                    }
                } else {
                    ""
                }
            ));
            if !report.interop_fallback.is_empty() {
                info.description
                    .push_str(&format!(" · {}", report.interop_fallback));
            }
            if !report.fallback_reason.is_empty() {
                info.description
                    .push_str(&format!(" ({})", report.fallback_reason));
            }
        }
        Some(info)
    }
    fn gpu_frame(
        &self,
        time: Time,
        renderer: &Renderer,
    ) -> veylvik_render::Result<Option<Arc<GpuFrame>>> {
        self.gpu_frame_sized(time, renderer, None)
    }
    fn gpu_frame_sized(
        &self,
        time: Time,
        renderer: &Renderer,
        target: Option<[u32; 2]>,
    ) -> veylvik_render::Result<Option<Arc<GpuFrame>>> {
        let work = || -> Result<Arc<GpuFrame>> {
            if stamp(&self.path)? != self.stamp {
                return Err("Video changed on disk; reload sources before continuing".into());
            }
            let index = self.index(time)?;
            let mut state = self.state.lock().map_err(|_| "Video decoder unavailable")?;
            if let Some((n, size, frame)) = &state.gpu
                && *n == index
                && *size == target
                && renderer.owns_frame(frame)
            {
                return Ok(frame.clone());
            }
            state.cache.clear();
            state.gpu = None;
            let (bytes, encoding) = self.decoded(&mut state, index)?;
            let direct = if let Decoded::Hardware(hardware) = &bytes {
                match hardware.cuda() {
                    Ok(Some(cuda)) => Some(renderer.import_cuda_video_sized(
                        &cuda,
                        encoding,
                        &self.colour.lut,
                        target,
                    )),
                    Ok(None) => None,
                    Err(e) => Some(Err(veylvik_render::Error::Message(e.to_string()))),
                }
            } else {
                None
            };
            let frame = match direct {
                Some(Ok(frame)) => {
                    state.host_staging = false;
                    state.direct_gpu_transfers += 1;
                    state.interop_fallback.clear();
                    frame
                }
                attempt => {
                    state.host_staging = true;
                    if let Some(Err(e)) = attempt {
                        state.interop_fallback = format!("GPU bridge unavailable: {e}");
                    }
                    let staged = bytes.stage()?;
                    if matches!(bytes, Decoded::Hardware(_)) {
                        state.staged_frames += 1;
                    }
                    renderer.import_video_sized(&staged, encoding, &self.colour.lut, target)?
                }
            };
            let frame = Arc::new(frame);
            state.gpu_conversions += 1;
            state.gpu = Some((index, target, frame.clone()));
            self.publish(&state);
            Ok(frame)
        };
        work()
            .map(Some)
            .map_err(|e| veylvik_render::Error::Message(e.to_string()))
    }
    fn frame(&self, time: Time) -> veylvik_render::Result<Arc<Image>> {
        let work = || -> Result<Arc<Image>> {
            if stamp(&self.path)? != self.stamp {
                return Err("Video changed on disk; reload sources before continuing".into());
            }
            let index = self.index(time)?;
            let mut state = self.state.lock().map_err(|_| "Video decoder unavailable")?;
            if let Some((_, image)) = state.cache.iter().find(|(n, _)| *n == index) {
                return Ok(image.clone());
            }
            state.gpu = None;
            let (bytes, encoding) = self.decoded(&mut state, index)?;
            state.cpu_realizations += 1;
            if matches!(bytes, Decoded::Hardware(_)) {
                state.staged_frames += 1;
            }
            let bytes = bytes.stage()?;
            let pixels = self.colour.decoded_pixels(&bytes, encoding);
            let image = Arc::new(Image::new(self.info.width, self.info.height, pixels)?);
            state.raw.clear();
            let size = image.byte_size();
            while (state.cache.len() as u64 + 1) * size > self.budget {
                state.cache.pop_front();
            }
            state.cache.push_back((index, image.clone()));
            self.publish(&state);
            Ok(image)
        };
        work().map_err(|e| veylvik_render::Error::Message(e.to_string()))
    }
}
