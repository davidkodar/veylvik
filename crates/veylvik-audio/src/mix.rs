//! Composition audio rendered once to interleaved stereo f32 by the export's FFmpeg mix, so
//! preview and export sound the same; length and bytes are bounded, and cancellation is honoured.
use crate::Result;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use veylvik_document::Composition;

/// Longest stretch mixed at once (about 230 MB at 48 kHz stereo).
pub const MAX_SECONDS: f64 = 600.0;

/// Interleaved stereo samples; sample frame 0 plays at composition time `start`.
pub struct Pcm {
    pub rate: u32,
    pub start: f64,
    pub samples: Vec<f32>,
}
impl Pcm {
    pub fn frames(&self) -> u64 {
        self.samples.len() as u64 / 2
    }
    pub fn end(&self) -> f64 {
        self.start + self.frames() as f64 / f64::from(self.rate)
    }
}

/// Mix every audible footage clip of `doc` within `[start, end)` composition seconds (at
/// most `MAX_SECONDS` from `start`) at `rate`. `None` when nothing in the window has sound.
/// `keep` is polled while FFmpeg runs; returning false cancels with `Ok(None)`.
pub fn mix(
    doc: &Composition,
    base: &Path,
    window: [f64; 2],
    rate: u32,
    keep: impl FnMut() -> bool,
) -> Result<Option<Pcm>> {
    mix_within(
        doc,
        base,
        &veylvik_document::access::Reach::Anywhere,
        window,
        rate,
        keep,
    )
}
/// As [`mix`], reading only the files `reach` allows (see `veylvik_document::access`).
pub fn mix_within(
    doc: &Composition,
    base: &Path,
    reach: &veylvik_document::access::Reach,
    [start, end]: [f64; 2],
    rate: u32,
    mut keep: impl FnMut() -> bool,
) -> Result<Option<Pcm>> {
    if !(8000..=192_000).contains(&rate) {
        return Err(format!("unsupported output rate {rate} Hz").into());
    }
    let end = end.min(start + MAX_SECONDS);
    // Also rejects NaN bounds.
    if end.partial_cmp(&start) != Some(std::cmp::Ordering::Greater) {
        return Ok(None);
    }
    let clips = veylvik_io::export_audio::clips_within(doc, base, reach, [start, end])
        .map_err(|e| e.to_string())?;
    if clips.is_empty() {
        return Ok(None);
    }
    let duration = end - start;
    // Input 0 stands in for the export's video; the shared graph reads clips from input 1.
    let mut command = Command::new("ffmpeg");
    command.args([
        "-nostdin",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo",
    ]);
    for clip in &clips {
        command.arg("-i").arg(&clip.path);
    }
    command
        .arg("-filter_complex")
        .arg(veylvik_io::export_audio::graph(&clips, None, duration))
        .args([
            "-map",
            "[aout]",
            "-ac",
            "2",
            "-ar",
            &rate.to_string(),
            "-f",
            "f32le",
        ])
        .args(["-t", &format!("{duration:.6}"), "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start FFmpeg to mix preview audio: {e}"))?;
    let limit = (duration * f64::from(rate) * 2.0 * 4.0) as u64 + 64 * 1024;
    let stdout = child.stdout.take().ok_or("FFmpeg output unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(limit).read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let deadline = Instant::now() + Duration::from_secs_f64((duration * 2.0).max(60.0));
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if !keep() || Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let bytes = reader.join().map_err(|_| "audio reader failed")??;
    let errors = errors.join().unwrap_or_default();
    match status {
        None if Instant::now() > deadline => Err("Mixing preview audio took too long".into()),
        None => Ok(None),
        Some(status) if !status.success() => {
            Err(format!("Could not mix preview audio: {}", errors.trim()).into())
        }
        Some(_) => {
            let samples = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            Ok(Some(Pcm {
                rate,
                start,
                samples,
            }))
        }
    }
}
