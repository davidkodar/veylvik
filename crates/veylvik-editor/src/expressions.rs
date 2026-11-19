//! Expression editing for scalar parameters: formulas are shown and typed with node names, links are picked from a menu, and the document turns names into ids when the draft is applied.
use eframe::egui;
use std::collections::BTreeMap;
use std::sync::Arc;
use veylvik_document::{Composition, NodeId, Scalar, Time};

/// The project's nodes as an expression can name them, published by the inspector.
#[derive(Default)]
struct Names {
    /// Node id text -> label, for nodes whose label is unique.
    labels: BTreeMap<String, String>,
    /// Linkable nodes: label and animatable parameters.
    nodes: Vec<(String, Vec<&'static str>)>,
}
fn key() -> egui::Id {
    egui::Id::new("expression-names")
}
/// Make the project's node names available to expression rows drawn this frame.
pub fn publish(ctx: &egui::Context, composition: &Composition) {
    let mut names = Names::default();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in composition.nodes.values() {
        *counts.entry(node.label.as_str()).or_default() += 1;
    }
    for node in composition.nodes.values() {
        let unique = counts[node.label.as_str()] == 1 && !node.label.contains(']');
        let parameters = veylvik_document::expression::scalar_names(node);
        if unique {
            names.labels.insert(node.id.to_string(), node.label.clone());
            if !parameters.is_empty() {
                names.nodes.push((node.label.clone(), parameters));
            }
        }
    }
    names.nodes.sort_by_key(|n| n.0.to_lowercase());
    ctx.data_mut(|d| d.insert_temp(key(), Arc::new(names)));
}
fn names(ctx: &egui::Context) -> Arc<Names> {
    ctx.data(|d| d.get_temp::<Arc<Names>>(key()))
        .unwrap_or_default()
}
/// `source` with node ids shown as their names.
fn readable(ctx: &egui::Context, source: &str) -> String {
    let names = names(ctx);
    veylvik_expr::relink(source, |text| {
        text.parse::<NodeId>()
            .ok()
            .and_then(|id| names.labels.get(&id.to_string()).cloned())
    })
    .unwrap_or_else(|_| source.to_string())
}
/// What the field shows, kept while typing; reset when the stored formula changes.
#[derive(Clone, Default)]
struct Buffer {
    stored: String,
    text: String,
    problem: Option<String>,
}
/// The formula field for a parameter driven by an expression, with its value now.
/// Typing commits when the field is left or Enter is pressed and the text parses.
pub fn row(ui: &mut egui::Ui, scalar: &mut Scalar, time: Time) -> bool {
    let Scalar::Expression { expression, .. } = scalar else {
        return false;
    };
    let id = ui.id().with("expression");
    let mut buffer: Buffer = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if buffer.stored != *expression {
        buffer = Buffer {
            stored: expression.clone(),
            text: readable(ui.ctx(), expression),
            problem: None,
        };
    }
    let value = scalar.at(time);
    let mut changed = false;
    let width = (ui.available_width() - 4. * 25. - crate::design::NUMBER_WIDTH).max(60.);
    let response = ui.add(
        egui::TextEdit::singleline(&mut buffer.text)
            .desired_width(width)
            .font(egui::TextStyle::Monospace)
            .hint_text("sin(time * 4) * 10"),
    );
    let response = response.on_hover_text(
        "An expression: time (seconds), frame, numbers, + - * / % ^, functions such as sin, cos, noise, random, clamp, mix and smoothstep, and links such as [Title].size. The result is kept within the parameter's range.",
    );
    if response.lost_focus() && buffer.text.trim() != readable(ui.ctx(), &buffer.stored) {
        match veylvik_expr::parse(buffer.text.trim()) {
            Ok(_) => {
                *scalar = Scalar::Expression {
                    expression: buffer.text.trim().to_string(),
                    resolved: None,
                };
                buffer.problem = None;
                changed = true;
            }
            Err(e) => buffer.problem = Some(e.0),
        }
    }
    match &value {
        Ok(v) => ui.weak(format!("= {v:.3}")),
        Err(_) => ui.weak("= …"),
    };
    if let Some(problem) = &buffer.problem {
        ui.colored_label(egui::Color32::LIGHT_RED, problem)
            .on_hover_text(problem);
    }
    ui.data_mut(|d| d.insert_temp(id, buffer));
    changed
}
/// Append a link to `source` with + between it and what was there.
fn with_link(source: &str, link: &str) -> String {
    let trimmed = source.trim_end();
    if trimmed.is_empty() || trimmed.ends_with(['+', '-', '*', '/', '%', '^', '(', ',']) {
        format!(
            "{trimmed}{}{link}",
            if trimmed.is_empty() { "" } else { " " }
        )
    } else {
        format!("{trimmed} + {link}")
    }
}
/// The Expression part of a parameter's details menu: start one, link to another
/// parameter, or go back to a plain value.
pub fn section(ui: &mut egui::Ui, scalar: &mut Scalar, time: Time) -> bool {
    let mut changed = false;
    ui.separator();
    let names = names(ui.ctx());
    let current = scalar.at(time).ok();
    let expression = match scalar {
        Scalar::Expression { expression, .. } => Some(readable(ui.ctx(), expression)),
        _ => None,
    };
    ui.horizontal_wrapped(|ui| {
        if expression.is_none()
            && ui
                .button("Use an expression")
                .on_hover_text("Drive this value with a formula of time, frame and other parameters instead of keys")
                .clicked()
        {
            *scalar = Scalar::Expression {
                expression: format!("{}", current.unwrap_or(0.0)),
                resolved: None,
            };
            changed = true;
        }
        ui.menu_button(
            if expression.is_some() {
                "Add a link"
            } else {
                "Link to a parameter"
            },
            |ui| {
                egui::ScrollArea::vertical().max_height(320.).show(ui, |ui| {
                    for (label, parameters) in &names.nodes {
                        ui.menu_button(label, |ui| {
                            for parameter in parameters {
                                if ui.button(*parameter).clicked() {
                                    let link = format!("[{label}].{parameter}");
                                    *scalar = Scalar::Expression {
                                        expression: with_link(
                                            expression.as_deref().unwrap_or(""),
                                            &link,
                                        ),
                                        resolved: None,
                                    };
                                    changed = true;
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            },
        )
        .response
        .on_hover_text("Follow another node's parameter; it can be scaled or offset in the expression, such as [Title].size * 0.5 + 10");
        if expression.is_some()
            && ui
                .button("Remove expression")
                .on_hover_text("Keep the value it has at the playhead as a plain value")
                .clicked()
        {
            *scalar = current.unwrap_or(0.0).into();
            changed = true;
        }
    });
    changed
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_are_appended_with_a_plus_unless_an_operator_is_waiting() {
        assert_eq!(with_link("", "[A].x"), "[A].x");
        assert_eq!(with_link("2 *", "[A].x"), "2 * [A].x");
        assert_eq!(with_link("[B].y", "[A].x"), "[B].y + [A].x");
    }
}
