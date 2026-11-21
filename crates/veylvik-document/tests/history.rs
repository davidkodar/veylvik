use veylvik_document::{Command, Composition, Document, NodeId, demo, limits::MAX_DOCUMENT_BYTES};
#[test]
fn canonical_size_matches_saved_bytes_and_oversized_edits_are_atomic() {
    let original = demo();
    assert_eq!(
        original.encoded_size().unwrap(),
        serde_json::to_vec_pretty(&original).unwrap().len()
    );
    let mut document = Document::new(original.clone()).unwrap();
    document
        .apply(
            0,
            &[Command::Resize {
                width: 100,
                height: 50,
            }],
        )
        .unwrap();
    let before = serde_json::to_value(document.snapshot()).unwrap();
    let history = document.history();
    let mut node = original.nodes[&original.output].clone();
    // Escaping doubles this label's canonical size; do not count raw string bytes.
    node.label = "\n".repeat(MAX_DOCUMENT_BYTES / 2);
    assert!(
        document
            .apply(
                1,
                &[
                    Command::Resize {
                        width: 2,
                        height: 2
                    },
                    Command::Put {
                        node: Box::new(node.clone())
                    }
                ]
            )
            .is_err()
    );
    assert_eq!(document.revision(), 1);
    assert_eq!(serde_json::to_value(document.snapshot()).unwrap(), before);
    assert_eq!(document.history().retained_bytes, history.retained_bytes);
    let mut oversized = original;
    oversized.nodes.insert(node.id, node);
    let path = std::env::temp_dir().join(format!("veylvik-size-{}.veylvik", NodeId::new_v4()));
    std::fs::write(&path, b"existing file").unwrap();
    assert!(oversized.save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing file");
    std::fs::remove_file(path).unwrap();
    document.undo(1).unwrap();
    assert_ne!(document.snapshot().width, 100);
}
#[test]
fn retention_bounds_both_history_directions_and_branching() {
    let mut composition = demo();
    let id = composition.output;
    composition.nodes.get_mut(&id).unwrap().label = "a".repeat(2048);
    let limit = composition.encoded_size().unwrap() * 3;
    let mut document = Document::with_history_budget(composition, limit).unwrap();
    for index in 0..12 {
        let mut node = document.snapshot().nodes[&id].clone();
        node.label = format!("{index:02}{}", "b".repeat(2048));
        document
            .apply(
                document.revision(),
                &[Command::Put {
                    node: Box::new(node),
                }],
            )
            .unwrap();
        assert!(document.history().retained_bytes <= limit);
    }
    assert!((1..=3).contains(&document.history().undo_steps));
    let latest = document.snapshot().nodes[&id].label.clone();
    let mut count = 0;
    while document.history().undo_steps > 0 {
        let revision = document.revision();
        document.undo(revision).unwrap();
        assert_eq!(document.revision(), revision + 1);
        assert!(document.history().retained_bytes <= limit);
        count += 1;
    }
    assert!((1..=3).contains(&count));
    while document.history().redo_steps > 0 {
        document.redo(document.revision()).unwrap();
        assert!(document.history().retained_bytes <= limit);
    }
    assert_eq!(document.snapshot().nodes[&id].label, latest);
    document.undo(document.revision()).unwrap();
    document
        .apply(
            document.revision(),
            &[Command::Resize {
                width: 80,
                height: 40,
            }],
        )
        .unwrap();
    assert_eq!(document.history().redo_steps, 0);
    assert!(document.history().retained_bytes <= limit);
    assert!(document.redo(document.revision()).is_err());
}
#[test]
fn history_can_be_disabled_and_step_limit_still_applies() {
    let mut disabled = Document::with_history_budget(demo(), 0).unwrap();
    disabled
        .apply(
            0,
            &[Command::Resize {
                width: 10,
                height: 10,
            }],
        )
        .unwrap();
    assert_eq!(disabled.history().undo_steps, 0);
    assert!(disabled.undo(1).is_err());
    let mut document = Document::new(demo()).unwrap();
    for i in 0..80 {
        document
            .apply(
                document.revision(),
                &[Command::Resize {
                    width: 100 + i,
                    height: 50,
                }],
            )
            .unwrap();
    }
    assert_eq!(document.history().undo_steps, 64);
    let path = std::env::temp_dir().join(format!("veylvik-bounded-{}.veylvik", NodeId::new_v4()));
    document.snapshot().save(&path).unwrap();
    assert_eq!(Composition::load(&path).unwrap().width, 179);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn every_revision_says_whether_it_was_a_new_step_an_undo_or_a_redo() {
    use veylvik_document::ChangeKind::{Edit, Redo, Undo};
    let mut document = Document::new(demo()).unwrap();
    let resize = |width| Command::Resize { width, height: 50 };
    document.apply(0, &[resize(100)]).unwrap();
    document.apply(1, &[resize(200)]).unwrap();
    document.undo(2).unwrap();
    document.redo(3).unwrap();
    // A failed edit makes no revision and no change.
    assert!(document.apply(0, &[resize(300)]).is_err());
    let changes = document.take_changes();
    let kinds: Vec<_> = changes.iter().map(|c| (c.kind, c.revision)).collect();
    assert_eq!(kinds, [(Edit, 1), (Edit, 2), (Undo, 3), (Redo, 4)]);
    // The undo and the redo name the step they moved over: the second edit.
    assert!(!changes[1].label.is_empty());
    assert_eq!(changes[2].label, changes[1].label);
    assert_eq!(changes[3].label, changes[1].label);
    assert!(document.take_changes().is_empty(), "taken twice");
    // Nobody taking them keeps only the last 256.
    for n in 0..300 {
        document
            .apply(document.revision(), &[resize(10 + n)])
            .unwrap();
    }
    let kept = document.take_changes();
    assert_eq!(kept.len(), 256);
    assert_eq!(kept.last().unwrap().revision, document.revision());
}
