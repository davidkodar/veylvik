//! OpenEXR layer lists for the inspector, read once per file off the UI thread.
use eframe::egui;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

type Listing = Option<Result<Vec<String>, String>>;
fn cache() -> &'static Mutex<BTreeMap<PathBuf, Listing>> {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, Listing>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
/// The layer names of an OpenEXR still or sequence ("" is the main image); None while
/// they are being read, and an empty list for other media.
pub fn layers(ctx: &egui::Context, path: &Path) -> Listing {
    let mut entries = cache().lock().ok()?;
    if let Some(listing) = entries.get(path) {
        return listing.clone();
    }
    if entries.len() > 256 {
        entries.clear();
    }
    entries.insert(path.to_owned(), None);
    drop(entries);
    let (path, ctx) = (path.to_owned(), ctx.clone());
    let _ = std::thread::Builder::new()
        .name("veylvik-exr-layers".into())
        .spawn(move || {
            let listing = veylvik_io::footage::layers(&path)
                .map(|layers| layers.into_iter().map(|l| l.name).collect())
                .map_err(|e| e.to_string());
            if let Ok(mut entries) = cache().lock() {
                entries.insert(path, Some(listing));
            }
            ctx.request_repaint();
        });
    None
}
/// The layer chooser for footage: shown only for multi-layer OpenEXR files.
pub fn choose(ui: &mut egui::Ui, path: &Path, layer: &mut Option<String>) -> bool {
    let Some(listing) = layers(ui.ctx(), path) else {
        ui.weak("Reading layers…");
        return false;
    };
    let names = match listing {
        Ok(names) if names.len() > 1 || layer.is_some() => names,
        Ok(_) => return false,
        Err(error) => {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            return false;
        }
    };
    let shown = |name: &str| {
        if name.is_empty() {
            "Main image".to_string()
        } else {
            name.to_string()
        }
    };
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Layer");
        egui::ComboBox::from_id_salt("exr-layer")
            .selected_text(layer.as_deref().map_or("Main image".into(), shown))
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(layer, None, "Main image")
                    .on_hover_text("The beauty: top-level R, G, B, A, or the Combined pass")
                    .changed();
                for name in names.iter().filter(|n| !n.is_empty()) {
                    changed |= ui
                        .selectable_value(layer, Some(name.clone()), shown(name))
                        .changed();
                }
            })
            .response
            .on_hover_text("A pass of this multi-layer OpenEXR, such as Depth for Lens blur, Normal or a light group");
    });
    changed
}
