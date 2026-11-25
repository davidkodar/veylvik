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
    /// Index into the saved materials; `id` is then unused.
    saved: Option<usize>,
}
/// One library card: a built-in entry or one of the user's saved materials.
#[derive(Clone, Copy)]
enum Card {
    Builtin(builtin::Builtin),
    Saved(usize),
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
    selected_saved: Option<String>,
    on_text: bool,
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
            selected_saved: None,
            on_text: true,
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
use veylvik_document::presets::{catalogue::get as catalogue, material::category};
fn label(entry: builtin::Builtin) -> String {
    entry
        .label
        .trim_end_matches(" Text")
        .trim_end_matches(" text")
        .to_owned()
}
impl Studio {
    /// Open the library on its Templates tab.
    pub(crate) fn show_templates(&mut self) {
        self.browser.visible = true;
        self.browser.tab = Tab::Templates;
        self.browser.category = "All".into();
        self.browser.selected = None;
    }
    pub(super) fn asset_browser(&mut self, ui: &mut egui::Ui) {
        if !self.browser.visible {
            egui::Panel::left("library-tool-rail")
                .exact_size(76.)
                .frame(design::panel(ui.ctx()))
                .show_inside(ui, |ui| {
                    // On the 3D page the scene's contents are one click away.
                    if self.pages.page == crate::pages::Page::Scene
                        && ui
                            .button("Scene")
                            .on_hover_text("The scene's objects, cameras and lights")
                            .clicked()
                    {
                        self.pages.scene_library = false;
                    }
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
        let can_fill_text = matches!(
            self.draft.operation,
            Operation::Text(_) | Operation::Text3d(_) | Operation::Shape3d(_)
        );
        let mut chosen = None;
        let mut chosen_saved = None;
        let mut toggle_favourite: Option<String> = None;
        let mut rename: Option<String> = None;
        let mut delete: Option<String> = None;
        let mut apply = None;
        let mut details = None;
        let mut example = None;
        let mut import = false;
        let mut media = None;
        let mut add_scene = false;
        (match self.layouts.library_width() {
            Some(width) => egui::Panel::left("asset-library").exact_size(width),
            None => egui::Panel::left("asset-library").default_size(326.).min_size(230.).max_size(420.),
        }).frame(design::panel(ui.ctx())).resizable(true).show_inside(ui,|ui| {
            ui.horizontal(|ui| {
                // On the 3D page this panel shares its place with the scene's contents.
                if self.pages.page == crate::pages::Page::Scene {
                    self.left_switch(ui);
                } else {
                    ui.heading("Library");
                }
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
                ui.horizontal_wrapped(|ui| {for name in ["All","Favourites","My materials","Metal","Glossy","Glass","Neon","Natural","Leather"] {ui.selectable_value(&mut self.browser.category,name.into(),name);}});
                if can_fill_text {
                    ui.checkbox(&mut self.browser.on_text,"Show on my text").on_hover_text("Preview every material on the selected text's own words and font instead of a sample");
                }
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
                let materials_tab=self.browser.tab==Tab::Materials;
                let selected=self.browser.selected.and_then(|id|builtin::ALL.iter().find(|e|e.id==id).copied().or_else(||catalogue(id).map(|e|e.builtin())));
                let selected_saved=self.browser.selected_saved.as_ref().filter(|_|materials_tab).and_then(|key|self.materials.entries.iter().position(|e|e.key()==*key));
                let reserve=if selected.is_some()||selected_saved.is_some() {145.} else {38.};
                egui::ScrollArea::vertical().id_salt("library-cards").max_height((ui.available_height()-reserve).max(80.)).show(ui,|ui| {
                    // Whole scenes built from ordinary nodes come first among the templates.
                    if self.browser.tab==Tab::Templates && "gold title 3d scene brushed metal lettering".contains(&query) {
                        let width=((ui.available_width()-10.)/2.).floor();
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            let (rect,response)=ui.allocate_exact_size(egui::vec2(width,width*9./16.),egui::Sense::click());
                            if ui.is_rect_visible(rect) {
                                let texture=self.browser.textures.entry("scene-gold_title").or_insert_with(|| {
                                    let image=image::load_from_memory(include_bytes!("../assets/showcase/scene-gold_title.png")).expect("bundled scene preview").to_rgba8();
                                    ui.ctx().load_texture("scene-gold_title",egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw()),egui::TextureOptions::LINEAR)
                                }).id();
                                ui.painter().image(texture,rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
                                if response.hovered() {ui.painter().rect_stroke(rect,6.,egui::Stroke::new(1.5_f32,design::border(ui.ctx())),egui::StrokeKind::Inside);}
                            }
                            let caption=ui.add_sized([width,26.],egui::Button::new("Gold title (3D)").frame(false));
                            let response=response.union(caption).on_hover_text("Brushed gold 3D lettering under studio strip lights on a dark wall, as ordinary nodes sized to this composition. Double-click to add it: it becomes the output and switches the display to Filmic, and its Title node is selected so you can change the words.");
                            if response.double_clicked() {add_scene=true;}
                        });
                        ui.add_space(6.);
                    }
                    let shown=self.browser.category.clone();
                    let favourite=|key:&str|self.materials.is_favourite(key);
                    let mut cards:Vec<Card>=Vec::new();
                    if materials_tab && matches!(shown.as_str(),"All"|"Favourites"|"My materials") {
                        cards.extend(self.materials.entries.iter().enumerate().filter(|(_,e)| {
                            (shown!="Favourites"||favourite(&e.key())) && e.saved.label.to_lowercase().contains(&query)
                        }).map(|(i,_)|Card::Saved(i)));
                    }
                    let looks=veylvik_document::presets::catalogue::ENTRIES.iter().map(|e|e.builtin()).filter(|_|materials_tab);
                    let mut entries:Vec<_>=builtin::ALL.iter().copied().chain(looks).filter(|e| {
                        let kind=match self.browser.tab {Tab::Materials=>builtin::is_material(e.id),Tab::Effects=>!builtin::is_material(e.id),_=>true};
                        kind && (!materials_tab || shown=="All" || (shown=="Favourites" && favourite(&format!("builtin:{}",e.id))) || category(e.id)==shown)
                            && format!("{} {}",e.label,e.description).to_lowercase().contains(&query)
                    }).collect();
                    if materials_tab {entries.sort_by_key(|e|match e.id {"gold_text"=>0,"silver_text"=>1,"bronze_text"=>2,"aluminium_text"=>3,_=>4});}
                    cards.extend(entries.into_iter().map(Card::Builtin));
                    if cards.is_empty() {
                        ui.weak(match shown.as_str() {
                            "Favourites" if materials_tab => "No favourites yet. Click the star on a material to keep it here.",
                            "My materials" => "No saved materials yet. Select a material node and press “Save as my material…”.",
                            _ => "No matching items. Try another search.",
                        });
                    }
                    let width=((ui.available_width()-10.)/2.).floor();
                    for row in cards.chunks(2) {
                        ui.horizontal_top(|ui| {for &card in row {
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                let (key,title,description,is_selected)=match card {
                                    Card::Builtin(entry)=>(format!("builtin:{}",entry.id),label(entry),entry.description.to_owned(),self.browser.selected==Some(entry.id)),
                                    Card::Saved(i)=>{let e=&self.materials.entries[i];(e.key(),e.saved.label.clone(),format!("My material · {}",e.saved.material.kind.label()),selected_saved==Some(i))}
                                };
                                let response=match card {
                                    Card::Builtin(entry) if !materials_tab || (swatch(entry.id).is_none() && catalogue(entry.id).is_none()) => self.browser.gallery.tile(ui,entry.id,width),
                                    _ => {
                                        let (rect,response)=ui.allocate_exact_size(egui::vec2(width,width),egui::Sense::click_and_drag());
                                        if ui.is_rect_visible(rect) {
                                            let words=match &self.draft.operation {Operation::Text(t) if self.browser.on_text => Some(t), _ => None};
                                            let texture=if let Some(words)=words {
                                                // Your own words in this material.
                                                let (card_key,material)=match card {
                                                    Card::Builtin(entry)=>(format!("builtin:{}",entry.id),self.materials.builtin_material(entry.id)),
                                                    Card::Saved(i)=>(self.materials.entries[i].key(),Some(self.materials.entries[i].saved.material.clone())),
                                                };
                                                let text_key=crate::material_library::text_key(words);
                                                material.and_then(|m|self.materials.text_swatch(ui.ctx(),&self.renderer,&card_key,words,text_key,&m))
                                            } else {match card {
                                                Card::Builtin(entry) if catalogue(entry.id).is_some()=>{let material=catalogue(entry.id).unwrap().material();self.materials.swatch_of(ui.ctx(),&self.renderer,format!("builtin:{}",entry.id),&material)}
                                                Card::Builtin(entry)=>Some(self.browser.textures.entry(entry.id).or_insert_with(|| {
                                                    let image=image::load_from_memory(swatch(entry.id).unwrap()).expect("bundled material sample").to_rgba8();
                                                    ui.ctx().load_texture(entry.id,egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw()),egui::TextureOptions::LINEAR)
                                                }).id()),
                                                Card::Saved(i)=>{let entry=self.materials.entries[i].clone();self.materials.swatch(ui.ctx(),&self.renderer,&entry)}
                                            }};
                                            let picture=if words.is_some() {egui::Rect::from_center_size(rect.center(),egui::vec2(rect.width()-6.,(rect.width()-6.)*0.5))} else {rect.shrink(3.)};
                                            ui.painter().rect_filled(rect,6.,design::canvas(ui.ctx()));
                                            match texture {
                                                Some(texture)=>{ui.painter().image(texture,picture,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);}
                                                None=>{egui::Spinner::new().paint_at(ui,egui::Rect::from_center_size(rect.center(),egui::vec2(18.,18.)));}
                                            }
                                        }
                                        response
                                    }
                                };
                                if materials_tab {
                                    let star=egui::Rect::from_center_size(response.rect.right_top()+egui::vec2(-14.,14.),egui::vec2(20.,20.));
                                    let hit=ui.interact(star,ui.id().with(("favourite",&key)),egui::Sense::click());
                                    let on=self.materials.is_favourite(&key);
                                    if on||hit.hovered()||response.hovered() {
                                        ui.painter().circle_filled(star.center(),10.,egui::Color32::from_black_alpha(110));
                                        crate::material_library::paint_star(ui.painter(),star.center(),7.,on,if on {egui::Color32::from_rgb(255,200,70)} else {egui::Color32::WHITE});
                                    }
                                    if hit.on_hover_text(if on {"Remove from favourites"} else {"Add to favourites"}).clicked() {toggle_favourite=Some(key.clone());}
                                }
                                if response.hovered() || is_selected {ui.painter().rect_stroke(response.rect,6.,egui::Stroke::new(1.5_f32,if is_selected {design::accent(ui.ctx())} else {design::border(ui.ctx())}),egui::StrokeKind::Inside);}
                                let caption=ui.add_sized([width,26.],egui::Button::new(title).frame(false).sense(egui::Sense::click_and_drag()));
                                let payload=match card {
                                    Card::Builtin(entry)=>LibraryDrag{id:entry.id,template:self.browser.tab==Tab::Templates,saved:None},
                                    Card::Saved(i)=>LibraryDrag{id:"",template:false,saved:Some(i)},
                                };
                                for response in [&response,&caption] {response.dnd_set_drag_payload(payload);}
                                let response=response.union(caption);
                                let material=payload.saved.is_some()||builtin::is_material(payload.id);
                                // One click puts a material on the selected text or shape. A card
                                // that is already chosen is not applied again by a second click.
                                let one_click=material&&can_fill_text&&!payload.template;
                                if response.clicked() {
                                    match card {Card::Builtin(entry)=>{chosen=Some(entry.id);},Card::Saved(_)=>{chosen_saved=Some(key.clone());}}
                                    if one_click&&!is_selected {use_action=Some((payload,Use::Text));}
                                }
                                if response.double_clicked()&&!one_click {use_action=Some((payload,if payload.template {Use::Example}else if material&&can_fill_text {Use::Text}else if payload.saved.is_some() {Use::Background}else{Use::Configure}));}
                                response.context_menu(|ui| {
                                    if let Some(action)=usage_options(ui,payload,can_fill_text) {use_action=Some((payload,action));ui.close();}
                                    if materials_tab {
                                        if ui.button(if self.materials.is_favourite(&key) {"Remove from favourites"} else {"Add to favourites"}).clicked() {toggle_favourite=Some(key.clone());ui.close();}
                                        if let Card::Saved(_)=card {
                                            if ui.button("Rename…").clicked() {rename=Some(key.clone());ui.close();}
                                            if ui.button("Delete…").clicked() {delete=Some(key.clone());ui.close();}
                                        }
                                    }
                                });
                                response.on_hover_text(if one_click {format!("{description}\nClick to put it on the selected layer · right-click for actions · drag to viewer or nodes")} else {format!("{description}\nDouble-click to use · right-click for actions · drag to viewer or nodes")});
                            });
                        }});
                    }
                    for warning in &self.materials.warnings {ui.colored_label(ui.visuals().warn_fg_color,warning);}
                });
                ui.separator();
                if let Some(i)=selected_saved {
                    let key=self.materials.entries[i].key();
                    ui.strong(&self.materials.entries[i].saved.label);
                    let payload=LibraryDrag{id:"",template:false,saved:Some(i)};
                    if can_fill_text && ui.add_sized([ui.available_width(),30.],egui::Button::new(egui::RichText::new("Apply to selection").color(crate::theme::ink(crate::theme::palette(ui.ctx()).selection))).fill(crate::theme::palette(ui.ctx()).selection)).clicked() {use_action=Some((payload,Use::Text));}
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Use as background").clicked() {use_action=Some((payload,Use::Background));}
                        if ui.button("Use as foreground").clicked() {use_action=Some((payload,Use::Foreground));}
                        if ui.button("Rename…").clicked() {rename=Some(key.clone());}
                        if ui.button("Delete…").clicked() {delete=Some(key.clone());}
                    });
                } else if let Some(entry)=selected {
                    ui.strong(label(entry));
                    if self.browser.tab==Tab::Materials && matches!(self.draft.operation,Operation::Text(_)) {
                        if ui.add_sized([ui.available_width(),30.],egui::Button::new(egui::RichText::new("Apply to selection").color(crate::theme::ink(crate::theme::palette(ui.ctx()).selection))).fill(crate::theme::palette(ui.ctx()).selection)).clicked() {apply=Some(entry.id);}
                    } else if catalogue(entry.id).is_none() && ui.button("Apply to composition…").clicked() {details=Some(entry.id);}
                    let payload=LibraryDrag{id:entry.id,template:false,saved:None};
                    ui.horizontal_wrapped(|ui| {
                        if catalogue(entry.id).is_some() {
                            if ui.button("Use as background").clicked() {use_action=Some((payload,Use::Background));}
                            if ui.button("Use as foreground").clicked() {use_action=Some((payload,Use::Foreground));}
                        } else {
                            if ui.button("Try example").clicked() {example=Some(entry.id);}
                            if ui.button("More options…").clicked() {details=Some(entry.id);}
                        }
                    });
                } else {ui.weak("Select an item to preview or apply it.");}
            }
        });
        if let Some((item, action)) = use_action {
            self.use_library_item(item, action, None, ui.ctx());
        }
        if let Some(id) = chosen {
            self.browser.selected = Some(id);
            self.browser.selected_saved = None;
        }
        if let Some(key) = chosen_saved {
            self.browser.selected_saved = Some(key);
            self.browser.selected = None;
        }
        if let Some(key) = toggle_favourite {
            self.materials.toggle_favourite(ui.ctx(), &key);
        }
        if let Some(key) = rename {
            self.materials.rename(&key);
        }
        if let Some(key) = delete {
            self.materials.delete(&key);
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
        if add_scene {
            self.add_gold_title();
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
            match veylvik_document::presets::material::apply_commands(
                self.project.document.snapshot(),
                self.project.selected,
                id.into(),
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
            let mut save = None;
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
                    .fill(design::canvas(ui.ctx()))
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
                                    if ui
                                        .button("Save…")
                                        .on_hover_text("Save this look to My materials")
                                        .clicked()
                                    {
                                        save = Some(material.clone());
                                    }
                                },
                            );
                        });
                    });
                changed |= crate::nodes::look_slider(ui, material);
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
                    self.text_edits.insert(id, node.clone());
                }
            }
            if let Some(material) = save {
                let label = node.label.trim_start_matches("Text · ").to_owned();
                self.materials.save(&label, &material);
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
    let look = item.saved.is_some() || catalogue(item.id).is_some();
    if look || builtin::is_material(item.id) {
        if ui
            .add_enabled(
                can_fill_text,
                egui::Button::new(
                    egui::RichText::new("Apply to selection")
                        .color(crate::theme::ink(crate::theme::palette(ui.ctx()).selection)),
                ),
            )
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
        // Saved and catalogue looks have no starter scene or effect configuration.
        if look {
            return action;
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
        if !self.settled() {
            return;
        }
        let saved = match item.saved {
            Some(i) => match self.materials.entries.get(i) {
                Some(entry) => Some(entry.saved.clone()),
                None => return,
            },
            None => None,
        };
        let source = match &saved {
            Some(saved) => veylvik_document::presets::material::Source::Saved(saved),
            None => item.id.into(),
        };
        if (saved.is_some() || catalogue(item.id).is_some())
            && matches!(action, Use::Example | Use::Configure)
        {
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
                let result = veylvik_document::presets::material::apply_commands(
                    self.project.document.snapshot(),
                    self.project.selected,
                    source,
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
                    builtin::material_layer(source, doc, true, position)
                } else {
                    veylvik_document::presets::background::insert(
                        source,
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
        if !self.settled() {
            return;
        }
        if let Some(payload) = egui::DragAndDrop::payload::<LibraryDrag>(ui.ctx())
            && ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p)))
        {
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_stroke(
                rect.shrink(3.),
                5.,
                egui::Stroke::new(2.0_f32, design::accent(ui.ctx())),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center_top() + egui::vec2(0., 12.),
                egui::Align2::CENTER_TOP,
                "Drop to choose placement",
                egui::FontId::proportional(14.),
                design::accent(ui.ctx()),
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
                if let Some(entry) = item.saved.and_then(|i| self.materials.entries.get(i)) {
                    ui.heading(&entry.saved.label);
                } else if let Some(entry) = builtin::ALL.iter().find(|e| e.id == item.id) {
                    ui.heading(label(*entry));
                } else if let Some(entry) = catalogue(item.id) {
                    ui.heading(entry.label);
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
