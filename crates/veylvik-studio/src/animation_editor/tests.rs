use super::*;
use veylvik_document::{Command, Document};
fn event(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
fn draw(
    ctx: &egui::Context,
    editor: &mut Editor,
    doc: &Composition,
    events: Vec<egui::Event>,
) -> (Vec<Action>, Vec<Pos2>) {
    let mut actions = Vec::new();
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 320.0))),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                editor.canvas(
                    ui,
                    &CanvasContext {
                        doc,
                        revision: 0,
                        current: 0,
                        now: Time::new(0, 1).unwrap(),
                        focus: egui::Id::new("test"),
                        range: [0.0, 96.0],
                    },
                    Track {
                        node: &doc.nodes[&doc.output],
                        parameter: "stops",
                        height: 240.0,
                    },
                    &mut actions,
                );
            });
        },
    );
    let circles = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Circle(c) => Some(c.center),
            _ => None,
        })
        .collect();
    (actions, circles)
}
#[test]
fn workspace_uses_native_clipboard_events_and_keeps_keyboard_focus() {
    let mut doc = veylvik_document::motion_demo();
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("stops")
        .unwrap() = Scalar::Linear {
        keys: [0, 24]
            .map(|ticks| Key {
                time: Time::new(ticks, 24).unwrap(),
                value: ticks as f32 / 24.0,
            })
            .into(),
    };
    let ctx = egui::Context::default();
    let mut editor = Editor {
        visible: true,
        ..Default::default()
    };
    let draw = |editor: &mut Editor, events, enabled| {
        let mut actions = Vec::new();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 400.0))),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    actions = editor.show(ui, &doc, 0, doc.output, 48);
                });
            },
        );
        (actions, output)
    };
    draw(&mut editor, vec![], true);
    editor.selection = [0, 1].into();
    ctx.memory_mut(|m| m.request_focus(editor.focus.unwrap()));
    let (actions, output) = draw(&mut editor, vec![egui::Event::Copy], true);
    assert!(actions.is_empty());
    assert!(editor.has_focus(&ctx));
    let text = output
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    let (actions, _) = draw(&mut editor, vec![egui::Event::Paste(text.clone())], true);
    assert_eq!(actions.len(), 1);
    let Action::Commit { scalar, .. } = &actions[0] else {
        panic!("expected paste")
    };
    assert_eq!(
        scalar
            .keys()
            .unwrap()
            .iter()
            .map(|k| frame_number(k.time, doc.frame_rate) as i64)
            .collect::<Vec<_>>(),
        [0, 24, 48, 72]
    );
    let (actions, _) = draw(
        &mut editor,
        vec![egui::Event::Paste("unrelated clipboard text".into())],
        true,
    );
    assert!(actions.is_empty());
    assert!(editor.error.is_some());
    let (actions, _) = draw(&mut editor, vec![egui::Event::Paste(text)], false);
    assert!(actions.is_empty());
}
#[test]
fn native_pointer_drag_is_one_atomic_edit_and_collision_is_not_an_edit() {
    let mut doc = veylvik_document::motion_demo();
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("stops")
        .unwrap() = Scalar::Smooth {
        keys: [(0, -1.0), (48, 1.0), (96, 0.0)]
            .map(|(ticks, value)| Key {
                time: Time::new(ticks, 24).unwrap(),
                value,
            })
            .into(),
    };
    let mut editor = Editor {
        active: Some((doc.output, "stops".into())),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let (_, points) = draw(&ctx, &mut editor, &doc, vec![]);
    assert_eq!(points.len(), 3);
    let (actions, _) = draw(
        &ctx,
        &mut editor,
        &doc,
        vec![
            egui::Event::PointerMoved(points[0]),
            event(points[0], true),
            event(points[0], false),
        ],
    );
    assert!(actions.is_empty());
    assert_eq!(editor.selection, [0].into());
    assert!(!editor.dragging());
    let start = points[1];
    let target = start + Vec2::new((points[2].x - points[0].x) / 8.0, 0.0);
    let (actions, _) = draw(
        &ctx,
        &mut editor,
        &doc,
        vec![
            egui::Event::PointerMoved(start),
            event(start, true),
            egui::Event::PointerMoved(target),
        ],
    );
    assert!(actions.is_empty());
    assert!(editor.dragging());
    let (actions, _) = draw(&ctx, &mut editor, &doc, vec![event(target, false)]);
    assert_eq!(actions.len(), 1);
    assert!(!editor.dragging());
    let Action::Commit { scalar, .. } = &actions[0] else {
        panic!("expected edit")
    };
    assert_eq!(
        frame_number(scalar.keys().unwrap()[1].time, doc.frame_rate),
        60.0
    );
    let mut document = Document::new(doc.clone()).unwrap();
    let before = serde_json::to_value(document.snapshot()).unwrap();
    document
        .apply(
            0,
            &[Command::SetParameter {
                id: doc.output,
                parameter: "stops".into(),
                value: serde_json::to_value(scalar).unwrap(),
            }],
        )
        .unwrap();
    assert_eq!(document.history().undo_steps, 1);
    document.undo(1).unwrap();
    assert_eq!(serde_json::to_value(document.snapshot()).unwrap(), before);
    draw(
        &ctx,
        &mut editor,
        &doc,
        vec![
            egui::Event::PointerMoved(start),
            event(start, true),
            egui::Event::PointerMoved(points[2]),
        ],
    );
    let (actions, _) = draw(&ctx, &mut editor, &doc, vec![event(points[2], false)]);
    assert!(actions.is_empty());
    assert!(editor.error.is_some());
}
