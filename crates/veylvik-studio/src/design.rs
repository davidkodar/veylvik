//! Shared visual language for the editing workspace.
use eframe::egui::{self, Color32};
pub fn accent(ctx: &egui::Context) -> Color32 {
    crate::theme::palette(ctx).accent
}
pub fn canvas(ctx: &egui::Context) -> Color32 {
    crate::theme::palette(ctx).canvas
}
pub fn border(ctx: &egui::Context) -> Color32 {
    crate::theme::palette(ctx).border
}
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
    let palette = crate::theme::palette(ctx);
    ctx.set_theme(if palette.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    style.visuals = if palette.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.panel_fill = palette.canvas;
    style.visuals.window_fill = palette.panel;
    style.visuals.extreme_bg_color = palette.canvas;
    style.visuals.faint_bg_color = crate::theme::mix(palette.panel, palette.text, 0.025);
    style.visuals.override_text_color = None;
    style.visuals.weak_text_color = Some(palette.muted);
    style.visuals.selection.bg_fill = palette.selection;
    style.visuals.selection.stroke = egui::Stroke::new(1_f32, crate::theme::ink(palette.selection));
    style.visuals.hyperlink_color = palette.accent;
    style.visuals.window_stroke = egui::Stroke::new(1_f32, palette.border);
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);
    style.visuals.slider_trailing_fill = true;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.spacing.slider_rail_height = 4.;
    for (widget, amount) in [
        (&mut style.visuals.widgets.noninteractive, 0.),
        (&mut style.visuals.widgets.inactive, 0.025),
        (&mut style.visuals.widgets.hovered, 0.09),
        (&mut style.visuals.widgets.active, 0.15),
        (&mut style.visuals.widgets.open, 0.09),
    ] {
        widget.corner_radius = egui::CornerRadius::same(5);
        widget.bg_fill = crate::theme::mix(palette.panel, palette.text, amount);
        widget.weak_bg_fill = widget.bg_fill;
        widget.bg_stroke = egui::Stroke::new(1_f32, palette.border);
        widget.fg_stroke = egui::Stroke::new(1_f32, palette.text);
    }
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
            egui::Stroke::new(2_f32, accent(ui.ctx())),
        );
    }
    response
}

pub fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    let accent = accent(ui.ctx());
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 16.;
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.bg_fill = accent;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::NONE;
        visuals.widgets.hovered.bg_fill = accent.gamma_multiply(1.1);
        ui.add(
            egui::Slider::new(value, range)
                .show_value(false)
                .clamping(egui::SliderClamping::Never),
        )
    })
    .inner
}

pub fn panel(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::new()
        .fill(crate::theme::palette(ctx).panel)
        .inner_margin(8.)
        .outer_margin(3.)
        .corner_radius(7.)
}

/// DragValue uses an intrinsic-width button and an expanding text editor.
/// Give it a clipped child UI so long/negative values cannot resize its parent.
pub const NUMBER_WIDTH: f32 = 80.;
pub fn number(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(NUMBER_WIDTH, 26.), egui::Sense::hover());
    let mut field = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    field.set_clip_rect(rect.intersect(ui.clip_rect()));
    field.spacing_mut().button_padding.x = 2.;
    field
        .style_mut()
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.));
    let hint = value.to_string();
    field
        .add_sized(
            rect.size(),
            egui::DragValue::new(value)
                .speed(0.1)
                .range(range)
                .max_decimals(3),
        )
        .on_hover_text(hint)
}
