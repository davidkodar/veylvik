//! Compact, font-independent node identity and readable port captions.
use eframe::egui::{self, Color32, Pos2};
use veylvik_document::{Node, Operation};

pub(super) fn summary(node: &Node, name: &str, reachable: bool, output: bool) -> String {
    let detail = match &node.operation {
        Operation::Text(text) => {
            let excerpt = text
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let excerpt: String = excerpt.chars().take(80).collect();
            if name.eq_ignore_ascii_case("Text") {
                format!("\"{excerpt}\"")
            } else {
                format!("Text · \"{excerpt}\"")
            }
        }
        Operation::Material(material) => {
            if material.optics.is_some() {
                "Custom optics · Material".into()
            } else if material.kind.optics().metallic {
                format!(
                    "{} · {}",
                    material.kind.label(),
                    material.finish().map_or("Custom", |f| f.label())
                )
            } else {
                format!("{} · Material", material.kind.label())
            }
        }
        _ => {
            let kind = node.operation.descriptor().label;
            if name.eq_ignore_ascii_case(kind) {
                String::new()
            } else {
                kind.into()
            }
        }
    };
    let status = if !reachable {
        "Unused"
    } else if output {
        "Output"
    } else {
        ""
    };
    match (status.is_empty(), detail.is_empty()) {
        (true, _) => detail,
        (false, true) => status.into(),
        (false, false) => format!("{status} · {detail}"),
    }
}

pub(super) fn port_label(key: &str) -> String {
    match key {
        "background" => "Background".into(),
        "foreground" => "Foreground".into(),
        "image" => "Image".into(),
        "mask" => "Mask".into(),
        _ => {
            let mut name = key.replace('_', " ");
            if let Some(first) = name.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            name
        }
    }
}

pub(super) fn line(
    painter: &egui::Painter,
    center_left: Pos2,
    width: f32,
    text: &str,
    size: f32,
    colour: Color32,
) {
    let mut job = egui::text::LayoutJob::simple(
        text.into(),
        egui::FontId::proportional(size),
        colour,
        width.max(1.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    painter.galley(
        center_left - egui::vec2(0.0, galley.size().y * 0.5),
        galley,
        colour,
    );
}

pub(super) fn icon(
    painter: &egui::Painter,
    op: &Operation,
    grouped: bool,
    center: Pos2,
    scale: f32,
    colour: Color32,
) {
    let point = |x, y| center + egui::vec2(x, y) * scale;
    let stroke = egui::Stroke::new((1.4 * scale).max(0.8), colour);
    let segment = |a, b| {
        painter.line_segment([a, b], stroke);
    };
    match op {
        Operation::Text(_) if !grouped => {
            segment(point(-6., -6.), point(6., -6.));
            segment(point(0., -6.), point(0., 7.));
            segment(point(-3., 7.), point(3., 7.));
        }
        Operation::Material(_) | Operation::Glass(_) | Operation::Patina(_) => {
            for y in [-3., 2., 6.] {
                painter.add(egui::Shape::line(
                    vec![point(-7., y), point(0., y + 4.), point(7., y)],
                    stroke,
                ));
            }
            painter.add(egui::Shape::line(
                vec![point(-7., -3.), point(0., -7.), point(7., -3.)],
                stroke,
            ));
        }
        Operation::Merge(_) | Operation::Blend(_) => {
            segment(point(-7., -5.), point(1., 0.));
            segment(point(-7., 5.), point(1., 0.));
            segment(point(1., 0.), point(7., 0.));
            for (x, y) in [(-7., -5.), (-7., 5.), (7., 0.)] {
                painter.circle_filled(point(x, y), 1.5 * scale, colour);
            }
        }
        _ if op.inputs().is_empty() => {
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-7., -6.), point(7., 6.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(egui::Shape::line(
                vec![
                    point(-5., 4.),
                    point(-1., -1.),
                    point(2., 2.),
                    point(5., -2.),
                ],
                stroke,
            ));
        }
        _ => {
            for y in [-5., 0., 5.] {
                segment(point(-7., y), point(7., y));
                painter.circle_filled(point(if y == 0. { 3. } else { -2. }, y), 2. * scale, colour);
            }
        }
    }
}
