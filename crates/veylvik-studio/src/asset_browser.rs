//! Docked library: browsing never changes the document; applying is explicit.
use crate::{Studio, design, icons};
use eframe::egui;
use std::collections::BTreeMap;
use veylvik_document::{
    Operation,
    presets::{builtin, text_style},
};
#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    Effects,
    #[default]
    Materials,
    Templates,
    Media,
}
#[derive(Clone, Copy)]
pub(super) struct LibraryDrag {
    id: &'static str,
    template: bool,
}
#[derive(Clone, Copy)]
enum Use {
    Text,
    Configure,
    Example,
    Background,
    Foreground,
}
pub struct Browser {
    drop_choice: Option<(LibraryDrag, Option<veylvik_document::NodePosition>)>,
    pub visible: bool,
    tab: Tab,
    query: String,
    category: String,
    selected: Option<&'static str>,
    textures: BTreeMap<&'static str, egui::TextureHandle>,
    gallery: crate::showcase_gallery::Gallery,
}
impl Browser {
    pub(super) fn choosing(&self) -> bool {
        self.drop_choice.is_some()
    }
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            visible: true,
            drop_choice: None,
            tab: Tab::Materials,
            query: String::new(),
            category: "All".into(),
            selected: None,
            textures: Default::default(),
            gallery: Default::default(),
        }
    }
}
fn swatch(id: &str) -> Option<&'static [u8]> {
    macro_rules! swatches {($($id:literal),*)=>{match id {$($id=>Some(include_bytes!(concat!("../assets/materials/",$id,".png")).as_slice()),)*_=>None}}}
    swatches!(
        "gold_text",
        "silver_text",
        "aluminium_text",
        "bronze_text",
        "phosphor_text",
        "wood_text",
        "oak_light",
        "oak_dark",
        "marble",
        "brick_light",
        "brick_red",
        "concrete",
        "paper_clean",
        "paper_aged",
        "leather_red",
        "leather_brown",
        "leather_tan",
        "leather_dark",
        "leather_black"
    )
}
fn category(id: &str) -> &'static str {
    if matches!(
        id,
        "gold_text" | "silver_text" | "aluminium_text" | "bronze_text"
    ) {
        "Metal"
    } else if id.starts_with("leather") {
        "Leather"
    } else {
        "Natural"
    }
}
fn label(entry: builtin::Builtin) -> String {
    entry
        .label
        .trim_end_matches(" Text")
        .trim_end_matches(" text")
        .to_owned()
}
impl Studio {
    pub(super) fn asset_browser(&mut self, ui: &mut egui::Ui) {
        if !self.browser.visible {
            egui::Panel::left("library-tool-rail")
                .exact_size(76.)
                .frame(design::panel())
                .show_inside(ui, |ui| {
                    for (tab, icon, label) in [
                        (Tab::Effects, icons::Icon::Effects, "Effects"),
                        (Tab::Materials, icons::Icon::Materials, "Materials"),
                        (Tab::Templates, icons::Icon::Templates, "Templates"),
                        (Tab::Media, icons::Icon::Media, "Media"),
                    ] {
                        if ui.add(icons::Button::new(icon, label).caption()).clicked() {
                            if self.browser.tab != tab {
                                self.browser.query.clear();
                                self.browser.category = "All".into();
                                self.browser.selected = None;
                            }
                            self.browser.tab = tab;
                            self.browser.visible = true;
                        }
                    }
                });
            return;
        }

        let mut use_action = None;
        let can_fill_text = matches!(self.draft.operation, Operation::Text(_));
        let mut chosen = None;
        let mut apply = None;
        let mut details = None;
        let mut example = None;
        let mut import = false;
        let mut media = None;
        egui::Panel::left("asset-library").frame(design::panel()).default_size(326.).min_size(230.).max_size(420.).resizable(true).show_inside(ui,|ui| {
            ui.horizontal(|ui| {
                ui.heading("Library");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {
                    if ui.add(icons::Button::new(icons::Icon::Left,"Collapse library")).clicked() {self.browser.visible=false;}
                });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x=1.;ui.spacing_mut().button_padding.x=5.;
                for (tab,name) in [(Tab::Effects,"Effects"),(Tab::Materials,"Materials"),(Tab::Templates,"Templates"),(Tab::Media,"Media")] {
                    if design::tab(ui,name,self.browser.tab==tab).clicked() {self.browser.tab=tab;self.browser.category="All".into();self.browser.selected=None;}
                }
            });
            ui.add_space(5.);
            ui.add(egui::TextEdit::singleline(&mut self.browser.query).hint_text("Search library…").desired_width(f32::INFINITY).margin(egui::vec2(9.,7.)));
            if self.browser.tab==Tab::Materials {
                ui.horizontal_wrapped(|ui| {for name in ["All","Metal","Natural","Leather"] {ui.selectable_value(&mut self.browser.category,name.into(),name);}});
            }
            ui.add_space(4.);
            let query=self.browser.query.to_lowercase();
            if self.browser.tab==Tab::Media {
                if ui.button("Import media…").clicked() {import=true;}
                egui::ScrollArea::vertical().id_salt("library-media").show(ui,|ui| {
                    let mut count=0;
                    for node in self.project.document.snapshot().nodes.values() {
                        if matches!(node.operation,Operation::Image(_)|Operation::Footage(_)) && node.label.to_lowercase().contains(&query) {
                            count+=1;
                            if ui.selectable_label(self.project.selected==node.id,&node.label).clicked(){media=Some(node.id);}
                        }
                    }
                    if count==0 {ui.weak("Import an image or clip to start building your scene.");}
                });
            } else {
                let selected=self.browser.selected.and_then(|id|builtin::ALL.iter().find(|e|e.id==id).copied());
                let reserve=if selected.is_some() {145.} else {38.};
                egui::ScrollArea::vertical().id_salt("library-cards").max_height((ui.available_height()-reserve).max(80.)).show(ui,|ui| {
                    let mut entries:Vec<_>=builtin::ALL.iter().copied().filter(|e| {
                        let kind=match self.browser.tab {Tab::Materials=>builtin::is_material(e.id),Tab::Effects=>!builtin::is_material(e.id),_=>true};
                        kind && (self.browser.tab!=Tab::Materials || self.browser.category=="All" || category(e.id)==self.browser.category)
                            && format!("{} {}",e.label,e.description).to_lowercase().contains(&query)
                    }).collect();
                    if self.browser.tab==Tab::Materials {entries.sort_by_key(|e|match e.id {"gold_text"=>0,"silver_text"=>1,"bronze_text"=>2,"aluminium_text"=>3,_=>4});}
                    if entries.is_empty() {ui.weak("No matching items. Try another search.");}
                    let width=((ui.available_width()-10.)/2.).floor();
                    for row in entries.chunks(2) {
                        ui.horizontal_top(|ui| {for &entry in row {
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                let response=if let Some(bytes)=swatch(entry.id).filter(|_|self.browser.tab==Tab::Materials) {
                                    let (rect,response)=ui.allocate_exact_size(egui::vec2(width,width),egui::Sense::click_and_drag());
                                    if ui.is_rect_visible(rect) {
                                        let texture=self.browser.textures.entry(entry.id).or_insert_with(|| {
                                            let image=image::load_from_memory(bytes).expect("bundled material sample").to_rgba8();
                                            ui.ctx().load_texture(entry.id,egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw()),egui::TextureOptions::LINEAR)
                                        });
                                        ui.painter().rect_filled(rect,6.,design::CANVAS);
                                        ui.painter().image(texture.id(),rect.shrink(3.),egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
                                    }
                                    response
                                } else {self.browser.gallery.tile(ui,entry.id,width)};
                                if response.hovered() || self.browser.selected==Some(entry.id) {ui.painter().rect_stroke(response.rect,6.,egui::Stroke::new(1.5_f32,if self.browser.selected==Some(entry.id) {design::ACCENT} else {design::BORDER}),egui::StrokeKind::Inside);}
                                let title=ui.add_sized([width,26.],egui::Button::new(label(entry)).frame(false).sense(egui::Sense::click_and_drag()));
                                let payload=LibraryDrag{id:entry.id,template:self.browser.tab==Tab::Templates};
                                for response in [&response,&title] {response.dnd_set_drag_payload(payload);}
                                let response=response.union(title);
                                if response.clicked() {chosen=Some(entry.id);}
                                if response.double_clicked() {use_action=Some((payload,if payload.template {Use::Example}else if builtin::is_material(entry.id)&&can_fill_text {Use::Text}else{Use::Configure}));}
                                response.context_menu(|ui| {if let Some(action)=usage_options(ui,payload,can_fill_text) {use_action=Some((payload,action));ui.close();}});
                                response.on_hover_text(format!("{}\nDouble-click to use · right-click for actions · drag to viewer or nodes",entry.description));
                            });
                        }});
                    }
                });
                ui.separator();
                if let Some(entry)=selected {
                    ui.strong(label(entry));
                    if self.browser.tab==Tab::Materials && matches!(self.draft.operation,Operation::Text(_)) {
                        if ui.add_sized([ui.available_width(),30.],egui::Button::new("Apply to selected text").fill(egui::Color32::from_rgb(110,66,174))).clicked() {apply=Some(entry.id);}
                    } else if ui.button("Apply to composition…").clicked() {details=Some(entry.id);}
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Try example").clicked() {example=Some(entry.id);}
                        if ui.button("More options…").clicked() {details=Some(entry.id);}
                    });
                } else {ui.weak("Select an item to preview or apply it.");}
            }
        });
        if let Some((item, action)) = use_action {
            self.use_library_item(item, action, None, ui.ctx());
        }
        if let Some(id) = chosen {
            self.browser.selected = Some(id);
        }
        if let Some(id) = details {
            self.open_preset_library(ui.ctx());
            if self.library.open {
                self.library.detail = Some(id);
            }
        }
        if let Some(id) = example {
            self.leave(crate::Leave::Example(id), ui.ctx());
        }
        if import {
            self.import_image(self.graph.insertion_position(ui.ctx()));
        }
        if let Some(id) = media
            && self.can_change_selection()
        {
            self.project.selected = id;
            self.project.preview = Some(id);
            self.refresh_draft();
            self.request_preview();
        }
        if let Some(id) = apply
            && self.apply_draft()
        {
            match text_style::apply(
                self.project.document.snapshot(),
                self.project.selected,
                Some(id),
                None,
            )
            .map_err(|e| e.to_string())
            .and_then(|commands| self.project.apply(&commands).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(e) => self.fail(e),
            }
        }
    }
}

impl Studio {
    pub(super) fn text_appearance_overview(&mut self, ui: &mut egui::Ui) -> bool {
        let time = match self.project.time() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut changed = false;
        ui.separator();
        ui.heading("Appearance");
        let material = text_style::current(self.project.document.snapshot(), self.draft.id)
            .ok()
            .flatten();
        if let Some(id) = material {
            let mut node = self
                .text_edits
                .get(&id)
                .unwrap_or(&self.project.document.snapshot().nodes[&id])
                .clone();
            if let Operation::Material(material) = &mut node.operation {
                let key = match material.kind {
                    veylvik_document::MaterialKind::Gold => "gold_text",
                    veylvik_document::MaterialKind::Silver => "silver_text",
                    veylvik_document::MaterialKind::Bronze => "bronze_text",
                    veylvik_document::MaterialKind::Aluminium => "aluminium_text",
                    _ => "",
                };
                ui.label("Material fill");
                egui::Frame::new()
                    .fill(design::CANVAS)
                    .corner_radius(5.)
                    .inner_margin(8.)
                    .show(ui, |ui| {
                        ui.set_min_width((ui.available_width() - 16.).max(0.));
                        ui.horizontal(|ui| {
                            if let Some(bytes) = swatch(key).filter(|_| material.optics.is_none()) {
                                let texture =
                                    self.browser.textures.entry(key).or_insert_with(|| {
                                        let image = image::load_from_memory(bytes)
                                            .expect("material thumbnail")
                                            .to_rgba8();
                                        ui.ctx().load_texture(
                                            key,
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [256, 256],
                                                image.as_raw(),
                                            ),
                                            egui::TextureOptions::LINEAR,
                                        )
                                    });
                                ui.image((texture.id(), egui::vec2(32., 32.)));
                            }
                            ui.strong(if material.optics.is_some() {
                                "Custom optical surface"
                            } else {
                                material.kind.label()
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Change…").clicked() {
                                        self.browser.visible = true;
                                        self.browser.tab = Tab::Materials;
                                        self.browser.category = "All".into();
                                        self.browser.query.clear();
                                    }
                                },
                            );
                        });
                    });
                changed |= crate::animation::scalar_with_default(
                    ui,
                    "Roughness",
                    &mut material.roughness,
                    0. ..=1.,
                    time,
                    Some(0.15),
                );
                changed |= crate::animation::scalar_with_default(
                    ui,
                    "Reflection",
                    &mut material.lighting,
                    -1. ..=1.,
                    time,
                    Some(0.),
                );
                if changed {
                    self.text_edits.insert(id, node);
                }
            }
        } else {
            ui.horizontal(|ui| {
                ui.label("Plain fill");
                if ui.button("Choose material…").clicked() {
                    self.browser.visible = true;
                    self.browser.tab = Tab::Materials;
                }
            });
        }
        ui.separator();
        changed |= crate::editable_text::transform_overview(ui, &mut self.draft.operation, time);
        changed
    }
}

fn usage_options(ui: &mut egui::Ui, item: LibraryDrag, can_fill_text: bool) -> Option<Use> {
    let mut action = None;
    if builtin::is_material(item.id) {
        if ui
            .add_enabled(can_fill_text, egui::Button::new("Apply to selected text"))
            .clicked()
        {
            action = Some(Use::Text);
        }
        if ui.button("Use as background").clicked() {
            action = Some(Use::Background);
        }
        if ui.button("Use as foreground").clicked() {
            action = Some(Use::Foreground);
        }
        ui.separator();
    }
    if ui.button("Apply to composition…").clicked() {
        action = Some(Use::Configure);
    }
    if ui
        .button(if item.template {
            "Open starter scene…"
        } else {
            "Try example scene…"
        })
        .clicked()
    {
        action = Some(Use::Example);
    }
    action
}
impl Studio {
    fn use_library_item(
        &mut self,
        item: LibraryDrag,
        action: Use,
        position: Option<veylvik_document::NodePosition>,
        ctx: &egui::Context,
    ) {
        if self.files.busy() || self.authoring_dialog_open() {
            return;
        }
        match action {
            Use::Example => self.leave(crate::Leave::Example(item.id), ctx),
            Use::Configure => {
                self.open_preset_library(ctx);
                if self.library.open {
                    self.library.detail = Some(item.id);
                    if position.is_some() {
                        self.library.position = position;
                    }
                }
            }
            Use::Text => {
                if !self.apply_draft() {
                    return;
                }
                let result = text_style::apply(
                    self.project.document.snapshot(),
                    self.project.selected,
                    Some(item.id),
                    None,
                )
                .map_err(|e| e.to_string())
                .and_then(|commands| self.project.apply(&commands).map_err(|e| e.to_string()));
                match result {
                    Ok(()) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Use::Background | Use::Foreground => {
                if !self.apply_draft() {
                    return;
                }
                let doc = self.project.document.snapshot();
                let result = if matches!(action, Use::Foreground) {
                    builtin::material_layer(item.id, doc, true, position)
                } else {
                    veylvik_document::presets::background::insert(
                        item.id,
                        doc,
                        veylvik_document::presets::background::Placement::Automatic,
                        position,
                    )
                };
                match result {
                    Ok(instance) => match self.project.apply(&instance.commands) {
                        Ok(()) => {
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
        }
    }
    pub(super) fn library_drop_target(&mut self, ui: &egui::Ui, rect: egui::Rect, graph: bool) {
        if self.files.busy() || self.authoring_dialog_open() {
            return;
        }
        if let Some(payload) = egui::DragAndDrop::payload::<LibraryDrag>(ui.ctx())
            && ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p)))
        {
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_stroke(
                rect.shrink(3.),
                5.,
                egui::Stroke::new(2.0_f32, design::ACCENT),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center_top() + egui::vec2(0., 12.),
                egui::Align2::CENTER_TOP,
                "Drop to choose placement",
                egui::FontId::proportional(14.),
                design::ACCENT,
            );
            if ui.input(|i| i.pointer.primary_released()) {
                egui::DragAndDrop::clear_payload(ui.ctx());
                self.browser.drop_choice = Some((
                    *payload,
                    graph.then(|| self.graph.insertion_position(ui.ctx())),
                ));
            }
        }
    }
    pub(super) fn library_drop_options(&mut self, ctx: &egui::Context) {
        let Some((item, position)) = self.browser.drop_choice else {
            return;
        };
        let mut open = true;
        let mut action = None;
        egui::Window::new("Use library item")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(280.)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                if let Some(entry) = builtin::ALL.iter().find(|e| e.id == item.id) {
                    ui.heading(label(*entry));
                }
                ui.label(format!("Selected layer: {}", self.draft.label));
                action =
                    usage_options(ui, item, matches!(self.draft.operation, Operation::Text(_)));
                if ui.button("Cancel").clicked() {
                    self.browser.drop_choice = None;
                }
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.browser.drop_choice = None;
        }
        if let Some(action) = action {
            self.browser.drop_choice = None;
            self.use_library_item(item, action, position, ctx);
        }
    }
}
