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
    query: String,
    category: String,
    sizing: veylvik_document::presets::resolution::Sizing,
    gallery: crate::showcase_gallery::Gallery,
    detail: Option<&'static str>,
    inputs: BTreeMap<String, NodeId>,
    position: Option<NodePosition>,
    identity: Option<(NodeId, u64)>,
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
    Import,
    Load(Source),
    Export,
    Insert,
}
impl Studio {
    pub(super) fn open_preset_library(&mut self, ctx: &egui::Context) {
        if self.files.busy() || self.authoring_dialog_open() || !self.can_change_selection() {
            return;
        }
        self.playing = false;
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
            Ok(Completed::PresetLibrary(
                veylvik_io::presets::list(&dir).map_err(|e| e.to_string())?,
            ))
        });
        self.file_started(result);
    }
    pub(super) fn save_effect_preset(&mut self, id: NodeId) {
        if self.files.busy() || !self.can_change_selection() {
            return;
        }
        let preset = match Preset::capture(self.project.document.snapshot(), id) {
            Ok(p) => p,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let result = self.files.start(move || {
            let dir = veylvik_io::presets::directory().map_err(|e| e.to_string())?;
            veylvik_io::presets::add(&dir, &preset).map_err(|e| e.to_string())?;
            Ok(Completed::PresetLibrary(
                veylvik_io::presets::list(&dir).map_err(|e| e.to_string())?,
            ))
        });
        self.file_started(result);
    }
    pub(super) fn loaded_preset(&mut self, preset: Preset) {
        if self.library.identity != Some((self.project.session, self.project.document.revision())) {
            self.fail("The composition changed; reopen the effect library.");
            return;
        }
        let doc = self.project.document.snapshot();
        let source = doc
            .groups
            .values()
            .find(|g| g.members.contains(&self.project.selected))
            .map_or(self.project.selected, |g| g.output);
        self.library.inputs = preset
            .group
            .inputs
            .keys()
            .map(|name| (name.clone(), source))
            .collect();
        self.status = format!("Loaded {}", preset.group.label);
        self.error = false;
        self.library.ready = Some(preset);
    }
    pub(super) fn preset_library(&mut self, ctx: &egui::Context) {
        if !self.library.open {
            return;
        }
        let mut open = true;
        let mut action = None;
        egui::Window::new("Effect library").id(egui::Id::new("effect-library")).open(&mut open).collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).default_width(560.0).default_height(560.0).show(ctx, |ui| {
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
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Add effect").clicked() { action = Some(Action::Insert); }
                            if ui.button("Export preset…").clicked() { action = Some(Action::Export); }
                            if ui.button("Back").clicked() { self.library.ready = None; }
                        });
                    } else if let Some(id) = self.library.detail {
                        let entry = builtin::ALL.iter().find(|e| e.id==id).unwrap();
                        ui.heading(veylvik_document::presets::showcase::title(id));
                        ui.add(egui::Label::new(veylvik_document::presets::showcase::description(id)).wrap());
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Try example").clicked() { action=Some(Action::Example(id)); }
                            if ui.button("Apply to my composition").clicked() { action=Some(Action::Load(Source::Builtin(id))); }
                            if ui.button("Back to library").clicked() { self.library.detail=None; }
                        });
                        ui.separator();
                        self.library.gallery.detail(ui,id);
                        ui.weak(format!("{} · 4 seconds · editable scene",entry.label));
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut self.library.query).hint_text("Search effects…").desired_width(f32::INFINITY));
                        ui.horizontal_wrapped(|ui| {
                            egui::ComboBox::from_id_salt("effect-category")
                                .selected_text(if self.library.category.is_empty() {"All categories"} else {&self.library.category})
                                .show_ui(ui,|ui| {
                                    ui.selectable_value(&mut self.library.category,String::new(),"All categories");
                                    for category in builtin::CATEGORIES.iter().filter(|c| **c!="All") {
                                        ui.selectable_value(&mut self.library.category,category.to_string(),*category);
                                    }
                                });
                            ui.weak(format!("{} starter effects",builtin::ALL.len()));
                        });
                        let words: Vec<_> = self.library.query.to_lowercase().split_whitespace().map(str::to_owned).collect();
                        let matches = |text: &str| words.iter().all(|w| text.to_lowercase().contains(w));
                        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                            ui.strong("Starter effects");
                            for entry in builtin::ALL {
                                if (self.library.category.is_empty() || self.library.category==builtin::category(entry.id)) && matches(&format!("{} {} {} {} {}", entry.label, entry.description,builtin::category(entry.id),veylvik_document::presets::showcase::title(entry.id),veylvik_document::presets::showcase::description(entry.id))) {
                                    ui.group(|ui| { ui.horizontal(|ui| {
                                        if self.library.gallery.card(ui,entry.id).clicked() { self.library.detail=Some(entry.id); }
                                        ui.vertical(|ui| {
                                            if ui.button(entry.label).clicked() { self.library.detail=Some(entry.id); }
                                            ui.add(egui::Label::new(entry.description).wrap());
                                            ui.weak(format!("{} · {}",builtin::category(entry.id),veylvik_document::presets::showcase::title(entry.id)));
                                        });
                                    }); });
                                }
                            }
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
            Some(Action::Example(id)) => {
                self.library.open = false;
                self.leave(crate::Leave::Example(id), ctx);
            }
            Some(Action::Refresh) => self.refresh_presets(),
            Some(Action::Load(source)) => {
                let result = self
                    .files
                    .start(move || Ok(Completed::PresetLoaded(Box::new(source.load()?))));
                self.file_started(result);
            }
            Some(Action::Import) => {
                let base = self.project.base.clone();
                let result = self.files.start(move || {
                    let Some(path) = files::select(Choice::PresetOpen, &base, "")? else {
                        return Ok(Completed::Cancelled);
                    };
                    let preset = veylvik_io::presets::load(&path).map_err(|e| e.to_string())?;
                    let dir = veylvik_io::presets::directory().map_err(|e| e.to_string())?;
                    veylvik_io::presets::add(&dir, &preset).map_err(|e| e.to_string())?;
                    Ok(Completed::PresetLibrary(
                        veylvik_io::presets::list(&dir).map_err(|e| e.to_string())?,
                    ))
                });
                self.file_started(result);
            }
            Some(Action::Export) => {
                let preset = self.library.ready.as_ref().unwrap().clone();
                let base = self.project.base.clone();
                let result = self.files.start(move || {
                    let Some(path) = files::select(Choice::PresetSave, &base, "effect.veyfx")?
                    else {
                        return Ok(Completed::Cancelled);
                    };
                    veylvik_io::presets::save_new(&path, &preset).map_err(|e| e.to_string())?;
                    Ok(Completed::Exported(path))
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
                match preset.instantiate_sized(
                    self.project.document.snapshot(),
                    &self.library.inputs,
                    self.library.position,
                    self.library.sizing,
                ) {
                    Ok(instance) => match self.project.apply(&instance.commands) {
                        Ok(()) => {
                            self.library.open = false;
                            self.project.selected = instance.output;
                            self.project.preview = Some(instance.output);
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
