use crate::*;
use veylvik_document::{Operation, TextAlignment, TextFont, Time};
/// Resolve editable typography to contours at an exact timeline time.
pub fn outline(operation: &Operation, time: Time) -> Result<PathMask> {
    let Operation::Text {
        content,
        font,
        alignment,
        rtl,
        language,
        size,
        x,
        y,
        tracking,
        line_height,
        rotation,
        scale_x,
        scale_y,
        wrap_width,
        underline,
        features,
        ..
    } = operation
    else {
        return Err(error("Expected editable text"));
    };
    operation.validate().map_err(error)?;
    let (bytes, index): (Arc<[u8]>, u32) = match font {
        TextFont::Sans {} => (
            Arc::from(include_bytes!("../assets/fonts/LiberationSans-Regular.ttf").as_slice()),
            0,
        ),
        TextFont::Display => (
            Arc::from(include_bytes!("../assets/fonts/LiberationSans-Bold.ttf").as_slice()),
            0,
        ),
        TextFont::Italic {} => (
            Arc::from(include_bytes!("../assets/fonts/LiberationSans-Italic.ttf").as_slice()),
            0,
        ),
        TextFont::BoldItalic {} => (
            Arc::from(include_bytes!("../assets/fonts/LiberationSans-BoldItalic.ttf").as_slice()),
            0,
        ),
        TextFont::Mono {} => (Arc::from(epaint_default_fonts::HACK_REGULAR), 0),
        TextFont::Embedded { index, .. } => {
            (Arc::from(font.bytes().map_err(error)?.unwrap()), *index)
        }
    };
    let font = Font::from_bytes(bytes, index)?;
    let sample = |value: &veylvik_document::Scalar| value.at(time).map_err(error);
    let size = sample(size)?;
    let origin = [sample(x)?, sample(y)?];
    let features: Vec<harfrust::Feature> = features
        .split([',', ' '])
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().map_err(error))
        .collect::<Result<_>>()?;
    let options = TextOptions {
        run: RunOptions {
            font_size: size,
            origin,
            tracking: sample(tracking)?,
            features,
            direction: if *rtl {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            },
            language: if language.is_empty() {
                None
            } else {
                Some(language.clone())
            },
        },
        alignment: match alignment {
            TextAlignment::Left => Alignment::Left,
            TextAlignment::Center => Alignment::Center,
            TextAlignment::Right => Alignment::Right,
        },
        line_height: Some(size * sample(line_height)?),
    };
    let width = sample(wrap_width)?;
    let wrapped = if width > 0.0 {
        wrap(&font, content, &options.run, width)?
    } else {
        content.clone()
    };
    let shaped = font.shape_lines(&wrapped, &options)?;
    if shaped.glyphs.iter().any(|g| g.missing) {
        return Err(error(
            "This font is missing characters in the text. Choose a font that supports them.",
        ));
    }
    let mut path = shaped.outline.unwrap_or_else(|| PathMask {
        fill_rule: FillRule::NonZero,
        commands: vec![
            PathCommand::Move { to: [0.0; 2] },
            PathCommand::Line { to: [0.0; 2] },
            PathCommand::Line { to: [0.0; 2] },
            PathCommand::Close {},
        ],
    });
    if *underline {
        for (i, line) in wrapped.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let advance = font.shape_run(line, &options.run)?.advance[0];
            let start = origin[0]
                - match alignment {
                    TextAlignment::Left => 0.0,
                    TextAlignment::Center => advance * 0.5,
                    TextAlignment::Right => advance,
                };
            let y = origin[1] + i as f32 * options.line_height.unwrap() + size * 0.1;
            path.commands.extend([
                PathCommand::Move { to: [start, y] },
                PathCommand::Line {
                    to: [start + advance, y],
                },
                PathCommand::Line {
                    to: [start + advance, y + size / 18.0],
                },
                PathCommand::Line {
                    to: [start, y + size / 18.0],
                },
                PathCommand::Close {},
            ]);
        }
    }
    let angle = sample(rotation)?.to_radians();
    let (sin, cos) = angle.sin_cos();
    let sx = sample(scale_x)?;
    let sy = sample(scale_y)?;
    let point = |p: [f32; 2]| {
        let dx = (p[0] - origin[0]) * sx;
        let dy = (p[1] - origin[1]) * sy;
        [
            origin[0] + cos * dx - sin * dy,
            origin[1] + sin * dx + cos * dy,
        ]
    };
    for command in &mut path.commands {
        match command {
            PathCommand::Move { to } | PathCommand::Line { to } => *to = point(*to),
            PathCommand::Quad { control, to } => {
                *control = point(*control);
                *to = point(*to);
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                *control1 = point(*control1);
                *control2 = point(*control2);
                *to = point(*to);
            }
            PathCommand::Close {} => {}
        }
    }
    path.validate().map_err(error)?;
    Ok(path)
}
// Word wrapping preserves shaping of complete words. An overlong word is kept
// intact rather than splitting a ligature or a combining sequence.
fn wrap(font: &Font, text: &str, options: &RunOptions, width: f32) -> Result<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let next = if line.is_empty() {
                word.into()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && font.shape_run(&next, options)?.advance[0].abs() > width {
                lines.push(line);
                line = word.into();
            } else {
                line = next;
            }
            if lines.len() >= MAX_LINES {
                return Err(error("Wrapped text exceeds 64 lines"));
            }
        }
        lines.push(line);
    }
    if lines.len() > MAX_LINES {
        return Err(error("Wrapped text exceeds 64 lines"));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text() -> Operation {
        // The registry is also the source of defaults used by MCP and Studio.
        let node = veylvik_document::presets::showcase::build("gold_text")
            .unwrap()
            .composition
            .nodes
            .into_values()
            .find(|n| matches!(n.operation, Operation::Text { .. }))
            .unwrap();
        node.operation
    }
    #[test]
    fn typography_is_portable_and_transformable_without_losing_wording() {
        let mut op = text();
        let original = outline(&op, Time::new(0, 1).unwrap()).unwrap();
        if let Operation::Text { scale_x, .. } = &mut op {
            *scale_x = (-1.0).into();
        }
        assert_ne!(original, outline(&op, Time::new(0, 1).unwrap()).unwrap());
        if let Operation::Text { content, font, .. } = &mut op {
            *content = "Åffi\nVeylvik".into();
            *font = TextFont::embedded(
                "Portable font".into(),
                0,
                include_bytes!("../assets/fonts/LiberationSans-Bold.ttf"),
            )
            .unwrap();
        }
        outline(&op, Time::new(0, 1).unwrap()).unwrap();
        if let Operation::Text { font, .. } = &mut op {
            *font = TextFont::Display;
        }
        let bundled = outline(&op, Time::new(0, 1).unwrap()).unwrap();
        if let Operation::Text {
            underline,
            wrap_width,
            features,
            ..
        } = &mut op
        {
            *underline = true;
            *wrap_width = 240.0.into();
            *features = "kern=0, liga=0".into();
        }
        assert_ne!(bundled, outline(&op, Time::new(0, 1).unwrap()).unwrap());
        if let Operation::Text { content, .. } = &mut op {
            *content = String::new();
        }
        outline(&op, Time::new(0, 1).unwrap())
            .unwrap()
            .validate()
            .unwrap();
        if let Operation::Text { content, .. } = &mut op {
            *content = "\u{10ffff}".into();
        }
        assert!(
            outline(&op, Time::new(0, 1).unwrap())
                .unwrap_err()
                .to_string()
                .contains("missing")
        );
    }
}
