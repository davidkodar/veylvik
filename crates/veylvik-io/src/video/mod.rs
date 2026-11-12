//! Timestamp-indexed FFmpeg footage. Rust owns bounds, caching and source identity.
mod colour;
mod process;
use crate::Result;
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use veylvik_document::Time;
use veylvik_render::{FrameSource, Image, SourceInfo};
struct State {
    cache: VecDeque<(usize, Arc<Image>)>,
    decoder: Option<process::Decoder>,
}
pub struct Video {
    path: PathBuf,
    stamp: (u64, std::time::SystemTime),
    timestamps: Vec<i64>,
    time_base: Time,
    info: SourceInfo,
    filter: String,
    colour: colour::Transform,
    budget: u64,
    state: Mutex<State>,
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
    pub fn open(path: &Path, budget: u64) -> Result<Self> {
        let path = path.canonicalize()?;
        let original = stamp(&path)?;
        let bytes=process::capture(Command::new("ffprobe").args(["-v","error","-protocol_whitelist","file,pipe","-select_streams","v:0","-show_streams","-show_frames","-show_entries","stream=width,height,time_base,avg_frame_rate,pix_fmt,color_range,color_space,color_transfer,color_primaries,sample_aspect_ratio,field_order:stream_side_data=rotation:stream_tags=alpha_mode:frame=best_effort_timestamp,duration","-of","json"]).arg(&path),32*1024*1024)?;
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
        let frames = data["frames"].as_array().ok_or("No video frames found")?;
        if frames.is_empty() || frames.len() > 250_000 {
            return Err("Video needs 1..250000 frames; split longer footage into shots".into());
        }
        let timestamps: Vec<i64> = frames
            .iter()
            .map(|f| {
                f["best_effort_timestamp"]
                    .as_i64()
                    .ok_or("Video frame has no presentation timestamp")
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
            colour,
            budget: (frame_bytes * 2).min(budget),
            state: Mutex::new(State {
                cache: VecDeque::new(),
                decoder: None,
            }),
        };
        // Fail at import for unavailable decoders/colour transforms, not at first playback.
        video.frame(Time::new(0, 1)?)?;
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
    fn decoder(&self, index: usize) -> Result<process::Decoder> {
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
        cmd.arg("-i").arg(&self.path).args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &self.filter,
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgba64le",
            "-f",
            "rawvideo",
            "pipe:1",
        ]);
        process::Decoder::new(
            &mut cmd,
            self.info.width as usize * self.info.height as usize * 8,
            index,
        )
    }
}
impl FrameSource for Video {
    fn byte_budget(&self) -> u64 {
        self.budget
    }
    fn info(&self) -> Option<SourceInfo> {
        Some(self.info.clone())
    }
    fn frame(&self, time: Time) -> veylvik_render::Result<Arc<Image>> {
        let work = || -> Result<Arc<Image>> {
            if stamp(&self.path)? != self.stamp {
                return Err("Video changed on disk; reload sources before continuing".into());
            }
            let index = self.index(time)?;
            let mut state = self.state.lock().map_err(|_| "Video decoder unavailable")?;
            if let Some(i) = state.cache.iter().position(|(n, _)| *n == index) {
                let entry = state.cache.remove(i).unwrap();
                let image = entry.1.clone();
                state.cache.push_back(entry);
                return Ok(image);
            }
            let frame_bytes = u64::from(self.info.width) * u64::from(self.info.height) * 16;
            while state.cache.len() as u64 * frame_bytes + frame_bytes > self.budget {
                state.cache.pop_front();
            }
            if state.decoder.as_ref().is_none_or(|d| d.next != index) {
                state.decoder = None;
                state.decoder = Some(self.decoder(index)?);
            }
            let bytes = match state.decoder.as_mut().unwrap().read() {
                Ok(b) => b,
                Err(e) => {
                    state.decoder = None;
                    return Err(e);
                }
            };
            if stamp(&self.path)? != self.stamp {
                state.decoder = None;
                return Err("Video changed during decoding; reload sources".into());
            }
            let pixels = self.colour.pixels(&bytes);
            let image = Arc::new(Image::new(self.info.width, self.info.height, pixels)?);
            state.cache.push_back((index, image.clone()));
            Ok(image)
        };
        work().map_err(|e| veylvik_render::Error::Message(e.to_string()))
    }
}
