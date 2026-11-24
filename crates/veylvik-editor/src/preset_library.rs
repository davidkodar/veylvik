//! Background library IO; insertion is one ordinary document transaction.
use crate::{
    Studio,
    files::{self, Choice, Completed},
};
use eframe::egui;
use std::{collections::BTreeMap, path::PathBuf};
use veylvik_document::{
    NodeId, NodePosition,
    presets::{Preset, builtin},
};
#[derive(Default)]
pub struct Library {
    pub open: bool,
    pub listing: veylvik_io::presets::Listing,
    pub ready: Option<Preset>,
    pub source: Option<NodeId>,
    query: String,
    category: String,
    sizing: veylvik_document::presets::resolution::Sizing,
    background_placement: veylvik_document::presets::background::Placement,
    gallery: crate::showcase_gallery::Gallery,
    pub(super) detail: Option<&'static str>,
    inputs: BTreeMap<String, NodeId>,
    pub(super) position: Option<NodePosition>,
    connect: bool,
    anchor: Option<NodeId>,
    identity: Option<(NodeId, u64)>,
    /// Save to library, waiting for the choice of sounds to bring.
    saving: Option<Saving>,
}
/// An effect about to be saved, and the composition's sounds it may bring.
struct Saving {
    group: NodeId,
    /// Sound node, its name, its file, and whether it is brought.
    sounds: Vec<(NodeId, String, String, bool)>,
}
#[derive(Clone)]
enum Source {
    Builtin(&'static str),
    File(PathBuf),
}
impl Source {
    fn load(&self) -> Result<Preset, String> {
        match self {
            Self::Builtin(id) => builtin::build(id).map_err(|e| e.to_string()),
            Self::File(path) => veylvik_io::presets::load(path).map_err(|e| e.to_string()),
        }
    }
}
enum Action {
    Refresh,
    Example(&'static str),
    MaterialLayer(&'static str, bool),
    Import,
    Load(Source),
    Export,
    Insert,
}
impl Studio {
    pub(super) fn open_preset_library(&mut self, ctx: &egui::Context) {
        if !self.ready() || !self.can_change_selection() {
            return;
        }
        self.transport.pause();
        self.error = false;
        self.library = Library {
            open: true,
            position: Some(self.graph.insertion_position(ctx)),
            identity: Some((self.project.session, self.project.document.revision())),
            ..Default::default()
        };
        self.refresh_presets();
    }
    fn refresh_presets(&mut self) {
        let result = self.files.start(|| {
            let dir = veylvik_io::presets::directory().map_err(|e| e.to_string())?;
            let listing = veylvik_io::presets::list(&dir).map_err(|e| e.to_string())?;
            Ok(Completed::then(move |s, _| s.preset_listing(listing)))
        });
        self.file_started(result);
    }
    /// Save to library: at once when the composition has no sounds, else after asking
    /// which of them the effect brings.
    pub(super) fn save_effect_preset(&mut self, id: NodeId) {
        if self.files.busy() || !self.can_change_selection() {
            return;
        }
        let sounds: Vec<_> = self
            .project
            .document
            .snapshot()
            .nodes
            .values()
            .filter_map(|node| match &node.operation {
                veylvik_document::Operation::Sound(sound) if !sound.file.trim().is_empty() => {
                    Some((node.id, node.label.clone(), sound.file.clone(), false))
                }
                _ => None,
            })
            .collect();
        if sounds.is_empty() {
            self.store_effect(id, &[]);
        } else {
            self.library.saving = Some(Saving { group: id, sounds });
        }
    }
    /// The window that asks which sounds an effect brings.
    pub(super) fn save_effect_window(&mut self, ctx: &egui::Context) {
        let Some(saving) = &mut self.library.saving else {
            return;
        };
        let (mut save, mut cancel) = (false, false);
        egui::Window::new("Save effect to library")
            .id(egui::Id::new("save-effect"))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label("Sounds the effect brings with it");
                ui.weak("A sound you tick is copied into the library and comes back as a Sound node each time the effect is used. Audio files of at most 4 MiB, eight at most.");
                ui.add_space(6.0);
                let chosen = saving.sounds.iter().filter(|s| s.3).count();
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (_, label, file, bring) in &mut saving.sounds {
                        let name = std::path::Path::new(file.as_str())
                            .file_name()
                            .map_or(file.clone(), |n| n.to_string_lossy().into_owned());
                        ui.add_enabled_ui(*bring || chosen < veylvik_document::presets::MAX_CUES, |ui| {
                            ui.checkbox(bring, format!("{label}  ({name})"));
                        });
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = crate::design::primary(ui, "Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if cancel {
            self.library.saving = None;
        } else if save && let Some(saving) = self.library.saving.take() {
            let bring: Vec<_> = saving.sounds.iter().filter(|s| s.3).map(|s| s.0).collect();
            self.store_effect(saving.group, &bring);
        }
    }
    fn store_effect(&mut self, id: NodeId, bring: &[NodeId]) {
        if self.files.busy() || !self.can_change_selection() {
            return;
        }
        let doc = self.project.document.snapshot();
        let preset = Preset::capture(doc, id).and_then(|mut preset| {
            for sound in bring {
                let veylvik_document::Operation::Sound(node) = &doc.nodes[sound].operation else {
                    continue;
                };
                // Named in full here; the library copies the file in.
                let file = if node.file.starts_with(veylvik_io::sounds::PREFIX) {
                    node.file.clone()
                } else {
                    self.project
                        .base
                        .join(&node.file)
                        .to_string_lossy()
                        .into_owned()
                };
                preset.sounds.push(Preset::cue(doc, *sound, file)?);
            }
            preset.validate()?;
            Ok(preset)
        });
        let preset = match preset {
            Ok(p) => p,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let result = self.files.start(move || {
            let dir = veylvik_io::presets::directory().map_err(|e| e.to_string())?;
            veylvik_io::presets::add(&dir, &preset).map_err(|e| e.to_string())?;
            let listing = veylvik_io::presets::list(&dir).map_err(|e| e.to_string())?;
            Ok(Completed::then(move |s, _| s.preset_listing(listing)))
        });
        self.file_started(result);
    }
    pub(super) fn loaded_preset(&mut self, preset: Preset) {
        if self.library.identity != Some((self.project.session, self.project.document.revision())) {
            self.fail("The composition changed; reopen the effect library.");
            return;
        }
        let doc = self.project.document.snapshot();
        let selected = self.library.source.unwrap_or(self.project.selected);
        let source = doc
            .groups
            .values()
            .find(|g| g.members.contains(&selected))
            .map_or(selected, |g| g.output);
        self.library.anchor = Some(source);
        self.library.connect = !preset.group.inputs.is_empty();
        self.library.inputs = preset
            .group
            .inputs
            .keys()
            .map(|name| (name.clone(), source))
            .collect();
        self.tell(format!("Loaded {}", preset.group.label));
        self.library.ready = Some(preset);
    }
    pub(super) fn preset_library(&mut self, ctx: &egui::Context) {
        if !self.library.open {
            return;
        }
        let mut open = true;
        let mut action = None;
        egui::Window::new("Effect library").id(egui::Id::new("effect-library")).open(&mut open).collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).default_width(740.0).default_height(560.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-100.0).max(220.0)).show(ui, |ui| {
                ui.add_enabled_ui(!self.files.busy(), |ui| {
                    if let Some(preset) = &self.library.ready {
                        ui.heading(&preset.group.label);
                        ui.weak(format!("{} nodes · authored at {} × {}", preset.nodes.len(), preset.width, preset.height));
                        ui.add(egui::Label::new("Choose the image for each input. Spatial values adapt to the composition. Animation timing is preserved.").wrap());
                        egui::ComboBox::from_id_salt("preset-sizing").selected_text(self.library.sizing.label()).show_ui(ui,|ui| {
                            use veylvik_document::presets::resolution::Sizing;
                            for mode in [Sizing::Fit,Sizing::Fill,Sizing::Original] {ui.selectable_value(&mut self.library.sizing,mode,mode.label());}
                        });
                        let doc = self.project.document.snapshot();
                        for (key, input) in &preset.group.inputs {
                            ui.label(&input.label);
                            let source = self.library.inputs.get_mut(key).unwrap();
                            egui::ComboBox::from_id_salt(("preset-input", key)).selected_text(&doc.nodes[source].label).show_ui(ui, |ui| {
                                for node in doc.nodes.values() {
                                    if doc.groups.values().any(|g| g.members.contains(&node.id) && g.output != node.id) { continue; }
                                    let label = doc.groups.values().find(|g| g.output == node.id).map_or(node.label.as_str(), |g| g.label.as_str());
                                    ui.selectable_value(source, node.id, label);
                                }
                            });
                        }
                        ui.checkbox(&mut self.library.connect, "Connect into selected layer").on_hover_text("Keep downstream effects and the scene output connected. Disable to add a separate branch.");
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Add effect").clicked() { action = Some(Action::Insert); }
                            if ui.button("Export preset…").clicked() { action = Some(Action::Export); }
                            if ui.button("Back").clicked() { self.library.ready = None; }
                        });
                    } else if let Some(id) = self.library.detail {
                        let entry = builtin::ALL.iter().find(|e| e.id==id).unwrap();
                        ui.heading(veylvik_presets::showcase::title(id));
                        ui.add(egui::Label::new(veylvik_presets::showcase::description(id)).wrap());
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Try example").clicked() { action=Some(Action::Example(id)); }
                            if ui.button("Apply to my composition").clicked() { action=Some(Action::Load(Source::Builtin(id))); }
                            if builtin::is_material(id) {
                                if ui.button("Use as background").on_hover_text("Replace the scene backdrop, preserving the subject and animation. Advanced placement can keep the existing backdrop.").clicked() { action=Some(Action::MaterialLayer(id,false)); }
                                if ui.button("Use as foreground").on_hover_text("Fill the canvas over the composition. Adjust opacity on Material layer composite to reveal layers below.").clicked() { action=Some(Action::MaterialLayer(id,true)); }
                            }
                            if ui.button("Back to library").clicked() { self.library.detail=None; }
                        });
                        if builtin::is_material(id) {
                            use veylvik_document::presets::background::{self,Placement};
                            let doc=self.project.document.snapshot();
                            let target=background::automatic(doc);
                            ui.weak(match self.library.background_placement {
                                Placement::Behind=>"Background: behind the finished composition; opaque areas will hide it.".into(),
                                Placement::Replace(node)=>format!("Background: replaces {}.",doc.nodes.get(&node).map_or("selected backdrop",|n|n.label.as_str())),
                                Placement::Automatic=>match target {Some(node)=>format!("Background: replaces {} while keeping the effects above it.",doc.nodes[&node].label),None=>"No single backdrop identified. Choose one under Background placement.".into()},
                            });
                            egui::CollapsingHeader::new("Background placement · advanced").show(ui,|ui| {
                                ui.selectable_value(&mut self.library.background_placement,Placement::Automatic,"Automatic — replace scene backdrop");
                                ui.selectable_value(&mut self.library.background_placement,Placement::Behind,"Behind finished composition — keep existing backdrop");
                                ui.weak("Behind mode only shows through transparent areas. Replacing keeps the old nodes available for reconnection and Undo.");
                                egui::ComboBox::from_id_salt("background-target").selected_text(match self.library.background_placement {Placement::Replace(node)=>doc.nodes.get(&node).map_or("Choose backdrop to replace",|n|n.label.as_str()),_=>"Choose backdrop to replace"}).show_ui(ui,|ui| {
                                    for node in background::candidates(doc) {
                                        let label=doc.groups.values().find(|g|g.output==node).map_or(doc.nodes[&node].label.as_str(),|g|g.label.as_str());
                                        ui.selectable_value(&mut self.library.background_placement,Placement::Replace(node),label);
                                    }
                                });
                            });
                        }
                        ui.separator();
                        self.library.gallery.detail(ui,id);
                        ui.weak(format!("{} · 4 seconds · editable scene",entry.label));
                    } else {
                        ui.horizontal_top(|ui| {
                        ui.set_max_height((ctx.content_rect().height()-190.0).clamp(240.0,445.0));
                        ui.vertical(|ui| {
                            ui.set_width(118.0);
                            ui.strong("Categories");
                            ui.separator();
                            ui.selectable_value(&mut self.library.category,String::new(),format!("All effects ({})",builtin::ALL.len()));
                            for category in builtin::CATEGORIES.iter().filter(|c| **c!="All") {
                                let count=builtin::ALL.iter().filter(|e| builtin::category(e.id)==*category).count();
                                ui.selectable_value(&mut self.library.category,category.to_string(),format!("{category} ({count})"));
                            }
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                        ui.set_min_width(280.0);
                        ui.add(egui::TextEdit::singleline(&mut self.library.query).hint_text("Search effects…").desired_width(f32::INFINITY));
                        let words: Vec<_> = self.library.query.to_lowercase().split_whitespace().map(str::to_owned).collect();
                        let matches = |text: &str| words.iter().all(|w| text.to_lowercase().contains(w));
                        egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-240.0).clamp(190.0,400.0)).show(ui, |ui| {
                            ui.strong("Starter effects");
                            let mut result_count = 0;
                            for entry in builtin::ALL {
                                if (self.library.category.is_empty() || self.library.category==builtin::category(entry.id)) && matches(&format!("{} {} {} {} {}", entry.label, entry.description,builtin::category(entry.id),veylvik_presets::showcase::title(entry.id),veylvik_presets::showcase::description(entry.id))) {
                                    result_count += 1;
                                    ui.group(|ui| { ui.horizontal(|ui| {
                                        if self.library.gallery.card(ui,entry.id).clicked() { self.library.detail=Some(entry.id); }
                                        ui.vertical(|ui| {
                                            if ui.button(entry.label).clicked() { self.library.detail=Some(entry.id); }
                                            ui.add(egui::Label::new(entry.description).wrap());
                                            ui.weak(format!("{} · {}",builtin::category(entry.id),veylvik_presets::showcase::title(entry.id)));
                                        });
                                    }); });
                                }
                            }
                            if result_count == 0 { ui.weak("No starter effects match. Try another category or search."); }
                            ui.separator(); ui.strong("Your effects");
                            if self.library.listing.entries.is_empty() { ui.weak("Select a group and choose Save to library, or import a .veyfx preset."); }
                            for entry in &self.library.listing.entries {
                                if matches(&entry.label) {
                                    ui.push_id(&entry.path, |ui| { ui.horizontal(|ui| {
                                        if ui.button(&entry.label).clicked() { action = Some(Action::Load(Source::File(entry.path.clone()))); }
                                        ui.weak(format!("{} nodes · {} inputs", entry.nodes, entry.inputs));
                                    }); });
                                }
                            }
                        });
                        });
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Import preset…").clicked() { action = Some(Action::Import); }
                            if ui.button("Refresh").clicked() { action = Some(Action::Refresh); }
                        });
                        if !self.library.listing.warnings.is_empty() {
                            egui::CollapsingHeader::new("Library notices").show(ui, |ui| {
                                for warning in &self.library.listing.warnings { ui.label(warning); }
                            });
                        }
                    }
                });
                if self.files.busy() { ui.spinner(); }
                if self.error { ui.colored_label(egui::Color32::LIGHT_RED, &self.status); }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        self.library.open &= open;
        match action {
            Some(Action::MaterialLayer(id, foreground)) => {
                if self.library.identity
                    != Some((self.project.session, self.project.document.revision()))
                {
                    self.fail("Composition changed; reopen the library.");
                    return;
                }
                let result = if foreground {
                    builtin::material_layer(
                        id,
                        self.project.document.snapshot(),
                        true,
                        self.library.position,
                    )
                } else {
                    veylvik_document::presets::background::insert(
                        id,
                        self.project.document.snapshot(),
                        self.library.background_placement,
                        self.library.position,
                    )
                };
                match result {
                    Ok(instance) => match self.project.apply(&instance.commands) {
                        Ok(()) => {
                            self.library.open = false;
                            self.project.selected = instance.output;
                            self.project.preview = None;
                            self.graph.collapse(instance.group);
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(e) => self.fail(e),
                    },
                    Err(e) => self.fail(e),
                }
            }
            Some(Action::Example(id)) => {
                self.library.open = false;
                self.leave(crate::Leave::Example(id), ctx);
            }
            Some(Action::Refresh) => self.refresh_presets(),
            Some(Action::Load(source)) => {
                let result = self.files.start(move || {
                    let preset = source.load()?;
                    Ok(Completed::then(move |s, _| s.loaded_preset(preset)))
                });
                self.file_started(result);
            }
            Some(Action::Import) => {
                let base = self.project.base.clone();
                let result = self.files.start(move || {
                    let Some(path) = files::select(Choice::PresetOpen, &base, "")? else {
                        return Ok(Completed::cancelled());
                    };
                    let preset = veylvik_io::presets::load(&path).map_err(|e| e.to_string())?;
                    let dir = veylvik_io::presets::directory().map_err(|e| e.to_string())?;
                    veylvik_io::presets::add(&dir, &preset).map_err(|e| e.to_string())?;
                    let listing = veylvik_io::presets::list(&dir).map_err(|e| e.to_string())?;
                    Ok(Completed::then(move |s, _| s.preset_listing(listing)))
                });
                self.file_started(result);
            }
            Some(Action::Export) => {
                let preset = self.library.ready.as_ref().unwrap().clone();
                let base = self.project.base.clone();
                let result = self.files.start(move || {
                    let Some(path) = files::select(Choice::PresetSave, &base, "effect.veyfx")?
                    else {
                        return Ok(Completed::cancelled());
                    };
                    veylvik_io::presets::save_new(&path, &preset).map_err(|e| e.to_string())?;
                    Ok(Completed::then(move |s, _| s.exported(path)))
                });
                self.file_started(result);
            }
            Some(Action::Insert) => {
                if self.library.identity
                    != Some((self.project.session, self.project.document.revision()))
                {
                    self.fail("Composition changed; reopen the library.");
                    return;
                }
                let preset = self.library.ready.as_ref().unwrap();
                match preset
                    .instantiate_sized(
                        self.project.document.snapshot(),
                        &self.library.inputs,
                        self.library.position,
                        self.library.sizing,
                    )
                    .and_then(|mut instance| {
                        if self.library.connect {
                            instance.commands = veylvik_document::presets::connect_after(
                                self.project.document.snapshot(),
                                self.library.anchor.unwrap(),
                                instance.output,
                                instance.commands,
                            )?;
                        }
                        Ok(instance)
                    }) {
                    Ok(instance) => match self.project.apply(&instance.commands) {
                        Ok(()) => {
                            self.library.open = false;
                            self.project.selected = instance.output;
                            self.project.preview = if self.library.connect {
                                None
                            } else {
                                Some(instance.output)
                            };
                            self.graph.collapse(instance.group);
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(e) => self.fail(e),
                    },
                    Err(e) => self.fail(e),
                }
            }
            None => {}
        }
    }
}
