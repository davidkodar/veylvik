//! Files handed to the window, dropped on it or pasted after copying them in a file manager: a project opens, media is imported (or starts a composition from the start screen), one file at a time through the existing open and import paths. At most 32 wait.
use crate::{Leave, Studio, start::Chosen};
use eframe::egui;
use std::path::{Path, PathBuf};

/// Files that may wait to be taken in.
const WAITING: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Project,
    Media,
}
/// What a file is, by its name; None for anything Studio cannot take in this way.
pub(crate) fn kind(path: &Path) -> Option<Kind> {
    if path.file_name().is_some_and(|n| n == "sequence.json") {
        return Some(Kind::Media);
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "veylvik" => Some(Kind::Project),
        "png" | "exr" | "jpg" | "jpeg" | "tif" | "tiff" | "webp" | "svg" | "mp4" | "mov"
        | "m4v" | "mkv" | "webm" | "avi" | "mpg" | "mpeg" | "veyseq" | "wav" | "mp3" | "flac"
        | "ogg" | "oga" | "opus" | "m4a" | "aac" | "aiff" | "aif" => Some(Kind::Media),
        _ => None,
    }
}
fn decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
/// The files a pasted text names: every line an absolute path or a file:// address
/// (as file managers put copied files on the clipboard). Text that is anything else
/// names no files, so ordinary pasting is left alone.
pub(crate) fn listed(text: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        // Some file managers lead with the operation.
        if files.is_empty() && matches!(line, "copy" | "cut") {
            continue;
        }
        let path = match line.strip_prefix("file://") {
            // An address may carry a host name before the path.
            Some(rest) => match rest.find('/').and_then(|slash| decoded(&rest[slash..])) {
                Some(path) => PathBuf::from(path),
                None => return Vec::new(),
            },
            None if line.starts_with('/') => PathBuf::from(line),
            None => return Vec::new(),
        };
        if files.len() >= WAITING {
            break;
        }
        files.push(path);
    }
    files
}
impl Studio {
    /// Collect files dropped on the window, and files pasted while no text is being
    /// typed. Called early in the frame, before anything else reads pasted text.
    pub(crate) fn gather_files(&mut self, ctx: &egui::Context) {
        let mut found: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if !ctx.egui_wants_keyboard_input() {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    let egui::Event::Paste(text) = event else {
                        return true;
                    };
                    let files: Vec<PathBuf> =
                        listed(text).into_iter().filter(|p| p.is_file()).collect();
                    if files.is_empty() {
                        return true;
                    }
                    found.extend(files);
                    false
                });
            });
        }
        for path in found {
            if self.dropped.len() < WAITING {
                self.dropped.push_back(path);
            }
        }
    }
    /// Take in the next waiting file, when nothing else is in the way.
    pub(crate) fn take_next_file(&mut self, ctx: &egui::Context) {
        if self.dropped.is_empty()
            || self.files.busy()
            || self.authoring_dialog_open()
            || self.confirm.is_some()
            || self.media_import.is_some()
        {
            return;
        }
        let Some(path) = self.dropped.pop_front() else {
            return;
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match kind(&path) {
            Some(Kind::Project) => {
                // Opening a project replaces everything: the rest would land in it unasked.
                self.dropped.clear();
                self.start.chosen = Some(Chosen::Recent(path));
                self.leave(Leave::Start, ctx);
            }
            Some(Kind::Media) if self.start.open => {
                self.dropped.clear();
                self.start.chosen = Some(Chosen::FromMedia(path));
                self.leave(Leave::Start, ctx);
            }
            Some(Kind::Media) => {
                // The first file may set an empty composition's format (it asks); the
                // others are placed in the composition as it then is.
                let matching = (self.imported_files > 0).then_some(false);
                self.imported_files += 1;
                self.import_media_from(self.graph.insertion_position(ctx), false, Some(path), matching);
            }
            None => self.fail(format!(
                "{name} is not something Veylvik can open here. Use a project, a video, a PNG or EXR image, or an image sequence."
            )),
        }
        if self.dropped.is_empty() {
            self.imported_files = 0;
        }
    }
    /// While files are held over the window: say what dropping them will do.
    pub(crate) fn drop_hint(&self, ctx: &egui::Context) {
        let held: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if held.is_empty() {
            return;
        }
        let text = match held.first().and_then(|p| kind(p)) {
            Some(Kind::Project) => "Drop to open this project",
            Some(Kind::Media) if self.start.open => "Drop to start a composition from this",
            Some(Kind::Media) => "Drop to add this to the composition",
            None => "Veylvik cannot open this kind of file",
        };
        let area = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop-hint"),
        ));
        painter.rect_filled(area, 0.0, egui::Color32::from_black_alpha(170));
        painter.rect_stroke(
            area.shrink(18.0),
            14.0,
            egui::Stroke::new(2.0_f32, crate::design::accent(ctx)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(26.0),
            egui::Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn files_are_recognised_by_name_and_pasted_lists_are_read_strictly() {
        assert_eq!(kind(Path::new("/a/Title.VEYLVIK")), Some(Kind::Project));
        for name in [
            "clip.mp4",
            "plate.MOV",
            "frame.png",
            "pass.exr",
            "photo.jpg",
            "scan.TIFF",
            "shot.veyseq",
            "/out/sequence.json",
        ] {
            assert_eq!(kind(Path::new(name)), Some(Kind::Media), "{name}");
        }
        for name in [
            "notes.txt",
            "model.glb",
            "other.json",
            "raw.cr3",
            "noextension",
        ] {
            assert_eq!(kind(Path::new(name)), None, "{name}");
        }
        // Plain paths, file addresses with escapes and a host, and a leading operation.
        assert_eq!(
            listed("/home/me/a clip.mp4\n/home/me/b.png\n"),
            vec![
                PathBuf::from("/home/me/a clip.mp4"),
                PathBuf::from("/home/me/b.png")
            ]
        );
        assert_eq!(
            listed("copy\nfile:///home/me/My%20Film.veylvik\nfile://host/srv/x.exr"),
            vec![
                PathBuf::from("/home/me/My Film.veylvik"),
                PathBuf::from("/srv/x.exr")
            ]
        );
        // Anything that is not wholly a list of files is ordinary text.
        assert!(listed("hello").is_empty());
        assert!(listed("/home/me/a.png\nand some words").is_empty());
        assert!(listed("relative/path.png").is_empty());
        assert!(listed("file:///bad%zzescape.png").is_empty());
        assert!(listed("").is_empty());
        // Never more than the limit.
        let many: String = (0..100).map(|i| format!("/f/{i}.png\n")).collect();
        assert_eq!(listed(&many).len(), WAITING);
    }
}
