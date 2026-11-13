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
pub struct Browser {
    pub visible: bool,
    tab: Tab,
    query: String,
    category: String,
    selected: Option<&'static str>,
    textures: BTreeMap<&'static str, egui::TextureHandle>,
    gallery: crate::showcase_gallery::Gallery,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            visible: true,
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
                                    let (rect,response)=ui.allocate_exact_size(egui::vec2(width,width),egui::Sense::click());
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
                                if response.clicked() {chosen=Some(entry.id);}
                                response.on_hover_text(entry.description);
                                if ui.add_sized([width,26.],egui::Button::new(label(entry)).frame(false)).clicked() {chosen=Some(entry.id);}
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
                            if let Some(bytes) = swatch(key) {
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
                            ui.strong(format!("{:?}", material.kind));
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
