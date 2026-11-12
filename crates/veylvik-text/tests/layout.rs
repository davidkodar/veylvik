use std::sync::Arc;
use veylvik_text::*;
fn font() -> Font {
    Font::from_bytes(Arc::from(epaint_default_fonts::UBUNTU_LIGHT), 0).unwrap()
}
#[test]
fn explicit_lines_preserve_run_shapes_alignment_and_original_clusters() {
    let font = font();
    let mut options = TextOptions::default();
    options.run.origin = [300.0, 100.0];
    options.line_height = Some(90.0);
    let single = font.shape_run("Åffi", &options.run).unwrap();
    let lines = font.shape_lines("Åffi", &options).unwrap();
    assert_eq!(single.outline, lines.outline);
    assert_eq!(single.advance, lines.advance);
    let text = "Åffi\r\n\nAV\n";
    for alignment in [Alignment::Left, Alignment::Center, Alignment::Right] {
        options.alignment = alignment;
        let shaped = font.shape_lines(text, &options).unwrap();
        assert_eq!(shaped.line_count, 4);
        assert_eq!(shaped.advance[1], 270.0);
        let second = font.shape_run("AV", &options.run).unwrap();
        assert!((shaped.advance[0] - single.advance[0].max(second.advance[0])).abs() < 0.001);
        let factor = match alignment {
            Alignment::Left => 0.0,
            Alignment::Center => 0.5,
            Alignment::Right => 1.0,
        };
        for (glyph, expected) in shaped
            .glyphs
            .iter()
            .zip(single.glyphs.iter().chain(second.glyphs.iter()))
        {
            let is_second = glyph.cluster >= 8;
            let shift = if is_second {
                second.advance[0]
            } else {
                single.advance[0]
            } * factor;
            assert!((glyph.origin[0] - (expected.origin[0] - shift)).abs() < 0.001);
            assert!(
                (glyph.origin[1] - (expected.origin[1] + if is_second { 180.0 } else { 0.0 }))
                    .abs()
                    < 0.001
            );
            assert_eq!(
                glyph.cluster,
                expected.cluster + if is_second { 8 } else { 0 }
            );
            assert!(text.is_char_boundary(glyph.cluster as usize));
        }
        shaped.outline.unwrap().validate().unwrap();
    }
    options.run.direction = Direction::RightToLeft;
    let rtl = font.shape_lines("ab\ncd", &options).unwrap();
    assert_eq!(
        rtl.glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(),
        [1, 0, 4, 3]
    );
}
#[test]
fn limits_apply_to_the_entire_text_and_empty_lines_have_layout() {
    let font = font();
    let mut options = TextOptions::default();
    let empty = font.shape_lines("\n\n", &options).unwrap();
    assert!(empty.outline.is_none());
    assert!(empty.glyphs.is_empty());
    assert_eq!(empty.advance, [0.0, 160.0]);
    for text in [
        "\n".repeat(64),
        " ".repeat(4097),
        format!("{}\n{}", " ".repeat(300), " ".repeat(300)),
        "a\rb".into(),
        "a\tb".into(),
    ] {
        assert!(font.shape_lines(&text, &options).is_err());
    }
    let commands = font
        .shape_run("A", &options.run)
        .unwrap()
        .outline
        .unwrap()
        .commands
        .len();
    let line = "A".repeat(4096 / commands / 2 + 1);
    assert!(
        font.shape_lines(&format!("{line}\n{line}\n{line}"), &options)
            .is_err()
    );
    for height in [0.0, -1.0, 4097.0, f32::NAN, f32::INFINITY] {
        options.line_height = Some(height);
        assert!(font.shape_lines("A", &options).is_err());
    }
    options.line_height = Some(4096.0);
    assert!(font.shape_lines(&"\n".repeat(10), &options).is_err());
    options.line_height = None;
    options.run.origin[0] = f32::NAN;
    assert!(font.shape_lines("", &options).is_err());
}
