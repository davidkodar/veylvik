//! Audio for video export: the sound of every Footage node feeding the output, placed by its
//! timeline timing and trims, plus optional music, mixed and muxed by one bounded FFmpeg run.
//! The encoded video stream is copied, never re-encoded.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use veylvik_document::{Composition, NodeId, Operation};

/// Export audio choices. The default mixes footage sound into the video.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    /// Export without any audio track.
    #[serde(default)]
    pub mute: bool,
    /// An extra track, such as music, starting at the first exported frame.
    #[serde(default)]
    pub music: Option<PathBuf>,
    /// Level of the extra track in decibels (−60…+12).
    #[serde(default)]
    pub music_gain_db: f32,
    /// Directory relative footage assets resolve against; supplied by the host, never saved.
    #[serde(skip)]
    pub base: Option<PathBuf>,
}
/// One footage sound placed on the export timeline, in seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub path: PathBuf,
    /// Source span to play.
    pub from: f64,
    pub to: f64,
    /// Export time where it starts.
    pub at: f64,
    /// Level in decibels (0 keeps the file's own).
    pub gain_db: f32,
}
/// Whether `path` names an audio file by its extension.
pub fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        [
            "wav", "mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "aiff", "aif",
        ]
        .contains(&e.to_ascii_lowercase().as_str())
    })
}
/// Probed audio facts about a media file: whether it has sound, and its length.
pub struct Probe {
    pub has_audio: bool,
    pub duration: Option<f64>,
}
/// Places a footage node's sound inside the export window `[start, end)` (composition seconds).
/// `timing` is (timeline start, source in, optional source out); `duration` the file's length.
pub fn place(
    path: &Path,
    timing: (f64, f64, Option<f64>),
    duration: Option<f64>,
    [start, end]: [f64; 2],
) -> Option<Clip> {
    let (at, source_in, source_out) = timing;
    let source_end = match (source_out, duration) {
        (Some(out), Some(length)) => out.min(length),
        (Some(out), None) => out,
        (None, Some(length)) => length,
        (None, None) => return None,
    };
    let length = source_end - source_in;
    let (visible_from, visible_to) = (at.max(start), (at + length).min(end));
    (length > 0. && visible_to > visible_from + 1e-6).then(|| Clip {
        path: path.to_path_buf(),
        from: source_in + (visible_from - at),
        to: source_in + (visible_to - at),
        at: visible_from - start,
        gain_db: 0.0,
    })
}
/// Footage nodes that feed the composition output, in graph order, each counted once.
fn footage_nodes(doc: &Composition) -> Vec<NodeId> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![doc.output];
    let mut found = Vec::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = doc.nodes.get(&id) else {
            continue;
        };
        if matches!(node.operation, Operation::Footage(_)) {
            found.push(id);
        }
        pending.extend(node.inputs.values().copied());
    }
    found
}
pub fn probe(path: &Path) -> Result<Probe> {
    let bytes = crate::video::process::capture(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type:format=duration",
                "-of",
                "json",
            ])
            .arg(path),
        1024 * 1024,
    )?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(Probe {
        has_audio: value["streams"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["codec_type"] == "audio")),
        duration: value["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse().ok()),
    })
}
/// Every audible footage clip within the export window of `doc`.
pub fn clips(doc: &Composition, base: &Path, window: [f64; 2]) -> Result<Vec<Clip>> {
    let mut clips = Vec::new();
    for id in footage_nodes(doc) {
        let Operation::Footage(footage) = &doc.nodes[&id].operation else {
            continue;
        };
        let path = base.join(&footage.asset);
        let probe = probe(&path)?;
        if !probe.has_audio {
            continue;
        }
        let timing = (
            footage.timing.start.seconds()?,
            footage.timing.source_in.seconds()?,
            footage.timing.source_out.map(|t| t.seconds()).transpose()?,
        );
        clips.extend(place(&path, timing, probe.duration, window));
    }
    // Every Sound node plays, wherever it sits in the graph; in id order so the mix
    // is the same every time.
    for node in doc.nodes.values() {
        let Operation::Sound(sound) = &node.operation else {
            continue;
        };
        if node.bypass || sound.file.trim().is_empty() {
            continue;
        }
        let path = base.join(&sound.file);
        let probe = probe(&path)?;
        if !probe.has_audio {
            return Err(format!("{}: {} has no sound", node.label, path.display()).into());
        }
        let timing = (
            sound.timing.start.seconds()?,
            sound.timing.source_in.seconds()?,
            sound.timing.source_out.map(|t| t.seconds()).transpose()?,
        );
        if let Some(mut clip) = place(&path, timing, probe.duration, window) {
            clip.gain_db = sound.gain.at(sound.timing.start)?.clamp(-60.0, 12.0);
            clips.push(clip);
        }
    }
    Ok(clips)
}
/// FFmpeg filter graph: input 0 is the video, inputs 1.. the clips, then the music. The mix
/// starts from generated silence of exactly `duration` and follows it, so the track always
/// matches the video's length whatever the clips cover.
pub fn graph(clips: &[Clip], music_gain_db: Option<f32>, duration: f64) -> String {
    let mut parts = vec![format!(
        "anullsrc=r=48000:cl=stereo,atrim=end={duration:.6}[base]"
    )];
    let mut labels = String::from("[base]");
    for (i, clip) in clips.iter().enumerate() {
        let delay = (clip.at * 1000.).round() as u64;
        parts.push(format!(
            "[{}:a:0]atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS,aresample=48000,aformat=channel_layouts=stereo,volume={:.2}dB,adelay=delays={delay}:all=1[a{i}]",
            i + 1,
            clip.from,
            clip.to,
            clip.gain_db
        ));
        labels.push_str(&format!("[a{i}]"));
    }
    if let Some(gain) = music_gain_db {
        let n = clips.len();
        parts.push(format!(
            "[{}:a:0]atrim=end={duration:.6},asetpts=PTS-STARTPTS,aresample=48000,aformat=channel_layouts=stereo,volume={gain:.2}dB[a{n}]",
            n + 1
        ));
        labels.push_str(&format!("[a{n}]"));
    }
    let inputs = clips.len() + usize::from(music_gain_db.is_some()) + 1;
    // No automatic level reduction: footage keeps its recorded loudness when mixed.
    parts.push(format!(
        "{labels}amix=inputs={inputs}:normalize=0:duration=first[aout]"
    ));
    parts.join(";")
}
/// Mixes `clips` and optional music into `video`, writing `output`. Returns false when cancelled.
pub fn mux(
    video: &Path,
    output: &Path,
    clips: &[Clip],
    music: Option<(&Path, f32)>,
    duration: f64,
    mut keep: impl FnMut() -> bool,
) -> Result<bool> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(video);
    for clip in clips {
        command.arg("-i").arg(&clip.path);
    }
    if let Some((path, _)) = music {
        command.arg("-i").arg(path);
    }
    command
        .arg("-filter_complex")
        .arg(graph(clips, music.map(|(_, gain)| gain), duration))
        .args([
            "-map", "0:v", "-map", "[aout]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-ar",
            "48000",
        ])
        .args(["-t", &format!("{duration:.6}"), "-movflags", "+faststart"])
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start FFmpeg to mix audio: {e}"))?;
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
        }
        text.chars()
            .rev()
            .take(4000)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    });
    // Audio work is a small fraction of the export; allow generously, but never hang.
    let limit = Duration::from_secs_f64((duration * 4.).max(120.));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if !keep() || started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let errors = errors.join().unwrap_or_default();
    match status {
        None if started.elapsed() > limit => {
            Err("Mixing audio took too long; export without audio or shorten the range".into())
        }
        None => Ok(false),
        Some(status) if status.success() => Ok(true),
        Some(_) => Err(format!("Could not mix audio: {}", errors.trim()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clips_follow_timeline_placement_trims_and_the_export_window() {
        let p = Path::new("clip.mov");
        // Placed at 2 s, source 1..4 s: plays 2..5 s of the composition.
        assert_eq!(
            place(p, (2., 1., Some(4.)), Some(10.), [0., 10.]),
            Some(Clip {
                path: p.into(),
                from: 1.,
                to: 4.,
                at: 2.,
                gain_db: 0.0,
            })
        );
        // Exporting 3..4 s keeps only that second, starting at export time 0.
        assert_eq!(
            place(p, (2., 1., Some(4.)), Some(10.), [3., 4.]),
            Some(Clip {
                path: p.into(),
                from: 2.,
                to: 3.,
                at: 0.,
                gain_db: 0.0,
            })
        );
        // Untrimmed clips end with the file; a source out beyond it is clamped.
        assert_eq!(
            place(p, (0., 0., None), Some(6.), [0., 10.]).unwrap().to,
            6.
        );
        assert_eq!(
            place(p, (0., 0., Some(9.)), Some(6.), [0., 10.])
                .unwrap()
                .to,
            6.
        );
        // Outside the window, or unknown length without a trim: nothing to play.
        assert_eq!(place(p, (20., 0., Some(3.)), Some(6.), [0., 10.]), None);
        assert_eq!(place(p, (0., 0., None), None, [0., 10.]), None);
    }
    #[test]
    fn graph_delays_each_clip_mixes_without_normalizing_over_exact_length_silence() {
        let clips = [
            Clip {
                path: "a.mov".into(),
                from: 1.,
                to: 4.,
                at: 2.5,
                gain_db: 0.0,
            },
            Clip {
                path: "b.mov".into(),
                from: 0.,
                to: 3.,
                at: 0.,
                gain_db: 0.0,
            },
        ];
        let graph = graph(&clips, Some(-6.), 12.);
        assert!(graph.contains("[1:a:0]atrim=start=1.000000:end=4.000000"));
        assert!(graph.contains("adelay=delays=2500:all=1[a0]"));
        assert!(graph.contains("[3:a:0]atrim=end=12.000000"));
        assert!(graph.contains("volume=-6.00dB[a2]"));
        assert!(graph.starts_with("anullsrc=r=48000:cl=stereo,atrim=end=12.000000[base]"));
        assert!(
            graph.ends_with("[base][a0][a1][a2]amix=inputs=4:normalize=0:duration=first[aout]")
        );
    }
}
