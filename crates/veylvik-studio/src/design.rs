//! Shared visual language for the editing workspace.
use eframe::egui::{self, Color32};
pub const ACCENT: Color32 = Color32::from_rgb(167, 106, 238);
pub const PANEL: Color32 = Color32::from_rgb(32, 35, 41);
pub const CANVAS: Color32 = Color32::from_rgb(18, 20, 25);
pub const BORDER: Color32 = Color32::from_rgb(62, 67, 78);
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "Inter Semibold".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    let mut heading = fonts.families[&egui::FontFamily::Proportional].clone();
    heading.insert(0, "Inter Semibold".into());
    fonts
        .families
        .insert(egui::FontFamily::Name("heading".into()), heading);
    fonts
}
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = CANVAS;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.slider_trailing_fill = true;
    style.spacing.slider_rail_height = 4.;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = CANVAS;
    style.visuals.faint_bg_color = Color32::from_rgb(35, 38, 45);
    style.visuals.override_text_color = Some(Color32::from_rgb(226, 228, 235));
    style.visuals.selection.bg_fill = Color32::from_rgb(99, 59, 153);
    style.visuals.selection.stroke = egui::Stroke::new(1_f32, ACCENT);
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);
    for w in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
        &mut style.visuals.widgets.noninteractive,
    ] {
        w.corner_radius = egui::CornerRadius::same(5);
        w.bg_stroke = egui::Stroke::new(1_f32, BORDER);
        w.fg_stroke = egui::Stroke::new(1_f32, Color32::from_rgb(226, 228, 235));
    }
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(39, 43, 51);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(35, 39, 46);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 57, 68);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(47, 51, 62);
    style.visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1_f32, Color32::from_rgb(116, 99, 145));
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(110, 66, 174);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(77, 49, 116);
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1_f32, ACCENT);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1_f32, BORDER);
    style.spacing.item_spacing = egui::vec2(8., 8.);
    style.spacing.button_padding = egui::vec2(9., 5.);
    style.spacing.interact_size = egui::vec2(32., 28.);
    style.spacing.window_margin = egui::Margin::same(12);
    style.spacing.menu_margin = egui::Margin::same(8);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(11.));
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(16., egui::FontFamily::Name("heading".into())),
    );
    ctx.set_global_style(style);
}
pub fn tab(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let response = ui.add(
        egui::Button::new(label)
            .frame(false)
            .min_size(egui::vec2(label.chars().count() as f32 * 6.5 + 12., 30.)),
    );
    if selected {
        let r = response.rect;
        ui.painter().line_segment(
            [r.left_bottom(), r.right_bottom()],
            egui::Stroke::new(2_f32, ACCENT),
        );
    }
    response
}

pub fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 16.;
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.bg_fill = ACCENT;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::NONE;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(190, 147, 245);
        ui.add(
            egui::Slider::new(value, range)
                .show_value(false)
                .clamping(egui::SliderClamping::Never),
        )
    })
    .inner
}

pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(8.)
        .outer_margin(3.)
        .corner_radius(7.)
}
