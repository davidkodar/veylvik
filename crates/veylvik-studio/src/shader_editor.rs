//! The custom shader node's inspector: a code field that only commits code that compiles, with the compiler's message under it, and the node's sixteen values shown as the controls the code declares.
use eframe::egui;
use veylvik_document::{Operation, Scalar, nodes::Shader};
use veylvik_shader::Control;

/// What the code field shows while typing; reset when the node's code changes.
#[derive(Clone, Default)]
struct Buffer {
    node: Option<veylvik_document::NodeId>,
    stored: String,
    text: String,
    /// The compiler's verdict on `text`.
    problem: Option<String>,
    checked: String,
}
/// One of the node's sixteen values as its declared control: label, range, default.
pub struct Slot {
    pub label: String,
    pub range: std::ops::RangeInclusive<f32>,
    pub default: f32,
}
/// The control behind value `parameter` (p1 to p16) of a shader node, or None when
/// the code declares nothing there (the value is unused and not shown).
pub fn slot(operation: &Operation, parameter: &str) -> Option<Slot> {
    let Operation::Shader(shader) = operation else {
        return None;
    };
    let index = parameter
        .strip_prefix('p')?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    let spaced = |name: &str| name.replace('_', " ");
    veylvik_shader::controls(&shader.code)
        .ok()?
        .into_iter()
        .find_map(|control| match control {
            Control::Slider {
                name,
                slot,
                min,
                max,
                default,
            } if slot == index => Some(Slot {
                label: spaced(&name),
                range: min..=max,
                default,
            }),
            Control::Colour {
                name,
                slot,
                default,
            } if (slot..slot + 3).contains(&index) => Some(Slot {
                label: format!(
                    "{} {}",
                    spaced(&name),
                    ["red", "green", "blue"][index - slot]
                ),
                range: 0.0..=64.0,
                default: default[index - slot],
            }),
            _ => None,
        })
}
fn values(shader: &mut Shader) -> [&mut Scalar; veylvik_shader::SLOTS] {
    [
        &mut shader.p1,
        &mut shader.p2,
        &mut shader.p3,
        &mut shader.p4,
        &mut shader.p5,
        &mut shader.p6,
        &mut shader.p7,
        &mut shader.p8,
        &mut shader.p9,
        &mut shader.p10,
        &mut shader.p11,
        &mut shader.p12,
        &mut shader.p13,
        &mut shader.p14,
        &mut shader.p15,
        &mut shader.p16,
    ]
}
/// The code editor. Returns whether the node changed.
pub fn show(ui: &mut egui::Ui, node: veylvik_document::NodeId, shader: &mut Shader) -> bool {
    let id = egui::Id::new("shader-editor");
    let mut buffer: Buffer = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if buffer.node != Some(node) || buffer.stored != shader.code {
        buffer = Buffer {
            node: Some(node),
            stored: shader.code.clone(),
            text: shader.code.clone(),
            problem: None,
            checked: shader.code.clone(),
        };
    }
    let mut changed = false;
    ui.weak("Write fn shade(at: vec2<f32>) -> vec4<f32>. Read the inputs with image(at) and second(at); size and time are given. Declare controls in comments: // slider NAME MIN MAX DEFAULT or // colour NAME R G B, then use u.NAME.");
    egui::ScrollArea::vertical()
        .id_salt("shader-code")
        .max_height(320.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut buffer.text)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY)
                    .char_limit(veylvik_shader::MAX_SOURCE),
            );
        });
    if buffer.checked != buffer.text {
        buffer.problem = veylvik_render::custom::check(&buffer.text).err();
        buffer.checked = buffer.text.clone();
    }
    let edited = buffer.text != buffer.stored;
    ui.horizontal_wrapped(|ui| match &buffer.problem {
        Some(problem) => {
            ui.colored_label(egui::Color32::LIGHT_RED, problem);
        }
        None if edited => {
            ui.colored_label(egui::Color32::from_rgb(140, 210, 150), "Compiles");
        }
        None => {}
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(edited && buffer.problem.is_none(), egui::Button::new("Use this code"))
            .on_hover_text("Run the edited code. Controls it declares for the first time start at their defaults; the others keep their values.")
            .clicked()
        {
            // Values of controls that are new, or moved to another slot, start at
            // their declared defaults.
            let before = veylvik_shader::controls(&shader.code).unwrap_or_default();
            let after = veylvik_shader::controls(&buffer.text).unwrap_or_default();
            let defaults = veylvik_shader::defaults(&after);
            let place = |c: &Control| match c {
                Control::Slider { name, slot, .. } => (name.clone(), *slot, 1),
                Control::Colour { name, slot, .. } => (name.clone(), *slot, 3),
            };
            let kept: Vec<usize> = after
                .iter()
                .map(place)
                .filter(|a| before.iter().map(place).any(|b| b == *a))
                .flat_map(|(_, slot, len)| slot..slot + len)
                .collect();
            shader.code = buffer.text.clone();
            for (i, value) in values(shader).into_iter().enumerate() {
                if !kept.contains(&i) {
                    *value = defaults[i].into();
                }
            }
            buffer.stored = shader.code.clone();
            changed = true;
        }
        if ui
            .add_enabled(edited, egui::Button::new("Revert"))
            .on_hover_text("Go back to the code the node is running")
            .clicked()
        {
            buffer.text = buffer.stored.clone();
            buffer.checked.clear();
        }
    });
    ui.data_mut(|d| d.insert_temp(id, buffer));
    changed
}
