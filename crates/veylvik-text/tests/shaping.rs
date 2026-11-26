use std::sync::Arc;
use veylvik_text::*;
fn font() -> Font {
    Font::from_bytes(Arc::from(epaint_default_fonts::UBUNTU_LIGHT), 0).unwrap()
}
#[test]
fn shaping_preserves_ligatures_kerning_clusters_and_whitespace() {
    let font = font();
    let options = RunOptions::default();
    let ligature = font.shape_run("ffi", &options).unwrap();
    assert!(ligature.glyphs.len() < 3);
    assert_eq!(ligature.glyphs[0].cluster, 0);
    assert!(ligature.glyphs.iter().all(|g| !g.missing));
    ligature.outline.unwrap().validate().unwrap();
    let joined = font.shape_run("AV", &options).unwrap();
    let separate = font.shape_run("A", &options).unwrap().advance[0]
        + font.shape_run("V", &options).unwrap().advance[0];
    assert!(joined.advance[0] < separate);
    let text = "e\u{301} Å";
    let combined = font.shape_run(text, &options).unwrap();
    assert!(combined.glyphs.len() < text.chars().count());
    for glyph in &combined.glyphs {
        assert!(text.is_char_boundary(glyph.cluster as usize));
    }
    let whitespace = font.shape_run("   ", &options).unwrap();
    assert!(whitespace.outline.is_none());
    assert!(whitespace.advance[0] > 0.0);
    assert!(
        whitespace
            .glyphs
            .iter()
            .all(|g| !g.has_outline && !g.missing)
    );
    let empty = font.shape_run("", &options).unwrap();
    assert!(empty.glyphs.is_empty());
    assert!(empty.outline.is_none());
    let missing = font.shape_run("\u{10ffff}", &options).unwrap();
    assert!(missing.glyphs[0].missing);
}
#[test]
fn explicit_direction_scale_and_origin_are_preserved() {
    let font = font();
    let base = font.shape_run("abc", &RunOptions::default()).unwrap();
    let rtl = font
        .shape_run(
            "abc",
            &RunOptions {
                direction: Direction::RightToLeft,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        rtl.glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(),
        vec![2, 1, 0]
    );
    let scaled = font
        .shape_run(
            "abc",
            &RunOptions {
                font_size: 128.0,
                origin: [40.0, 200.0],
                language: Some("en".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!((scaled.advance[0] - base.advance[0] * 2.0).abs() < 0.001);
    assert_eq!(scaled.glyphs[0].origin, [40.0, 200.0]);
    assert_eq!(
        base.glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
        scaled.glyphs.iter().map(|g| g.id).collect::<Vec<_>>()
    );
    let a = base.outline.unwrap();
    let b = scaled.outline.unwrap();
    assert_eq!(a.commands.len(), b.commands.len());
    use veylvik_document::vector::PathCommand;
    let (PathCommand::Move { to: a }, PathCommand::Move { to: b }) = (a.commands[0], b.commands[0])
    else {
        panic!("expected contour starts")
    };
    assert!((b[0] - (a[0] * 2.0 + 40.0)).abs() < 0.001);
    assert!((b[1] - ((a[1] - 64.0) * 2.0 + 200.0)).abs() < 0.001);
}
#[test]
fn malformed_fonts_and_unbounded_runs_are_rejected() {
    assert!(Font::from_bytes(Arc::from([0u8; 100]), 0).is_err());
    assert!(Font::from_bytes(Arc::from(epaint_default_fonts::UBUNTU_LIGHT), 999).is_err());
    let font = font();
    for text in [
        "hello\nworld".to_string(),
        "a\tb".into(),
        "a\u{202e}b".into(),
        "x".repeat(MAX_TEXT_BYTES + 1),
        " ".repeat(MAX_GLYPHS + 1),
        "W".repeat(MAX_GLYPHS),
    ] {
        assert!(font.shape_run(&text, &RunOptions::default()).is_err());
    }
    for size in [0.0, 4097.0, f32::NAN, f32::INFINITY] {
        assert!(
            font.shape_run(
                "a",
                &RunOptions {
                    font_size: size,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    assert!(
        font.shape_run(
            "a",
            &RunOptions {
                origin: [f32::NAN, 0.0],
                ..Default::default()
            }
        )
        .is_err()
    );
}
