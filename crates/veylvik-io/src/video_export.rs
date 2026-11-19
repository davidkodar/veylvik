//! Hardware-first video export with GPU colour preparation and atomic publication.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};
use veylvik_document::{Composition, NodeId, Time};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    #[default]
    H264,
    Hevc10,
    /// ProRes 422 HQ in a MOV: an editing codec, for handing a shot to an editor that
    /// will cut and grade it further. Ten-bit 4:2:2, every frame a key frame, large files.
    #[serde(rename = "prores")]
    ProRes,
}
impl Codec {
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264 · MP4",
            Self::Hevc10 => "HEVC · 10-bit MP4",
            Self::ProRes => "ProRes 422 HQ · MOV",
        }
    }
    pub fn ten_bit(self) -> bool {
        self != Self::H264
    }
    /// An editing codec: software encoded, in a MOV.
    pub fn intermediate(self) -> bool {
        self == Self::ProRes
    }
    /// The file extension its videos carry.
    pub fn extension(self) -> &'static str {
        if self.intermediate() { "mov" } else { "mp4" }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Draft,
    #[default]
    High,
    Master,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Time,
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub codec: Codec,
    #[serde(default)]
    pub quality: Quality,
    #[serde(default)]
    pub allow_software: bool,
    #[serde(default)]
    pub matte: [f32; 3],
    #[serde(default)]
    pub audio: crate::export_audio::Audio,
    /// Replace a file already at the destination (the user agreed to in the save
    /// dialog). The finished video takes its place only when rendering succeeded.
    #[serde(default)]
    pub replace: bool,
    /// The composition's SDR display transform; filled in by [`render`], never saved.
    #[serde(skip)]
    pub display: veylvik_document::media::DisplayTransform,
}
impl Settings {
    fn look(&self) -> veylvik_render::Look {
        veylvik_render::Look {
            matte: self.matte,
            display: self.display,
        }
    }
    pub fn for_document(doc: &Composition) -> Self {
        Self {
            width: doc.width,
            height: doc.height,
            frame_rate: doc.frame_rate,
            start: 0,
            end: i64::from(doc.frame_count) - 1,
            codec: Codec::H264,
            quality: Quality::High,
            allow_software: false,
            matte: [0.; 3],
            audio: Default::default(),
            replace: false,
            display: doc.output_display(),
        }
    }
    pub fn times(&self, doc: &Composition) -> Result<Vec<Time>> {
        doc.validate()?;
        if self.width < 2
            || self.height < 2
            || self.width > 8192
            || self.height > 8192
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
        {
            return Err("Video dimensions must be even and within 2..8192".into());
        }
        if self.start < 0 || self.end < self.start || self.end >= i64::from(doc.frame_count) {
            return Err("Choose an inclusive frame range inside the composition".into());
        }
        let fps = self.frame_rate.seconds()?;
        if !(1.0..=240.0).contains(&fps)
            || self.frame_rate.ticks > 240_000
            || self.frame_rate.rate > 100_000
        {
            return Err("Video frame rate must be in 1..240 fps".into());
        }
        if self
            .matte
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.).contains(v))
        {
            return Err("Video matte must be linear RGB within 0..1".into());
        }
        let numerator = i128::from(self.end - self.start + 1)
            * i128::from(doc.frame_rate.rate)
            * i128::from(self.frame_rate.ticks);
        let denominator = i128::from(doc.frame_rate.ticks) * i128::from(self.frame_rate.rate);
        let count = (numerator + denominator - 1) / denominator;
        if !(1..=10_000).contains(&count) {
            return Err("Video export needs 1..10000 output frames".into());
        }
        let start = Time::at_frame(self.start, doc.frame_rate)?;
        (0..count as i64)
            .map(|n| Ok(start.checked_add(Time::at_frame(n, self.frame_rate)?)?))
            .collect()
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub completed: u32,
    pub total: u32,
    pub backend: String,
    pub phase: String,
}
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub path: Option<PathBuf>,
    pub cancelled: bool,
    pub backend: String,
    pub hardware: bool,
    pub frames: u32,
    pub host_staging: bool,
    pub direct_gpu_transfers: u32,
    pub staged_frames: u32,
    pub interop_fallback: Option<String>,
    pub experimental_interop: bool,
    /// Sources mixed into the audio track; empty for a silent video.
    pub audio: Vec<String>,
}
#[derive(Clone)]
enum Backend {
    Nvenc,
    Vaapi(PathBuf),
    Software,
}
impl Backend {
    fn device_kind(&self) -> Result<veylvik_decode::DeviceKind> {
        match self {
            Self::Nvenc => Ok(veylvik_decode::DeviceKind::Cuda),
            Self::Vaapi(node) => Ok(veylvik_decode::DeviceKind::Vaapi(node.clone())),
            Self::Software => Err("Software encoding has no direct GPU device".into()),
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Nvenc => "NVIDIA NVENC",
            Self::Vaapi(_) => "VA-API",
            Self::Software => "Software fallback",
        }
    }
    fn command(&self, s: &Settings, destination: Option<&Path>) -> Command {
        let mut c = Command::new("ffmpeg");
        c.args([
            "-hide_banner",
            "-v",
            "error",
            "-nostdin",
            "-threads",
            "2",
            "-filter_threads",
            "1",
        ]);
        if let Self::Vaapi(device) = self {
            c.arg("-vaapi_device").arg(device);
        }
        c.args([
            "-f",
            "rawvideo",
            "-pixel_format",
            if s.codec.ten_bit() { "p010le" } else { "nv12" },
            "-video_size",
            &format!("{}x{}", s.width, s.height),
            "-framerate",
            &format!("{}/{}", s.frame_rate.ticks, s.frame_rate.rate),
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
            "-i",
            "pipe:0",
            "-an",
            "-sn",
            "-dn",
        ]);
        let quality = match s.quality {
            Quality::Draft => 27,
            Quality::High => 20,
            Quality::Master => 15,
        }
        .to_string();
        match self {
            Self::Nvenc => {
                c.args([
                    "-c:v",
                    if s.codec.ten_bit() {
                        "hevc_nvenc"
                    } else {
                        "h264_nvenc"
                    },
                    "-preset",
                    if s.quality == Quality::Draft {
                        "p2"
                    } else {
                        "p5"
                    },
                    "-rc",
                    "vbr",
                    "-cq",
                    &quality,
                    "-b:v",
                    "0",
                ]);
            }
            Self::Vaapi(_) => {
                c.args([
                    "-vf",
                    if s.codec.ten_bit() {
                        "format=p010le,hwupload"
                    } else {
                        "format=nv12,hwupload"
                    },
                    "-c:v",
                    if s.codec.ten_bit() {
                        "hevc_vaapi"
                    } else {
                        "h264_vaapi"
                    },
                    "-qp",
                    &quality,
                ]);
            }
            Self::Software if s.codec.intermediate() => {
                c.args([
                    "-c:v",
                    "prores_ks",
                    "-profile:v",
                    "3",
                    "-vendor",
                    "apl0",
                    "-pix_fmt",
                    "yuv422p10le",
                ]);
            }
            Self::Software => {
                c.args([
                    "-c:v",
                    if s.codec.ten_bit() {
                        "libx265"
                    } else {
                        "libx264"
                    },
                    "-preset",
                    "medium",
                    "-crf",
                    &quality,
                    "-threads",
                    "2",
                    "-pix_fmt",
                    if s.codec.ten_bit() {
                        "yuv420p10le"
                    } else {
                        "yuv420p"
                    },
                ]);
                if s.codec.ten_bit() {
                    c.args(["-x265-params", "log-level=error:pools=2:frame-threads=2"]);
                }
            }
        }
        if s.codec.intermediate() {
        } else if s.codec.ten_bit() {
            c.args(["-profile:v", "main10"]);
        } else {
            c.args(["-profile:v", "high"]);
        }
        c.args([
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
            "-chroma_sample_location",
            "center",
        ]);
        if let Some(path) = destination {
            if s.codec.intermediate() {
                c.args(["-f", "mov"]);
            } else {
                c.args(["-movflags", "+faststart", "-f", "mp4"]);
                if s.codec.ten_bit() {
                    c.args(["-tag:v", "hvc1"]);
                }
            }
            c.arg(path);
        } else {
            c.args(["-frames:v", "1", "-f", "null", "-"]);
        }
        c
    }
}
struct Encoder {
    child: Child,
    sender: Option<mpsc::SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<std::result::Result<(), String>>>,
    errors: Option<JoinHandle<String>>,
}
impl Encoder {
    fn start(command: &mut Command) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Cannot start FFmpeg: {e}"))?;
        let mut input = child.stdin.take().unwrap();
        let mut errors = child.stderr.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(1);
        let writer = std::thread::spawn(move || {
            while let Ok(bytes) = receiver.recv() {
                input.write_all(&bytes).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        let errors = std::thread::spawn(move || {
            let mut tail = Vec::new();
            let mut buffer = [0; 2048];
            while let Ok(n) = errors.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buffer[..n]);
                if tail.len() > 8192 {
                    tail.drain(..tail.len() - 8192);
                }
            }
            String::from_utf8_lossy(&tail).trim().to_owned()
        });
        Ok(Self {
            child,
            sender: Some(sender),
            writer: Some(writer),
            errors: Some(errors),
        })
    }
    fn push(&mut self, mut bytes: Vec<u8>, keep: &mut impl FnMut() -> bool) -> Result<bool> {
        let started = Instant::now();
        loop {
            if !keep() {
                return Ok(false);
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(format!(
                    "Encoder stopped ({status}): {}",
                    self.errors
                        .take()
                        .and_then(|h| h.join().ok())
                        .unwrap_or_default()
                )
                .into());
            }
            match self.sender.as_ref().unwrap().try_send(bytes) {
                Ok(()) => return Ok(true),
                Err(mpsc::TrySendError::Full(b)) => bytes = b,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err("Encoder stopped accepting frames".into());
                }
            }
            if started.elapsed() > Duration::from_secs(30) {
                return Err("Encoder did not accept a frame within 30 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish(&mut self, keep: &mut impl FnMut() -> bool) -> Result<bool> {
        self.sender.take();
        let started = Instant::now();
        loop {
            if !keep() {
                return Ok(false);
            }
            if let Some(status) = self.child.try_wait()? {
                let written = self
                    .writer
                    .take()
                    .unwrap()
                    .join()
                    .map_err(|_| "Encoder input worker failed")?;
                let errors = self
                    .errors
                    .take()
                    .and_then(|h| h.join().ok())
                    .unwrap_or_default();
                if !status.success() {
                    return Err(format!("Encoder failed: {errors}").into());
                }
                written?;
                return Ok(true);
            }
            if started.elapsed() > Duration::from_secs(60) {
                return Err("Encoder finalization exceeded 60 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.sender.take();
        if let Some(h) = self.writer.take() {
            let _ = h.join();
        }
        if let Some(h) = self.errors.take() {
            let _ = h.join();
        }
    }
}
/// Put the finished file at its destination: never over an existing file, unless the
/// user agreed to replace it, when it takes the old file's place in one step.
fn publish(output: &Path, target: &Path, replace: bool) -> Result<()> {
    if replace {
        fs::rename(output, target)?;
    } else {
        fs::hard_link(output, target)?;
    }
    Ok(())
}
struct Temporary(PathBuf);
impl Temporary {
    fn new(parent: &Path) -> Result<Self> {
        let path = parent.join(format!(".veylvik-export-{}", NodeId::new_v4()));
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn backends(allow_software: bool) -> Vec<Backend> {
    let mut result = vec![Backend::Nvenc];
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        let mut paths = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name();
                if !name.to_string_lossy().starts_with("renderD") {
                    return None;
                }
                let vendor = fs::read_to_string(e.path().join("device/vendor")).ok()?;
                matches!(vendor.trim(), "0x8086" | "0x1002")
                    .then(|| PathBuf::from("/dev/dri").join(name))
            })
            .collect::<Vec<_>>();
        paths.sort();
        result.extend(paths.into_iter().map(Backend::Vaapi));
    }
    if allow_software {
        result.push(Backend::Software);
    }
    result
}
/// Snapshot export; target publication is no-replace and only happens after the
/// encoder successfully closes the MP4. Cancel removes private partial files.
pub fn render(
    renderer: &Renderer,
    doc: &Composition,
    inputs: &ImageInputs,
    path: &Path,
    settings: &Settings,
    mut progress: impl FnMut(Progress) -> bool,
) -> Result<Outcome> {
    let settings = &Settings {
        display: doc.output_display(),
        ..settings.clone()
    };
    let times = settings.times(doc)?;
    if !path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case(settings.codec.extension()))
    {
        return Err(format!(
            "A {} video must end in .{}",
            settings.codec.label(),
            settings.codec.extension()
        )
        .into());
    }
    match fs::symlink_metadata(path) {
        Err(_) => {}
        // Only an ordinary file is replaced, and only when the user agreed to it.
        Ok(existing) if settings.replace && existing.is_file() => {}
        Ok(_) => return Err("Video output already exists; choose a new filename".into()),
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| format!("The folder for {} cannot be used: {e}", path.display()))?;
    let target = parent.join(path.file_name().ok_or("Missing output filename")?);
    // Find out now, not after rendering, that the folder cannot be written to.
    let probe = parent.join(format!(".veylvik-write-test-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
        }
        Err(e) => {
            return Err(format!("Cannot write into {}: {e}", parent.display()).into());
        }
    }
    let total = times.len() as u32;
    let mut notify = |completed, backend: &str, phase: &str| {
        progress(Progress {
            completed,
            total,
            backend: backend.into(),
            phase: phase.into(),
        })
    };
    let cancelled = |backend: &str, hardware, frames| Outcome {
        path: None,
        cancelled: true,
        backend: backend.into(),
        hardware,
        frames,
        host_staging: true,
        direct_gpu_transfers: 0,
        staged_frames: frames,
        interop_fallback: None,
        experimental_interop: false,
        audio: Vec::new(),
    };
    if !notify(0, "", "Preparing") {
        return Ok(cancelled("", false, 0));
    }
    let soundtrack = Soundtrack::prepare(doc, settings, &times)?;
    let plan = Plan::compile(doc)?;
    let frame = renderer.render_gpu_export(&plan, RenderRequest::full(times[0]), inputs)?;
    // Probe the complete native encode/handoff/mux path before choosing it.
    // A failed probe can use the established staged path. Once real encoding
    // starts, any error aborts; it never silently restarts with another backend.
    let temporary = Temporary::new(&parent)?;
    let native = native_settings(settings);
    let trial = temporary.0.join("trial.mp4");
    let mut direct_failures = Vec::new();
    // An editing codec has no GPU encoder: it goes straight to the staged path.
    let direct = if settings.codec.intermediate() {
        Vec::new()
    } else {
        backends(false)
    };
    for backend in direct {
        if !notify(0, backend.label(), "Checking direct GPU encoder") {
            return Ok(cancelled(backend.label(), true, 0));
        }
        let direct = (|| -> Result<()> {
            let mut encoder =
                veylvik_decode::encode::Encoder::open_on(&trial, native, backend.device_kind()?)?;
            let target = prepare_native_frame(renderer, &encoder, &backend, &frame, settings)?;
            encoder.send(target)?;
            encoder.finish()
        })();
        match direct {
            Ok(()) => {
                fs::remove_file(&trial)?;
                return render_direct(
                    renderer,
                    &plan,
                    inputs,
                    &target,
                    settings,
                    &times,
                    frame,
                    temporary,
                    native,
                    &backend,
                    &soundtrack,
                    &mut notify,
                );
            }
            Err(e) => direct_failures.push(format!("{}: {e}", backend.label())),
        }
    }
    let interop_fallback = Some(direct_failures.join("; "));
    let cancelled = |backend: &str, hardware, frames| {
        let mut result = cancelled(backend, hardware, frames);
        result.interop_fallback = interop_fallback.clone();
        result
    };
    let first = renderer.encode_video_planes(
        &frame,
        settings.width,
        settings.height,
        settings.codec.ten_bit(),
        settings.look(),
    )?;
    drop(frame);
    let mut selected = None;
    let mut failures = Vec::new();
    let staged = if settings.codec.intermediate() {
        vec![Backend::Software]
    } else {
        backends(settings.allow_software)
    };
    for backend in staged {
        if !notify(0, backend.label(), "Checking encoder") {
            return Ok(cancelled(backend.label(), false, 0));
        }
        let result = (|| {
            let mut encoder = Encoder::start(&mut backend.command(settings, None))?;
            let mut keep = || notify(0, backend.label(), "Checking encoder");
            if !encoder.push(first.clone(), &mut keep)? {
                return Ok(false);
            }
            encoder.finish(&mut keep)
        })();
        match result {
            Ok(true) => {
                selected = Some(backend);
                break;
            }
            Ok(false) => return Ok(cancelled(backend.label(), false, 0)),
            Err(e) => failures.push(format!("{}: {e}", backend.label())),
        }
    }
    let backend = selected.ok_or_else(|| format!("No usable encoder. {}", failures.join("; ")))?;
    let hardware = !matches!(backend, Backend::Software);
    let output = temporary
        .0
        .join(format!("video.{}", settings.codec.extension()));
    let mut encoder = Encoder::start(&mut backend.command(settings, Some(&output)))?;
    let mut first = Some(first);
    for (n, time) in times.iter().enumerate() {
        if !notify(
            n as u32,
            backend.label(),
            "RAM-staged rendering and encoding",
        ) {
            return Ok(cancelled(backend.label(), hardware, n as u32));
        }
        let bytes = if let Some(first) = first.take() {
            first
        } else {
            let frame = renderer.render_gpu_export(&plan, RenderRequest::full(*time), inputs)?;
            renderer.encode_video_planes(
                &frame,
                settings.width,
                settings.height,
                settings.codec.ten_bit(),
                settings.look(),
            )?
        };
        if !encoder.push(bytes, &mut || notify(n as u32, backend.label(), "Encoding"))? {
            return Ok(cancelled(backend.label(), hardware, n as u32));
        }
    }
    if !encoder.finish(&mut || notify(total, backend.label(), "Finalizing MP4"))? {
        return Ok(cancelled(backend.label(), hardware, total));
    }
    drop(encoder);
    let Some(output) =
        soundtrack.apply(&output, || notify(total, backend.label(), "Mixing audio"))?
    else {
        return Ok(cancelled(backend.label(), hardware, total));
    };
    if !notify(total, backend.label(), "Publishing") {
        return Ok(cancelled(backend.label(), hardware, total));
    }
    fs::File::open(&output)?.sync_all()?;
    publish(&output, &target, settings.replace)?;
    fs::File::open(&parent)?.sync_all()?;
    Ok(Outcome {
        audio: soundtrack.sources(),
        path: Some(target),
        cancelled: false,
        backend: backend.label().into(),
        hardware,
        frames: total,
        host_staging: true,
        direct_gpu_transfers: 0,
        staged_frames: total,
        interop_fallback,
        experimental_interop: false,
    })
}

/// The export's audio, worked out before encoding so a bad music path fails early.
struct Soundtrack {
    clips: Vec<crate::export_audio::Clip>,
    music: Option<(PathBuf, f32)>,
    duration: f64,
}
impl Soundtrack {
    fn prepare(doc: &Composition, settings: &Settings, times: &[Time]) -> Result<Self> {
        let audio = &settings.audio;
        let start = times[0].seconds()?;
        let duration = times.len() as f64 / settings.frame_rate.seconds()?;
        if audio.mute {
            return Ok(Self {
                clips: Vec::new(),
                music: None,
                duration,
            });
        }
        if !(-60.0..=12.0).contains(&audio.music_gain_db) {
            return Err("Music level must be within -60..+12 dB".into());
        }
        let music = match &audio.music {
            Some(path) => {
                let probe = crate::export_audio::probe(path)
                    .map_err(|e| format!("Cannot read music file {}: {e}", path.display()))?;
                if !probe.has_audio {
                    return Err(format!("{} has no audio track", path.display()).into());
                }
                Some((path.clone(), audio.music_gain_db))
            }
            None => None,
        };
        let base = audio.base.clone().unwrap_or_else(|| PathBuf::from("."));
        let clips = crate::export_audio::clips(doc, &base, [start, start + duration])?;
        Ok(Self {
            clips,
            music,
            duration,
        })
    }
    fn sources(&self) -> Vec<String> {
        self.clips
            .iter()
            .map(|c| c.path.display().to_string())
            .chain(self.music.iter().map(|(p, _)| p.display().to_string()))
            .collect()
    }
    /// The file to publish: `video` itself when silent, otherwise a muxed copy beside it.
    /// `None` when cancelled.
    fn apply(&self, video: &Path, keep: impl FnMut() -> bool) -> Result<Option<PathBuf>> {
        if self.clips.is_empty() && self.music.is_none() {
            return Ok(Some(video.to_owned()));
        }
        // The same container as the picture.
        let mixed = video.with_file_name(format!(
            "with-audio.{}",
            video.extension().and_then(|e| e.to_str()).unwrap_or("mp4")
        ));
        let music = self.music.as_ref().map(|(p, g)| (p.as_path(), *g));
        Ok(
            crate::export_audio::mux(video, &mixed, &self.clips, music, self.duration, keep)?
                .then_some(mixed),
        )
    }
}

fn native_settings(s: &Settings) -> veylvik_decode::encode::Settings {
    veylvik_decode::encode::Settings {
        width: s.width,
        height: s.height,
        fps: [s.frame_rate.ticks as i32, s.frame_rate.rate as i32],
        ten_bit: s.codec.ten_bit(),
        fast: s.quality == Quality::Draft,
        quality: match s.quality {
            Quality::Draft => 27,
            Quality::High => 20,
            Quality::Master => 15,
        },
    }
}
#[allow(clippy::too_many_arguments)]
fn render_direct(
    renderer: &Renderer,
    plan: &Plan,
    inputs: &ImageInputs,
    target: &Path,
    settings: &Settings,
    times: &[Time],
    first: std::sync::Arc<veylvik_render::GpuFrame>,
    temporary: Temporary,
    native: veylvik_decode::encode::Settings,
    backend: &Backend,
    soundtrack: &Soundtrack,
    notify: &mut impl FnMut(u32, &str, &str) -> bool,
) -> Result<Outcome> {
    let total = times.len() as u32;
    let cancelled = |frames| Outcome {
        path: None,
        cancelled: true,
        backend: backend.label().into(),
        hardware: true,
        frames,
        host_staging: false,
        direct_gpu_transfers: frames,
        staged_frames: 0,
        interop_fallback: None,
        experimental_interop: matches!(backend, Backend::Vaapi(_)),
        audio: Vec::new(),
    };
    if !notify(0, backend.label(), "Preparing GPU export") {
        return Ok(cancelled(0));
    }
    let output = temporary.0.join("video.mp4");
    let mut encoder =
        veylvik_decode::encode::Encoder::open_on(&output, native, backend.device_kind()?)?;
    let mut first = Some(first);
    for (n, time) in times.iter().enumerate() {
        if !notify(
            n as u32,
            backend.label(),
            if matches!(backend, Backend::Vaapi(_)) {
                "GPU encoding · experimental DMA-BUF"
            } else {
                "GPU rendering and encoding"
            },
        ) {
            return Ok(cancelled(n as u32));
        }
        let source = match first.take() {
            Some(frame) => frame,
            None => renderer.render_gpu_export(plan, RenderRequest::full(*time), inputs)?,
        };
        let frame = prepare_native_frame(renderer, &encoder, backend, &source, settings)?;
        encoder.send(frame)?;
    }
    if !notify(total, backend.label(), "Finalizing MP4") {
        return Ok(cancelled(total));
    }
    encoder.finish()?;
    let Some(output) =
        soundtrack.apply(&output, || notify(total, backend.label(), "Mixing audio"))?
    else {
        return Ok(cancelled(total));
    };
    if !notify(total, backend.label(), "Publishing") {
        return Ok(cancelled(total));
    }
    fs::File::open(&output)?.sync_all()?;
    publish(&output, target, settings.replace)?;
    fs::File::open(target.parent().ok_or("Missing output parent")?)?.sync_all()?;
    Ok(Outcome {
        audio: soundtrack.sources(),
        path: Some(target.to_owned()),
        cancelled: false,
        backend: backend.label().into(),
        hardware: true,
        frames: total,
        host_staging: false,
        direct_gpu_transfers: total,
        staged_frames: 0,
        interop_fallback: None,
        experimental_interop: matches!(backend, Backend::Vaapi(_)),
    })
}

fn prepare_native_frame(
    renderer: &Renderer,
    encoder: &veylvik_decode::encode::Encoder,
    backend: &Backend,
    source: &veylvik_render::GpuFrame,
    settings: &Settings,
) -> Result<veylvik_decode::encode::Frame> {
    match backend {
        Backend::Nvenc => {
            let mut frame = encoder.frame()?;
            frame.fill(|destination| {
                Ok(renderer.encode_video_cuda(source, settings.look(), destination)?)
            })?;
            Ok(frame)
        }
        Backend::Vaapi(node) => encoder.map_dma(renderer.encode_video_dma(
            source,
            settings.width,
            settings.height,
            settings.codec.ten_bit(),
            settings.look(),
            node,
        )?),
        Backend::Software => Err("No direct GPU frame path for software encoder".into()),
    }
}
