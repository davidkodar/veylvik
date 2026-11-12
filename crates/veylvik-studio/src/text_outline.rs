use eframe::egui;
use veylvik_io::text::{Alignment, Direction, OutlineRequest};
pub struct Controls {
    pub open: bool,
    pub pending: bool,
    pub position: Option<veylvik_document::NodePosition>,
    text: String,
    size: f32,
    origin: [f32; 2],
    rtl: bool,
    language: String,
    font_index: u32,
    alignment: Alignment,
    line_height: f32,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            open: false,
            pending: false,
            position: None,
            text: "Veylvik".into(),
            size: 64.0,
            origin: [40.0, 120.0],
            rtl: false,
            language: String::new(),
            font_index: 0,
            alignment: Alignment::Left,
            line_height: 80.0,
        }
    }
}
impl Controls {
    pub fn open_at(&mut self, position: veylvik_document::NodePosition) {
        self.position = Some(position);
        self.open = true;
    }
    pub fn show(&mut self, ctx: &egui::Context, enabled: bool) -> Option<OutlineRequest> {
        let mut request = None;
        let mut open = self.open;
        if open {
            egui::Window::new("Outline text").open(&mut open).collapsible(false).resizable(false).show(ctx,|ui| {
                ui.add_enabled_ui(enabled,|ui| {
                    ui.label("Create editable vector outlines from text with explicit line breaks.");
                    ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(3).desired_width(420.0).char_limit(4096));
                    ui.horizontal(|ui| { ui.label("Size (pixels)");ui.add(egui::DragValue::new(&mut self.size).range(1.0..=1024.0)); });
                    ui.horizontal(|ui| { ui.label("Baseline position");for value in &mut self.origin { ui.add(egui::DragValue::new(value).range(-32768.0..=32768.0)); } });
                    ui.horizontal(|ui| {
                        ui.label("Alignment");
                        egui::ComboBox::from_id_salt("text-alignment").selected_text(format!("{:?}",self.alignment)).show_ui(ui,|ui| {
                            for value in [Alignment::Left,Alignment::Center,Alignment::Right] { ui.selectable_value(&mut self.alignment,value,format!("{value:?}")); }
                        });
                        ui.label("Line spacing");ui.add(egui::DragValue::new(&mut self.line_height).range(1.0..=4096.0));
                    });
                    ui.checkbox(&mut self.rtl,"Right-to-left run");
                    ui.collapsing("Language and font collection",|ui| {
                        ui.horizontal(|ui| { ui.label("Language tag (optional)");ui.text_edit_singleline(&mut self.language); });
                        ui.horizontal(|ui| { ui.label("Face index");ui.add(egui::DragValue::new(&mut self.font_index)); });
                    });
                    ui.weak("The saved shape does not need the font. Convert again to change the wording.");
                    let valid=!self.text.trim().is_empty() && self.text.len()<=4096;
                    if !valid { ui.colored_label(egui::Color32::LIGHT_RED,"Enter visible text, up to 4096 UTF-8 bytes."); }
                    if ui.add_enabled(valid,egui::Button::new("Choose font and create outlines…")).clicked() {
                        request=Some(OutlineRequest { font:String::new(),text:self.text.clone(),font_index:self.font_index,font_size:self.size,origin:self.origin,alignment:self.alignment,line_height:Some(self.line_height),direction:if self.rtl { Direction::RightToLeft } else { Direction::LeftToRight },language:if self.language.trim().is_empty() { None } else { Some(self.language.trim().into()) } });
                    }
                });
            });
        }
        self.open = open && request.is_none();
        request
    }
}
