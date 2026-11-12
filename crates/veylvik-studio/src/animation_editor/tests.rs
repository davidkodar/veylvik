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
                segment: None,
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
                segment: None,
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

#[test]
fn box_selection_moves_multiple_parameters_in_one_transaction() {
    let mut doc = veylvik_document::demo();
    let curve = Scalar::Linear {
        keys: [0, 12, 24]
            .map(|ticks| Key {
                time: Time::new(ticks, 24).unwrap(),
                value: ticks as f32,
                segment: None,
            })
            .into(),
    };
    let node = veylvik_document::Node::new(
        "motion",
        veylvik_document::Operation::Transform {
            x: curve.clone(),
            y: curve,
        },
        &[("image", doc.output)],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    doc.frame_count = 25;
    let ctx = egui::Context::default();
    let mut editor = Editor {
        visible: true,
        range: Some([0.0, 24.0]),
        ..Default::default()
    };
    let draw = |editor: &mut Editor, events| {
        let mut actions = Vec::new();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 500.0))),
                events,
                ..Default::default()
            },
            |ui| {
                actions = editor.show(ui, &doc, 0, doc.output, 0);
            },
        );
        actions
    };
    draw(&mut editor, vec![]);
    let x = editor
        .hit_keys
        .iter()
        .find(|(_, p, i, _)| p == "x" && *i == 1)
        .unwrap()
        .3;
    let y = editor
        .hit_keys
        .iter()
        .find(|(_, p, i, _)| p == "y" && *i == 1)
        .unwrap()
        .3;
    let start = x - Vec2::new(12.0, 8.0);
    let end = y + Vec2::new(12.0, 8.0);
    draw(
        &mut editor,
        vec![egui::Event::PointerMoved(start), event(start, true)],
    );
    draw(&mut editor, vec![egui::Event::PointerMoved(end)]);
    draw(&mut editor, vec![event(end, false)]);
    let selected = editor.selections();
    assert_eq!(selected.len(), 2);
    assert!(selected.values().all(|s| s == &[1].into()));
    let last = editor
        .hit_keys
        .iter()
        .find(|(_, p, i, _)| p == "x" && *i == 2)
        .unwrap()
        .3;
    let target = x + Vec2::new((last.x - x.x) / 2.0, 0.0);
    draw(
        &mut editor,
        vec![
            egui::Event::PointerMoved(x),
            event(x, true),
            egui::Event::PointerMoved(target),
        ],
    );
    let actions = draw(&mut editor, vec![event(target, false)]);
    let edits = actions
        .into_iter()
        .find_map(|a| {
            if let Action::Batch { edits, .. } = a {
                Some(edits)
            } else {
                None
            }
        })
        .expect("one grouped edit");
    assert_eq!(edits.len(), 2);
    let commands: Vec<_> = edits
        .iter()
        .map(|(id, p, s)| {
            assert_eq!(
                frame_number(s.keys().unwrap()[1].time, doc.frame_rate),
                18.0
            );
            Command::SetParameter {
                id: *id,
                parameter: p.clone(),
                value: serde_json::to_value(s).unwrap(),
            }
        })
        .collect();
    let mut document = Document::new(doc.clone()).unwrap();
    document.apply(0, &commands).unwrap();
    assert_eq!(document.history().undo_steps, 1);
    document.undo(1).unwrap();
    assert_eq!(
        serde_json::to_value(document.snapshot()).unwrap(),
        serde_json::to_value(&doc).unwrap()
    );
    assert!(groups::moved_group(&doc, &selected, Time::new(12, 24).unwrap()).is_err());
    editor.install_selection(selected);
    editor.copy_selection(&doc).unwrap();
    assert_eq!(editor.clipboard_tracks.len(), 2);
    let pasted = editor.paste_group(&doc, Time::new(6, 24).unwrap()).unwrap();
    assert_eq!(pasted.len(), 2);
    assert!(pasted.iter().all(|(_, _, s)| s.keys().unwrap().len() == 4));
}

#[test]
fn bezier_handles_use_frozen_plot_coordinates_and_shared_sampler() {
    use veylvik_document::{Handle, Segment};
    let curve = Scalar::Smooth {
        keys: vec![
            Key {
                time: Time::new(0, 1).unwrap(),
                value: 0.0,
                segment: Some(Segment::Bezier {
                    outgoing: Handle {
                        time: 0.25,
                        value: 0.0,
                    },
                    incoming: Handle {
                        time: 0.25,
                        value: 0.0,
                    },
                }),
            },
            Key {
                time: Time::new(2, 1).unwrap(),
                value: 1.0,
                segment: None,
            },
        ],
    };
    let graph = Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 200.0));
    let moved = tangents::moved_handle(
        &curve,
        (0, false),
        Vec2::new(48.0, -50.0),
        graph,
        [0.0, 48.0],
        Time::new(24, 1).unwrap(),
        [0.0, 2.0],
    )
    .unwrap();
    let Some(Segment::Bezier { outgoing, incoming }) = moved.keys().unwrap()[0].segment else {
        panic!()
    };
    assert!((outgoing.time - 0.35).abs() < 1e-6);
    assert_eq!(outgoing.value, 0.5);
    assert_eq!(incoming.value, 0.0);
    let clamped = tangents::moved_handle(
        &curve,
        (0, false),
        Vec2::new(1e6, 0.0),
        graph,
        [0.0, 48.0],
        Time::new(24, 1).unwrap(),
        [0.0, 2.0],
    )
    .unwrap();
    assert!(clamped.validate().is_ok());
    let plot = crate::curve_preview::plot(&moved, Time::new(1, 1).unwrap(), 400.0).unwrap();
    let expected = f64::from(moved.compile().unwrap().sample_segment(0, 0.5).unwrap());
    let middle = plot.points[plot.points.len() / 2];
    assert!(
        (f64::from(middle[1]) - (1.0 - (expected - plot.min) / (plot.max - plot.min))).abs() < 1e-6
    );
}
