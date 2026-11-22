//! A host draws a composition's published settings with no editor and no GPU: a drag on one gives the setting's new value as commands the document accepts, reported as one gesture.
use eframe::egui;
use veylvik_document::{
    Command, Composition, Document, Node, Operation, Time, blank, groups::Control, registry,
};
use veylvik_editor::controls::{self, ControlsState};

fn of(kind: &str) -> Operation {
    serde_json::from_value(
        registry::TYPES
            .iter()
            .find(|t| t.type_id == kind)
            .unwrap()
            .default_operation(),
    )
    .unwrap()
}
/// A blur over the frame, its radius published as "Softness", and a text's words.
fn composition() -> (Composition, veylvik_document::NodeId) {
    let mut doc = blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
    let blur = Node::new("Soften", of("blur"), &[("image", doc.output)]);
    let text = Node::new("Title", of("text"), &[]);
    let (id, words) = (blur.id, text.id);
    doc.nodes.insert(blur.id, blur);
    doc.nodes.insert(text.id, text);
    doc.output = id;
    doc.published = vec![
        Control::new(&doc, id, "radius", "Softness").unwrap(),
        Control::new(&doc, words, "content", "Words").unwrap(),
    ];
    Document::new(doc.clone()).unwrap();
    (doc, id)
}
struct Frame {
    commands: Vec<Command>,
    dragging: bool,
}
fn frame(
    context: &egui::Context,
    doc: &Composition,
    state: &mut ControlsState,
    events: Vec<egui::Event>,
) -> Frame {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 400.0),
        )),
        events,
        ..Default::default()
    };
    let mut out = None;
    let _ = context.run_ui(input, |ui| {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let response = controls::ui(ui, doc, Time::new(0, 1).unwrap(), state);
            out = Some(Frame {
                commands: response.commands,
                dragging: response.dragging,
            });
        });
    });
    out.unwrap()
}
fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

#[test]
fn a_host_drags_a_published_slider_and_gets_one_gesture_of_commands() {
    let (doc, blur) = composition();
    // Untouched, the panel draws both settings and changes nothing.
    let context = egui::Context::default();
    let mut state = ControlsState::default();
    for _ in 0..2 {
        let quiet = frame(&context, &doc, &mut state, vec![]);
        assert!(quiet.commands.is_empty() && !quiet.dragging);
    }
    // Find the slider's row: press, drag to the right and release there, applying each
    // frame's changes as a host does.
    for y in (8..200).step_by(4).map(|y| y as f32) {
        let context = egui::Context::default();
        let mut state = ControlsState::default();
        let mut document = Document::new(doc.clone()).unwrap();
        let mut step = |events: Vec<egui::Event>, document: &mut Document| {
            let shown = frame(&context, document.snapshot(), &mut state, events);
            if !shown.commands.is_empty() {
                assert!(shown.commands.iter().all(|c| matches!(c, Command::SetParameter { id, parameter, .. } if *id == blur && parameter == "radius")));
                let revision = document.revision();
                document
                    .apply(revision, &shown.commands)
                    .expect("the document takes the change");
            }
            shown
        };
        step(vec![], &mut document);
        step(
            vec![egui::Event::PointerMoved(egui::pos2(110.0, y))],
            &mut document,
        );
        let pressed = step(vec![button(egui::pos2(110.0, y), true)], &mut document);
        let mut changes = pressed.commands.len();
        let mut dragging = Vec::new();
        for x in [140.0, 170.0, 200.0] {
            let moved = step(
                vec![egui::Event::PointerMoved(egui::pos2(x, y))],
                &mut document,
            );
            dragging.push(moved.dragging);
            changes += moved.commands.len();
        }
        if changes == 0 {
            continue;
        }
        let radius = |document: &Document| {
            document.snapshot().nodes[&blur]
                .operation
                .scalar("radius")
                .unwrap()
                .at(Time::new(0, 1).unwrap())
                .unwrap()
        };
        assert_ne!(radius(&document), 4.0, "the drag changed the radius");
        // Held still: nothing new, but the gesture goes on; released: it is over.
        let still = step(vec![], &mut document);
        assert!(still.commands.is_empty(), "{:?}", still.commands);
        dragging.push(still.dragging);
        assert!(dragging.iter().all(|d| *d), "{dragging:?}");
        let released = step(vec![button(egui::pos2(200.0, y), false)], &mut document);
        assert!(!released.dragging);
        let after = step(vec![], &mut document);
        assert!(after.commands.is_empty() && !after.dragging);
        return;
    }
    panic!("no row of the panel moved the slider");
}

#[test]
fn only_settings_the_panel_can_draw_are_offered() {
    assert!(controls::drawable(registry::ParameterKind::Scalar));
    assert!(controls::drawable(registry::ParameterKind::TextContent));
    assert!(!controls::drawable(registry::ParameterKind::Path));
}
