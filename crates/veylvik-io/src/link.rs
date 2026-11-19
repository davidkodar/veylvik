//! A link to a host editor: a small file beside a project saying which clip of the host's timeline the project stands for and where its picture goes back. The host's own message is carried and sent, never understood, so no host's protocol is known here. Version 1.
use crate::{Result, video_export};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use veylvik_document::{Composition, Node, Operation, Time, markers::Marker};

/// The version of the link this build writes; a file of a later version is refused.
pub const PROTOCOL: u32 = 1;
/// Handles a host may ask for, in frames a side.
pub const MAX_HANDLES: u32 = 600;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_REPLY_BYTES: u64 = 64 * 1024;
const NOTIFY_WAIT: Duration = Duration::from_secs(3);

/// What the host sends when the picture has been updated: one line to its own socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notify {
    pub socket: PathBuf,
    /// Written as one line of JSON, exactly as the host gave it.
    pub message: serde_json::Value,
}
/// The link itself. A host writes one (without `before` and `after`) to ask for a linked
/// project; the stored one says what was made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub protocol: u32,
    /// The host's name, as shown to the user ("Klipp").
    pub host: String,
    /// The clip's name, as shown to the user.
    pub clip: String,
    /// The host's own name for the clip; kept, never read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_id: Option<String>,
    /// The clip's original media: the project reads it directly, no copy is made.
    pub media: PathBuf,
    /// The part of the media the clip uses, in the media's own time; out is exclusive.
    pub source_in: Time,
    pub source_out: Time,
    /// Extra frames wanted on each side, so the clip can still be trimmed in the host.
    #[serde(default)]
    pub handles: u32,
    /// The host timeline's frame rate and frame size: the project's own.
    pub frame_rate: Time,
    pub width: u32,
    pub height: u32,
    /// The frame of the clip to show first (0 is its first frame), if the host knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playhead: Option<u32>,
    /// Where the finished picture is written: a `.mov` with transparency.
    pub result: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<Notify>,
    /// Frames of handle the project really has before and after the clip (the media
    /// may not reach as far as was asked).
    #[serde(default)]
    pub before: u32,
    #[serde(default)]
    pub after: u32,
}
/// What a render back to the host did.
#[derive(Debug, Clone, Serialize)]
pub struct Sent {
    pub result: PathBuf,
    pub frames: u32,
    pub backend: String,
    /// Whether the host answered; a host that is not running is not an error.
    pub notified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Where a project's link is kept: beside it, under its own name.
pub fn path_for(project: &Path) -> PathBuf {
    let mut name = project.as_os_str().to_owned();
    name.push(".link");
    PathBuf::from(name)
}
fn read(path: &Path) -> Result<Link> {
    if std::fs::metadata(path)?.len() > MAX_FILE_BYTES {
        return Err(format!("{} is too large to be a link", path.display()).into());
    }
    let link: Link = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|e| format!("{} is not a link this build reads: {e}", path.display()))?;
    link.validate()?;
    Ok(link)
}
/// The link of `project`, if it has one.
pub fn load(project: &Path) -> Result<Option<Link>> {
    let path = path_for(project);
    if !path.is_file() {
        return Ok(None);
    }
    read(&path).map(Some)
}
fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}
fn time(numerator: i128, denominator: i128) -> Result<Time> {
    let d = gcd(numerator, denominator);
    Ok(Time::new(
        i64::try_from(numerator / d).map_err(|_| "time is out of range")?,
        u32::try_from(denominator / d).map_err(|_| "time is out of range")?,
    )?)
}
/// Whole frames of `rate` in `t`, rounded down (`up`: rounded up).
fn frames(t: Time, rate: Time, up: bool) -> i128 {
    let n = i128::from(t.ticks) * i128::from(rate.ticks);
    let d = i128::from(t.rate) * i128::from(rate.rate);
    if up {
        (n + d - 1).div_euclid(d)
    } else {
        n.div_euclid(d)
    }
}
impl Link {
    pub fn validate(&self) -> Result<()> {
        if self.protocol == 0 || self.protocol > PROTOCOL {
            return Err(format!(
                "this link is version {} and this Veylvik reads up to version {PROTOCOL}: update Veylvik",
                self.protocol
            )
            .into());
        }
        if self.host.trim().is_empty() || self.host.len() > 64 || self.clip.len() > 256 {
            return Err("a link needs the host's name (at most 64 characters) and a clip name of at most 256".into());
        }
        if !self.media.is_absolute() || !self.result.is_absolute() {
            return Err("a link's media and result must be full paths".into());
        }
        if !self
            .result
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mov"))
        {
            return Err("a link's result must be a .mov file".into());
        }
        if self.handles > MAX_HANDLES || self.before > MAX_HANDLES || self.after > MAX_HANDLES {
            return Err(format!("handles are at most {MAX_HANDLES} frames").into());
        }
        if self.source_in.seconds()? < 0.0 || !self.source_out.compare(self.source_in)?.is_gt() {
            return Err(
                "a link's source range must start at or after 0 and end after it starts".into(),
            );
        }
        if self.frame_rate.ticks <= 0 || self.frame_rate.rate == 0 {
            return Err("a link's frame rate must be positive".into());
        }
        Ok(())
    }
    /// Frames of the clip itself, without handles.
    pub fn clip_frames(&self) -> u32 {
        let n = frames(self.source_out, self.frame_rate, true)
            - frames(self.source_in, self.frame_rate, false);
        n.clamp(1, i128::from(veylvik_document::MAX_FRAME_COUNT)) as u32
    }
}
/// Make the project a host asked for and write it with its link: the clip's own media in
/// a composition of the host's size and rate, as long as the clip plus the handles the
/// media has, with a marker at each end of the clip. Refuses to replace a project.
pub fn create(request: &Path, project: &Path) -> Result<Link> {
    let mut link = read(request)?;
    if project.exists() || path_for(project).exists() {
        return Err(format!("{} already exists", project.display()).into());
    }
    let media = crate::footage::load(&link.media, 128 * 1024 * 1024)?;
    let info = media
        .info()
        .ok_or_else(|| format!("{} has no size", link.media.display()))?;
    let rate = link.frame_rate;
    link.before = i128::from(link.handles).min(frames(link.source_in, rate, false).max(0)) as u32;
    link.after = match info.duration {
        Some(length) => {
            let room = frames(length, rate, false) - frames(link.source_out, rate, true);
            i128::from(link.handles).min(room.max(0)) as u32
        }
        None => 0,
    };
    let clip = link.clip_frames();
    let mut composition = veylvik_document::blank(link.width, link.height, rate)?;
    composition.frame_count =
        (link.before + clip + link.after).min(veylvik_document::MAX_FRAME_COUNT);
    // The media time of the composition's first frame: the clip's start less its handle.
    let start = time(
        i128::from(link.source_in.ticks) * i128::from(rate.ticks)
            - i128::from(link.before) * i128::from(rate.rate) * i128::from(link.source_in.rate),
        i128::from(link.source_in.rate) * i128::from(rate.ticks),
    )?;
    let name = if link.clip.trim().is_empty() {
        link.media
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Clip")
            .to_owned()
    } else {
        link.clip.clone()
    };
    let node = Node::new(
        &name,
        Operation::Footage(veylvik_document::nodes::Footage {
            asset: link
                .media
                .to_str()
                .ok_or("the media's path is not UTF-8")?
                .to_owned(),
            timing: veylvik_document::media::Timing {
                start: Time::new(0, 1)?,
                source_in: start,
                source_out: None,
            },
            placement: veylvik_document::media::Placement::Fit,
            colour: Default::default(),
            layer: None,
        }),
        &[],
    );
    composition.nodes.clear();
    composition.output = node.id;
    composition.nodes.insert(node.id, node);
    if link.before > 0 {
        composition.markers.push(Marker {
            time: Time::at_frame(i64::from(link.before), rate)?,
            label: "Clip starts".into(),
        });
    }
    if link.after > 0 {
        composition.markers.push(Marker {
            time: Time::at_frame(i64::from(link.before + clip), rate)?,
            label: "Clip ends".into(),
        });
    }
    composition.validate()?;
    if let Some(parent) = project.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    composition.save(project)?;
    std::fs::write(path_for(project), serde_json::to_vec_pretty(&link)?)?;
    Ok(link)
}
/// Tell the host its picture changed: its own message, one line, and its one-line answer.
fn notify(notify: &Notify) -> std::io::Result<String> {
    let mut stream = std::os::unix::net::UnixStream::connect(&notify.socket)?;
    stream.set_write_timeout(Some(NOTIFY_WAIT))?;
    stream.set_read_timeout(Some(NOTIFY_WAIT))?;
    let mut line = notify.message.to_string();
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(std::io::Read::take(&stream, MAX_REPLY_BYTES)).read_line(&mut reply)?;
    Ok(reply.trim_end().to_owned())
}
/// Render a linked project's whole length, handles included, to the link's result (with
/// transparency and the composition's sound, taking the place of an earlier one only when
/// it is complete) and tell the host, at `socket` when given (a host's socket can change
/// between runs) or else at the one in the link. `progress` returning false cancels.
pub fn send(
    project: &Path,
    socket: Option<&Path>,
    progress: impl FnMut(video_export::Progress) -> bool,
) -> Result<Sent> {
    let link = load(project)?
        .ok_or_else(|| format!("{} is not linked to an editor", project.display()))?;
    let doc = Composition::load(project)?;
    let base = crate::document_base(project)?;
    let mut settings = video_export::Settings::for_document(&doc);
    settings.codec = video_export::Codec::ProRes4444;
    settings.quality = video_export::Quality::Master;
    settings.replace = true;
    settings.audio.base = Some(base.clone());
    if let Some(parent) = link.result.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let renderer = crate::runtime::renderer()?;
    let plan = veylvik_render::Plan::compile(&doc)?;
    let inputs = crate::load_inputs(&plan, &base, renderer.budget_bytes)?;
    let outcome =
        video_export::render(&renderer, &doc, &inputs, &link.result, &settings, progress)?;
    if outcome.cancelled {
        return Err("the render was cancelled; the earlier picture is untouched".into());
    }
    let (notified, reply, note) = match &link.notify {
        None => (false, None, None),
        Some(to) => match notify(&Notify {
            socket: socket.map_or_else(|| to.socket.clone(), Path::to_path_buf),
            message: to.message.clone(),
        }) {
            Ok(reply) => (true, Some(reply), None),
            Err(e) => (
                false,
                None,
                Some(format!(
                    "{} did not answer ({e}); it will show the new picture when it next reads the file",
                    link.host
                )),
            ),
        },
    };
    Ok(Sent {
        result: link.result,
        frames: outcome.frames,
        backend: outcome.backend,
        notified,
        reply,
        note,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn link() -> Link {
        Link {
            protocol: 1,
            host: "Klipp".into(),
            clip: "Shot".into(),
            clip_id: None,
            media: "/m/a.mov".into(),
            source_in: Time { ticks: 5, rate: 1 },
            source_out: Time { ticks: 15, rate: 2 },
            handles: 12,
            frame_rate: Time {
                ticks: 30000,
                rate: 1001,
            },
            width: 1920,
            height: 1080,
            playhead: None,
            result: "/p/comp.mov".into(),
            notify: None,
            before: 0,
            after: 0,
        }
    }
    #[test]
    fn a_link_is_checked_and_counts_its_frames_exactly() {
        let l = link();
        l.validate().unwrap();
        // 2.5 seconds at 30000/1001: frames 149 (floor of 149.85) to 225 (ceil of 224.77).
        assert_eq!(l.clip_frames(), 225 - 149);
        assert_eq!(
            path_for(Path::new("/p/shot.veylvik")),
            PathBuf::from("/p/shot.veylvik.link")
        );
        for bad in [
            Link {
                protocol: 2,
                ..link()
            },
            Link {
                protocol: 0,
                ..link()
            },
            Link {
                result: "/p/comp.mp4".into(),
                ..link()
            },
            Link {
                result: "comp.mov".into(),
                ..link()
            },
            Link {
                media: "a.mov".into(),
                ..link()
            },
            Link {
                source_out: Time { ticks: 5, rate: 1 },
                ..link()
            },
            Link {
                handles: MAX_HANDLES + 1,
                ..link()
            },
            Link {
                host: " ".into(),
                ..link()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        // Fields from a later minor addition are ignored, not refused.
        let mut json = serde_json::to_value(link()).unwrap();
        json["something_new"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Link>(json).is_ok());
    }
    #[test]
    fn the_host_is_sent_its_own_message_and_its_answer_comes_back() {
        let dir =
            std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        let socket = dir.join(format!("veylvik-link-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream.write_all(b"{\"ok\":true}\n").unwrap();
            line
        });
        let message = serde_json::json!({"id":1,"method":"anything the host likes"});
        let reply = notify(&Notify {
            socket: socket.clone(),
            message: message.clone(),
        })
        .unwrap();
        assert_eq!(reply, "{\"ok\":true}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&host.join().unwrap()).unwrap(),
            message
        );
        std::fs::remove_file(&socket).unwrap();
        // Nobody listening is an error for the caller to report, not a hang.
        assert!(notify(&Notify { socket, message }).is_err());
    }
}
