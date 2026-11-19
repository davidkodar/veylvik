//! Shared visual language for the editing workspace.
use eframe::egui::{self, Color32, Stroke};
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

/// A choice shown as a card: a small picture of the frame's shape, a name and a
/// detail line. Selected cards take the accent colour.
pub fn format_card(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    shape: Option<[f32; 2]>,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(118.0, 96.0), egui::Sense::click());
    let accent = accent(ui.ctx());
    let visuals = ui.visuals();
    let (fill, stroke) = if selected {
        (accent.gamma_multiply(0.18), Stroke::new(1.5_f32, accent))
    } else if response.hovered() {
        (
            visuals.widgets.hovered.weak_bg_fill,
            Stroke::new(1.0_f32, border(ui.ctx())),
        )
    } else {
        (
            visuals.widgets.inactive.weak_bg_fill,
            Stroke::new(1.0_f32, border(ui.ctx())),
        )
    };
    let painter = ui.painter();
    painter.rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
    let ink = if selected {
        accent
    } else {
        visuals.text_color()
    };
    let area = egui::Rect::from_center_size(
        rect.center_top() + egui::vec2(0.0, 30.0),
        egui::vec2(60.0, 34.0),
    );
    match shape {
        Some([w, h]) => {
            // The frame's proportions, fitted into the picture area.
            let scale = (area.width() / w).min(area.height() / h);
            let frame = egui::Rect::from_center_size(area.center(), egui::vec2(w, h) * scale);
            painter.rect(
                frame,
                2.0,
                ink.gamma_multiply(0.16),
                Stroke::new(1.5_f32, ink),
                egui::StrokeKind::Inside,
            );
        }
        None => {
            // Custom: a frame with adjustable corners.
            let frame = egui::Rect::from_center_size(area.center(), egui::vec2(44.0, 28.0));
            painter.rect_stroke(
                frame,
                2.0,
                Stroke::new(1.0_f32, ink.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
            for corner in [
                frame.left_top(),
                frame.right_top(),
                frame.left_bottom(),
                frame.right_bottom(),
            ] {
                painter.circle_filled(corner, 3.0, ink);
            }
        }
    }
    painter.text(
        rect.center_top() + egui::vec2(0.0, 58.0),
        egui::Align2::CENTER_TOP,
        name,
        egui::FontId::proportional(13.5),
        if selected {
            accent
        } else {
            visuals.strong_text_color()
        },
    );
    painter.text(
        rect.center_top() + egui::vec2(0.0, 75.0),
        egui::Align2::CENTER_TOP,
        detail,
        egui::FontId::proportional(11.5),
        visuals.weak_text_color(),
    );
    response
}
/// A small pill for one of a few choices.
pub fn chip(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let accent = accent(ui.ctx());
    let text = egui::RichText::new(label).size(13.0);
    let button = if selected {
        egui::Button::new(text.color(accent).strong())
            .fill(accent.gamma_multiply(0.18))
            .stroke(Stroke::new(1.5_f32, accent))
    } else {
        egui::Button::new(text).stroke(Stroke::new(1.0_f32, border(ui.ctx())))
    };
    ui.add(button.corner_radius(14.0).min_size(egui::vec2(58.0, 28.0)))
}
/// The button that carries out a dialog's purpose.
pub fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let accent = accent(ui.ctx());
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .color(Color32::from_gray(16)),
        )
        .fill(accent)
        .corner_radius(8.0)
        .min_size(egui::vec2(110.0, 32.0)),
    )
}
/// A dialog's quiet section heading.
pub fn heading(ui: &mut egui::Ui, label: &str) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(label)
            .size(12.0)
            .color(ui.visuals().weak_text_color())
            .strong(),
    );
    ui.add_space(2.0);
}
/// A choice shown as a wide card with a name and a line saying what it is for.
pub fn option_card(ui: &mut egui::Ui, name: &str, detail: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(303.0, 62.0), egui::Sense::click());
    let accent = accent(ui.ctx());
    let visuals = ui.visuals();
    let (fill, stroke) = if selected {
        (accent.gamma_multiply(0.18), Stroke::new(1.5_f32, accent))
    } else if response.hovered() {
        (
            visuals.widgets.hovered.weak_bg_fill,
            Stroke::new(1.0_f32, border(ui.ctx())),
        )
    } else {
        (
            visuals.widgets.inactive.weak_bg_fill,
            Stroke::new(1.0_f32, border(ui.ctx())),
        )
    };
    let painter = ui.painter();
    painter.rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
    painter.text(
        rect.left_top() + egui::vec2(14.0, 11.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(14.0),
        if selected {
            accent
        } else {
            visuals.strong_text_color()
        },
    );
    painter.text(
        rect.left_top() + egui::vec2(14.0, 34.0),
        egui::Align2::LEFT_TOP,
        detail,
        egui::FontId::proportional(11.5),
        visuals.weak_text_color(),
    );
    response
}
