//! Which project files the artist trusts to read files outside their own folder. A project made, saved or opened here before is trusted; one opened for the first time that reads files elsewhere is opened confined to its folder until the artist allows it.
use std::path::{Path, PathBuf};

#[cfg(not(test))]
fn list_file() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|state| state.join("veylvik").join("trusted.json"))
}
// Tests never touch the artist's own list.
#[cfg(test)]
fn list_file() -> Option<PathBuf> {
    Some(std::env::temp_dir().join(format!("veylvik-trusted-{}.json", std::process::id())))
}
fn key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
fn read() -> Vec<PathBuf> {
    let Some(file) = list_file() else {
        return Vec::new();
    };
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        // The first time: the projects in the recent list are the artist's own.
        Err(_) => {
            let recent = file.with_file_name("recent.json");
            let paths: Vec<PathBuf> = std::fs::read(recent)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
                .unwrap_or_default()
                .iter()
                .filter_map(|entry| entry.get("path")?.as_str().map(PathBuf::from))
                .map(|path| key(&path))
                .collect();
            let _ = write(&paths);
            paths
        }
    }
}
fn write(paths: &[PathBuf]) -> std::io::Result<()> {
    let Some(file) = list_file() else {
        return Ok(());
    };
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let partial = file.with_extension("partial");
    std::fs::write(
        &partial,
        serde_json::to_vec_pretty(paths).unwrap_or_default(),
    )?;
    std::fs::rename(partial, file)
}
pub fn is_trusted(path: &Path) -> bool {
    let path = key(path);
    read().contains(&path)
}
/// Trust `path` from now on (at most the last 4096 projects are remembered).
pub fn trust(path: &Path) {
    let path = key(path);
    let mut paths = read();
    if !paths.contains(&path) {
        paths.push(path);
        let excess = paths.len().saturating_sub(4096);
        paths.drain(..excess);
        let _ = write(&paths);
    }
}

impl crate::Studio {
    /// Asks, over everything, whether a project just opened may read the files it names
    /// outside its folder. Until the artist answers they are not read.
    pub(crate) fn trust_dialog(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        if !self.trust_prompt || self.project.outside.is_empty() {
            self.trust_prompt = false;
            return;
        }
        let outside = &self.project.outside;
        let mut answer = None;
        egui::Modal::new(egui::Id::new("veylvik-trust")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.heading("This project reads files outside its folder");
            ui.add_space(6.0);
            ui.label(format!(
                "{} {} outside the project's folder. A project from someone else could \
                 use this to read your files, so they are not read until you allow it.",
                outside.len(),
                if outside.len() == 1 {
                    "file it uses is"
                } else {
                    "files it uses are"
                }
            ));
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for (named, _) in outside.iter().take(64) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(&named.label).strong());
                            ui.monospace(&named.name);
                        });
                    }
                    if outside.len() > 64 {
                        ui.label(format!("and {} more", outside.len() - 64));
                    }
                });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if crate::design::primary(ui, "Allow").clicked() {
                    answer = Some(true);
                }
                if ui
                    .add(egui::Button::new("Open without them").min_size(egui::vec2(110.0, 32.0)))
                    .clicked()
                {
                    answer = Some(false);
                }
            });
            ui.add_space(4.0);
            ui.weak("Allowing is remembered for this project file.");
        });
        match answer {
            Some(true) => {
                self.trust_prompt = false;
                let Some(path) = self.project.path.clone() else {
                    return;
                };
                // Read again from the file, now trusted, on the file worker.
                let result = self.files.start(move || {
                    trust(&path);
                    crate::project::Project::open(Some(&path))
                        .map(|project| crate::files::Completed::Opened(Box::new(project)))
                        .map_err(|e| e.to_string())
                });
                self.file_started(result);
            }
            Some(false) => {
                self.trust_prompt = false;
                self.status = format!(
                    "Opened without {} file(s) outside the project's folder; File > Reload media will not read them either.",
                    self.project.outside.len()
                );
                self.error = false;
            }
            None => {}
        }
    }
}
