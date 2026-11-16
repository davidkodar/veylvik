//! The user's saved materials: file work and GPU swatches run off the UI thread.
use eframe::egui;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{Arc, mpsc},
};
use veylvik_document::{nodes::Material, presets::material::Saved};
use veylvik_io::materials::{self, Entry};

/// Swatches rendered at once; each one is a single small GPU job.
const PARALLEL_SWATCHES: usize = 2;

struct Loaded {
    entries: Vec<Entry>,
    favourites: BTreeSet<String>,
    warnings: Vec<String>,
}
pub(crate) enum Dialog {
    Save {
        label: String,
        material: Box<Material>,
    },
    Rename {
        path: PathBuf,
        label: String,
        material: Box<Material>,
    },
    Delete {
        path: PathBuf,
        label: String,
    },
}
/// A change to the materials folder, run off the UI thread.
type Job = dyn FnOnce(&std::path::Path) -> Result<(), String> + Send;
/// A swatch render, built only when it is about to run.
type Render = Box<dyn FnOnce() -> Result<Vec<[f32; 4]>, String> + Send>;
type Swatch = mpsc::Receiver<Result<Vec<[f32; 4]>, String>>;
#[derive(Default)]
pub(crate) struct Library {
    pub entries: Vec<Entry>,
    pub favourites: BTreeSet<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub dialog: Option<Dialog>,
    loaded: bool,
    pending: Option<mpsc::Receiver<Result<Loaded, String>>>,
    /// Texture and the material JSON it was rendered from.
    swatches: HashMap<String, (String, egui::TextureHandle)>,
    rendering: HashMap<String, (String, Swatch)>,
    builtin: HashMap<String, Material>,
}

fn load() -> Result<Loaded, String> {
    let directory = materials::directory().map_err(|e| e.to_string())?;
    let listing = materials::list(&directory).map_err(|e| e.to_string())?;
    let (favourites, mut warnings) = match materials::favourites(&directory) {
        Ok(f) => (f, Vec::new()),
        Err(e) => (BTreeSet::new(), vec![format!("Favourites: {e}")]),
    };
    warnings.extend(listing.warnings);
    Ok(Loaded {
        entries: listing.entries,
        favourites,
        warnings,
    })
}

impl Library {
    /// Run a change to the library folder in the background, then reload it.
    fn run(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce(&std::path::Path) -> Result<(), String> + Send + 'static,
    ) {
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = materials::directory()
                .map_err(|e| e.to_string())
                .and_then(|d| job(&d))
                .and_then(|()| load());
            let _ = send.send(result);
            ctx.request_repaint();
        });
        self.pending = Some(receive);
    }
    /// Load once on first use and collect finished background work.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if !self.loaded && self.pending.is_none() {
            self.loaded = true;
            self.run(ctx, |_| Ok(()));
        }
        if let Some(receive) = &self.pending
            && let Ok(result) = receive.try_recv()
        {
            self.pending = None;
            match result {
                Ok(loaded) => {
                    self.entries = loaded.entries;
                    self.favourites = loaded.favourites;
                    self.warnings = loaded.warnings;
                    let keys: BTreeSet<_> = self.entries.iter().map(Entry::key).collect();
                    // Built-in looks keep their swatches; saved ones may be gone.
                    self.swatches.retain(|k, _| {
                        k.starts_with("builtin:") || k.starts_with("text:") || keys.contains(k)
                    });
                }
                Err(e) => self.error = Some(e),
            }
        }
    }
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key() == key)
    }
    pub fn is_favourite(&self, key: &str) -> bool {
        self.favourites.contains(key)
    }
    pub fn toggle_favourite(&mut self, ctx: &egui::Context, key: &str) {
        if !self.favourites.remove(key) {
            self.favourites.insert(key.to_owned());
        }
        let keys = self.favourites.clone();
        self.run(ctx, move |d| {
            materials::set_favourites(d, &keys).map_err(|e| e.to_string())
        });
    }
    pub fn save(&mut self, label: &str, material: &Material) {
        self.dialog = Some(Dialog::Save {
            label: label.to_owned(),
            material: Box::new(material.clone()),
        });
    }
    /// A swatch for a saved material, rendered on the GPU the first time it is shown.
    pub fn swatch(
        &mut self,
        ctx: &egui::Context,
        renderer: &Arc<veylvik_render::Renderer>,
        entry: &Entry,
    ) -> Option<egui::TextureId> {
        self.swatch_of(ctx, renderer, entry.key(), &entry.saved.material)
    }
    /// A GPU swatch for any material, cached under `key` until the material changes.
    pub fn swatch_of(
        &mut self,
        ctx: &egui::Context,
        renderer: &Arc<veylvik_render::Renderer>,
        key: String,
        material: &Material,
    ) -> Option<egui::TextureId> {
        let version = serde_json::to_string(material).unwrap_or_default();
        self.swatch_with(ctx, key, version, [256, 256], || {
            let (renderer, material) = (renderer.clone(), material.clone());
            Box::new(move || {
                renderer
                    .material_sample(&material)
                    .map_err(|e| e.to_string())
            })
        })
    }
    /// The user's text in a material, cached until the text or the material changes.
    pub fn text_swatch(
        &mut self,
        ctx: &egui::Context,
        renderer: &Arc<veylvik_render::Renderer>,
        card: &str,
        text: &veylvik_document::nodes::Text,
        text_key: u64,
        material: &Material,
    ) -> Option<egui::TextureId> {
        let version = format!(
            "{text_key}:{}",
            serde_json::to_string(material).unwrap_or_default()
        );
        self.swatch_with(ctx, format!("text:{card}"), version, [256, 128], || {
            // Cloned only when a render starts: the text may carry an embedded font.
            let (renderer, text, material) = (renderer.clone(), text.clone(), material.clone());
            Box::new(move || {
                renderer
                    .material_on_text(&text, &material)
                    .map_err(|e| e.to_string())
            })
        })
    }
    fn swatch_with(
        &mut self,
        ctx: &egui::Context,
        key: String,
        version: String,
        size: [usize; 2],
        job: impl FnOnce() -> Render,
    ) -> Option<egui::TextureId> {
        if let Some((_, receive)) = self.rendering.get(&key)
            && let Ok(result) = receive.try_recv()
        {
            let (rendered, _) = self.rendering.remove(&key).unwrap();
            match result {
                Ok(pixels) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &srgb(&pixels));
                    let texture = ctx.load_texture(&key, image, egui::TextureOptions::LINEAR);
                    self.swatches.insert(key.clone(), (rendered, texture));
                }
                Err(e) => self.warnings.push(format!("{key}: {e}")),
            }
        }
        let current = self.swatches.get(&key);
        let stale = current.is_none_or(|(v, _)| *v != version);
        if stale && !self.rendering.contains_key(&key) && self.rendering.len() < PARALLEL_SWATCHES {
            let (send, receive) = mpsc::channel();
            let (ctx, job) = (ctx.clone(), job());
            std::thread::spawn(move || {
                let _ = send.send(job());
                ctx.request_repaint();
            });
            self.rendering.insert(key.clone(), (version, receive));
        }
        current.map(|(_, t)| t.id())
    }
    /// A built-in look's material, built once.
    pub fn builtin_material(&mut self, id: &str) -> Option<Material> {
        if let Some(m) = self.builtin.get(id) {
            return Some(m.clone());
        }
        let m = veylvik_document::presets::material::Source::Builtin(id)
            .material()
            .ok()?;
        self.builtin.insert(id.to_owned(), m.clone());
        Some(m)
    }

    /// Save, rename and delete dialogs. Changes happen in the background.
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let (title, action) = match dialog {
            Dialog::Save { .. } => ("Save my material", "Save"),
            Dialog::Rename { .. } => ("Rename material", "Rename"),
            Dialog::Delete { .. } => ("Delete material", "Delete"),
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let busy = self.pending.is_some();
        egui::Window::new(title)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(300.)
            .open(&mut open)
            .show(ctx, |ui| {
                let saving = matches!(dialog, Dialog::Save { .. });
                let valid = match dialog {
                    Dialog::Save { label, .. } | Dialog::Rename { label, .. } => {
                        ui.label("Name");
                        let response = ui.add(
                            egui::TextEdit::singleline(label)
                                .char_limit(veylvik_document::presets::material::MAX_LABEL)
                                .desired_width(f32::INFINITY),
                        );
                        if !response.has_focus() && !response.lost_focus() {
                            response.request_focus();
                        }
                        confirm |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if saving {
                            ui.weak("Saves the surface, colours, finish, optics and lighting rig. Use it from My materials in the Library.");
                        }
                        !label.trim().is_empty()
                    }
                    Dialog::Delete { label, .. } => {
                        ui.label(format!("Delete “{label}” from My materials? Projects that use it keep their copy."));
                        true
                    }
                };
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    confirm |= ui.add_enabled(valid && !busy, egui::Button::new(action)).clicked();
                    cancel |= ui.button("Cancel").clicked();
                });
                if !valid {
                    confirm = false;
                }
            });
        if !open || cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dialog = None;
            return;
        }
        if !confirm || busy {
            return;
        }
        let dialog = self.dialog.take().unwrap();
        let job: Box<Job> = match dialog {
            Dialog::Save { label, material } => match Saved::new(&label, &material) {
                Ok(saved) => Box::new(move |d| {
                    materials::add(d, &saved)
                        .map(drop)
                        .map_err(|e| e.to_string())
                }),
                Err(e) => return self.error = Some(e.to_string()),
            },
            Dialog::Rename {
                path,
                label,
                material,
            } => match Saved::new(&label, &material) {
                Ok(saved) => Box::new(move |d| {
                    materials::replace(d, &path, &saved).map_err(|e| e.to_string())
                }),
                Err(e) => return self.error = Some(e.to_string()),
            },
            Dialog::Delete { path, .. } => {
                Box::new(move |d| materials::remove(d, &path).map_err(|e| e.to_string()))
            }
        };
        self.run(ctx, job);
    }
    pub fn rename(&mut self, key: &str) {
        if let Some(entry) = self.entry(key) {
            self.dialog = Some(Dialog::Rename {
                path: entry.path.clone(),
                label: entry.saved.label.clone(),
                material: Box::new(entry.saved.material.clone()),
            });
        }
    }
    pub fn delete(&mut self, key: &str) {
        if let Some(entry) = self.entry(key) {
            self.dialog = Some(Dialog::Delete {
                path: entry.path.clone(),
                label: entry.saved.label.clone(),
            });
        }
    }
}

/// Premultiplied linear samples as straight sRGB bytes for display.
fn srgb(pixels: &[[f32; 4]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pixels.len() * 4);
    for &[r, g, b, a] in pixels {
        let a = a.clamp(0., 1.);
        let straight = |c: f32| {
            if a > 0.0001 {
                egui::ecolor::gamma_u8_from_linear_f32((c / a).clamp(0., 1.))
            } else {
                0
            }
        };
        bytes.extend([
            straight(r),
            straight(g),
            straight(b),
            (a * 255.).round() as u8,
        ]);
    }
    bytes
}

/// A five-point star, filled when the material is a favourite.
pub(crate) fn paint_star(
    painter: &egui::Painter,
    centre: egui::Pos2,
    radius: f32,
    filled: bool,
    colour: egui::Color32,
) {
    let point = |i: usize, r: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.;
        centre + egui::vec2(a.cos(), a.sin()) * r
    };
    let outline: Vec<_> = (0..10)
        .map(|i| point(i, if i % 2 == 0 { radius } else { radius * 0.45 }))
        .collect();
    if filled {
        let inner: Vec<_> = (0..5).map(|i| outline[i * 2 + 1]).collect();
        painter.add(egui::Shape::convex_polygon(
            inner,
            colour,
            egui::Stroke::NONE,
        ));
        for i in 0..5 {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    outline[(i * 2 + 9) % 10],
                    outline[i * 2],
                    outline[i * 2 + 1],
                ],
                colour,
                egui::Stroke::NONE,
            ));
        }
    }
    painter.add(egui::Shape::closed_line(
        outline,
        egui::Stroke::new(1.2_f32, colour),
    ));
}

impl crate::Studio {
    /// Move the selected material's lights into a new Light rig node and connect it,
    /// as one undo step, so other surfaces can share the same lights.
    pub(crate) fn share_material_lights(&mut self) {
        use veylvik_document::{Command, Node, Operation, nodes::LightRig};
        if !self.apply_draft() {
            return;
        }
        let doc = self.project.document.snapshot();
        let Some(mut material) = doc.nodes.get(&self.project.selected).cloned() else {
            return;
        };
        let Operation::Material(m) = &material.operation else {
            return;
        };
        let mut rig = Node::new(
            "Light rig",
            Operation::LightRig(LightRig {
                studio: m.studio.clone(),
            }),
            &[],
        );
        rig.position = material.position.map(|p| veylvik_document::NodePosition {
            x: p.x - 260.0,
            y: p.y + 140.0,
        });
        material.inputs.insert("lights".into(), rig.id);
        let commands = [
            Command::Put {
                node: Box::new(rig),
            },
            Command::Put {
                node: Box::new(material),
            },
        ];
        match self.project.apply(&commands) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e.to_string()),
        }
    }
}

/// A cheap identity for a text layer's preview: its words and font, not the font data.
pub(crate) fn text_key(text: &veylvik_document::nodes::Text) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.content.hash(&mut h);
    match &*text.font {
        veylvik_document::TextFont::Embedded { name, index, data } => {
            (name, index, data.len()).hash(&mut h)
        }
        other => format!("{other:?}").hash(&mut h),
    }
    h.finish()
}
