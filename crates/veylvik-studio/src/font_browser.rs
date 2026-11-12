//! Background font discovery and a portable, user-owned font library.
use eframe::egui;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
};
use veylvik_document::TextFont;
#[derive(Clone, Debug)]
pub struct Face {
    pub path: PathBuf,
    pub index: u32,
    pub label: String,
    added: bool,
}
#[derive(Clone)]
pub enum Selection {
    Face(Face),
    File,
    Add,
}
#[derive(Clone, Copy, Default, PartialEq)]
enum Source {
    #[default]
    All,
    Bundled,
    System,
    Added,
}
#[derive(Default)]
pub struct Browser {
    faces: Vec<Face>,
    query: String,
    source: Source,
    pending: Option<mpsc::Receiver<(Vec<Face>, String)>>,
    notice: String,
    refresh_pending: bool,
}
impl Browser {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut browser = Self::default();
        browser.refresh(ctx);
        browser
    }
    pub fn refresh(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            self.refresh_pending = true;
            return;
        }
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("veylvik-fonts".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(scan).unwrap_or_else(|_| {
                    (
                        vec![],
                        "Font discovery failed; bundled fonts are still available.".into(),
                    )
                });
                let _ = send.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => self.pending = Some(receive),
            Err(e) => self.notice = e.to_string(),
        }
    }
    pub fn added(&mut self, ctx: &egui::Context) {
        self.source = Source::Added;
        self.query.clear();
        self.refresh(ctx);
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok((faces, notice)) => {
                    self.faces = faces;
                    self.notice = notice;
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.notice = "Font discovery worker stopped. Refresh to retry.".into();
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none() && self.refresh_pending {
            self.refresh_pending = false;
            self.refresh(ctx);
        }
    }
    pub fn choose(&mut self, ui: &mut egui::Ui, font: &mut TextFont) -> (bool, Option<Selection>) {
        let mut changed = false;
        let mut selected = None;
        egui::ComboBox::from_id_salt("text-font-browser").width(ui.available_width().min(300.0)).height(400.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).selected_text(font.label()).show_ui(ui,|ui| {
            ui.set_min_width(300.0);
            ui.horizontal(|ui| { ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Search family or style…").desired_width(240.0)); if ui.small_button("Clear").clicked() { self.query.clear(); } });
            ui.horizontal(|ui| { for (value,label) in [(Source::All,"All"),(Source::Bundled,"Bundled"),(Source::System,"System"),(Source::Added,"Added")] { ui.selectable_value(&mut self.source,value,label); } });
            let query=self.query.to_lowercase();
            if matches!(self.source,Source::All|Source::Bundled) {
                ui.weak("Bundled with Veylvik");
                for value in [TextFont::Display,TextFont::Sans {},TextFont::Italic {},TextFont::BoldItalic {},TextFont::Mono {}] {
                    let label=value.label().to_owned();
                    if !query.split_whitespace().all(|word| label.to_lowercase().contains(word)) { continue; }
                    if ui.selectable_label(*font==value,label).clicked() { changed |= *font != value; *font=value; ui.close(); }
                }
            }
            let matches:Vec<_>=self.faces.iter().filter(|face| match self.source {Source::Bundled=>false,Source::Added=>face.added,Source::System=>!face.added,Source::All=>true}).filter(|face|query.split_whitespace().all(|word|face.label.to_lowercase().contains(word))).collect();
            if !matches!(self.source,Source::Bundled) {
                ui.weak(format!("{} font faces",matches.len()));
                egui::ScrollArea::vertical().id_salt("font-faces").max_height(if self.source==Source::All {140.0} else {220.0}).show_rows(ui,ui.text_style_height(&egui::TextStyle::Body)+6.0,matches.len(),|ui,rows| {
                    for index in rows {
                        let face=matches[index];
                        if ui.selectable_label(font.label()==face.label,format!("{}{}",face.label,if face.added {" · Added"}else{""})).on_hover_text(face.path.display().to_string()).clicked() { selected=Some(Selection::Face(face.clone())); ui.close(); }
                    }
                });
                if matches.is_empty() && self.pending.is_none() { ui.weak("No matching fonts. Try another source or add a font file."); }
            }
            if self.pending.is_some() { ui.horizontal(|ui| {ui.spinner();ui.weak("Reading installed fonts…");}); }
            if !self.notice.is_empty() { ui.add(egui::Label::new(&self.notice).wrap()); }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Add font…").on_hover_text("Keep a TTF, OTF or collection in Veylvik's font library for all projects.").clicked() { selected=Some(Selection::Add); ui.close(); }
                if ui.button("Refresh fonts").clicked() { self.refresh(ui.ctx()); }
                if ui.button("Use file once…").on_hover_text("Embed a font in this project without adding it to the library.").clicked() { selected=Some(Selection::File); ui.close(); }
            });
        });
        (changed, selected)
    }
}
fn directory() -> Result<PathBuf, String> {
    Ok(veylvik_io::presets::directory()
        .map_err(|e| e.to_string())?
        .with_file_name("fonts"))
}
pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(veylvik_document::text::MAX_FONT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > veylvik_document::text::MAX_FONT_BYTES {
        return Err("Font must contain 1..8 MiB".into());
    }
    Ok(bytes)
}
fn metadata(bytes: &[u8], index: u32) -> Result<String, String> {
    let face = ttf_parser::Face::parse(bytes, index)
        .map_err(|_| "Invalid font or collection face".to_string())?;
    let name = |ids: &[u16]| {
        ids.iter().find_map(|id| {
            face.names()
                .into_iter()
                .filter(|n| n.name_id == *id)
                .find_map(|n| n.to_string())
        })
    };
    let family = name(&[16, 1]).unwrap_or_else(|| "Unnamed family".into());
    let style = name(&[17, 2]).unwrap_or_else(|| "Regular".into());
    if face.tables().glyf.is_none() && face.tables().cff.is_none() && face.tables().cff2.is_none() {
        return Err("This font has no supported vector outlines (bitmap/colour fonts are not supported yet).".into());
    }
    let mut label: String = format!("{family} · {style}")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    while label.len() > 256 {
        label.pop();
    }
    Ok(label)
}
pub fn load(face: &Face) -> Result<TextFont, String> {
    let bytes = read(&face.path)?;
    let name = metadata(&bytes, face.index)?;
    TextFont::embedded(name, face.index, &bytes).map_err(|e| e.to_string())
}
pub fn from_file(path: PathBuf) -> Face {
    Face {
        path,
        index: 0,
        label: String::new(),
        added: false,
    }
}
/// Validate before writing; never overwrite an existing file, even on a name collision.
fn install_in(dir: &Path, bytes: &[u8]) -> Result<String, String> {
    use std::hash::{Hash, Hasher};
    if bytes.len() > veylvik_document::text::MAX_FONT_BYTES {
        return Err("Font exceeds 8 MiB".into());
    }
    let name = metadata(bytes, 0)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let extension = if bytes.starts_with(b"ttcf") {
        "ttc"
    } else if bytes.starts_with(b"OTTO") {
        "otf"
    } else {
        "ttf"
    };
    let mut path = dir.join(format!("font-{:016x}.{extension}", hash.finish()));
    if path.exists() {
        if read(&path)? == bytes {
            return Ok(name);
        }
        path = dir.join(format!(
            "font-{}.{extension}",
            veylvik_document::NodeId::new_v4()
        ));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&path);
        return Err(e.to_string());
    }
    Ok(name)
}
pub fn install(path: &Path) -> Result<String, String> {
    install_in(&directory()?, &read(path)?)
}
fn parse_system(text: &str) -> Vec<Face> {
    let mut seen = std::collections::BTreeSet::new();
    text.lines()
        .take(20000)
        .filter_map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 4 {
                return None;
            }
            let index: u32 = fields[1].parse().ok()?;
            if index > 65535 {
                return None;
            } // Named variable instances are not separate collection faces.
            let path = PathBuf::from(fields[0]);
            if !path.is_absolute()
                || !matches!(
                    path.extension()?.to_str()?.to_ascii_lowercase().as_str(),
                    "ttf" | "otf" | "ttc" | "otc"
                )
                || !seen.insert((path.clone(), index))
            {
                return None;
            }
            Some(Face {
                path,
                index,
                label: format!(
                    "{} · {}",
                    fields[2],
                    if fields[3].is_empty() {
                        "Regular"
                    } else {
                        fields[3]
                    }
                ),
                added: false,
            })
        })
        .collect()
}
fn scan() -> (Vec<Face>, String) {
    let mut notices = Vec::new();
    let mut faces = match std::process::Command::new("fc-list")
        .arg("--format")
        .arg("%{file}\t%{index}\t%{family[0]}\t%{style[0]}\n")
        .output()
    {
        Ok(output) if output.status.success() && output.stdout.len() <= 8 * 1024 * 1024 => {
            parse_system(&String::from_utf8_lossy(&output.stdout))
        }
        _ => {
            notices.push(
                "System font discovery unavailable. Install fontconfig or use Add font."
                    .to_string(),
            );
            vec![]
        }
    };
    if let Ok(dir) = directory()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.take(512).flatten() {
            let path = entry.path();
            if !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("ttf" | "otf" | "ttc")
            ) {
                continue;
            }
            if let Ok(bytes) = read(&path) {
                for index in 0..ttf_parser::fonts_in_collection(&bytes)
                    .unwrap_or(1)
                    .min(256)
                {
                    if let Ok(label) = metadata(&bytes, index) {
                        faces.push(Face {
                            path: path.clone(),
                            index,
                            label,
                            added: true,
                        });
                    }
                }
            }
        }
    }
    faces.sort_by_cached_key(|face| (face.label.to_lowercase(), face.path.clone(), face.index));
    (faces, notices.join("\n"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_catalog_preserves_collection_faces_and_filters_unsupported_records() {
        let entries = parse_system(
            "/fonts/Test.ttf\t0\tExample\tRegular\n/fonts/Test.ttf\t1\tExample\tBold\n/fonts/Test.ttf\t0\tExample\tRegular\n/fonts/Test.ttf\t65536\tExample\tVariable\n/fonts/Bitmap.pcf\t0\tBitmap\tRegular\ninvalid\n",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].index, 1);
    }
    #[test]
    fn installed_fonts_survive_source_removal_and_duplicate_import_is_idempotent() {
        let dir = std::env::temp_dir().join(format!(
            "veylvik-font-test-{}",
            veylvik_document::NodeId::new_v4()
        ));
        let bytes = include_bytes!("../../veylvik-text/assets/fonts/LiberationSans-Regular.ttf");
        assert!(install_in(&dir, b"invalid font").is_err());
        assert!(!dir.exists());
        assert!(install_in(&dir, bytes).unwrap().contains("Liberation Sans"));
        install_in(&dir, bytes).unwrap();
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let path = entries[0].as_ref().unwrap().path();
        let font = load(&from_file(path)).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        font.validate().unwrap();
        assert_eq!(font.bytes().unwrap().unwrap(), bytes);
    }
}
