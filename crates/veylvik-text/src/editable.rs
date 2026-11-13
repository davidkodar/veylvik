use crate::*;
use veylvik_document::{Operation, TextAlignment, TextFont, Time};
/// Resolve editable typography to contours at an exact timeline time.
pub fn outline(operation: &Operation, time: Time) -> Result<PathMask> {
    let Operation::Text(veylvik_document::nodes::Text {
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
        box_height,
        padding,
        typography,
        underline,
        features,
        ..
    }) = operation
    else {
        return Err(error("Expected editable text"));
    };
    operation.validate().map_err(error)?;
    let (bytes, index) = font_data(font)?;
    let font = Font::from_bytes(bytes, index)?;
    let sample = |value: &veylvik_document::Scalar| value.at(time).map_err(error);
    let size = sample(size)?;
    let origin = [sample(x)?, sample(y)?];
    let features: Vec<harfrust::Feature> = features
        .split([',', ' '])
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().map_err(error))
        .collect::<Result<_>>()?;
    let mut options = TextOptions {
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
    let box_width = sample(wrap_width)?;
    let box_height = sample(box_height)?;
    let padding = sample(padding)?;
    let width = (box_width - padding * 2.0).max(1.0);
    let mut wrapped = if box_width > 0.0 {
        wrap(&font, content, &options.run, width)?
    } else {
        content.clone()
    };
    let mut shaped = crate::rich::shape(&font, &wrapped, content, &options, &typography.spans)?;
    if typography.auto_fit && box_width > 0.0 && box_height > 0.0 {
        // Reduce actual font size and reflow, preserving glyph aspect ratio.
        for _ in 0..12 {
            let height = options.run.font_size + shaped.advance[1];
            let factor = (width / shaped.advance[0].max(1.0))
                .min((box_height - padding * 2.0).max(1.0) / height.max(1.0))
                .min(1.0);
            if factor >= 0.999 || options.run.font_size <= 1.0 {
                break;
            }
            options.run.font_size = (options.run.font_size * factor * 0.995).max(1.0);
            options.line_height = Some(options.run.font_size * sample(line_height)?);
            wrapped = wrap(&font, content, &options.run, width)?;
            shaped = crate::rich::shape(&font, &wrapped, content, &options, &typography.spans)?;
        }
    }
    if shaped.glyphs.iter().any(|g| g.missing) {
        return Err(error(
            "This font is missing characters in the text. Choose a font that supports them.",
        ));
    }
    let mut path = shaped.outline.unwrap_or_else(|| PathMask {
        keys: vec![],
        fill_rule: FillRule::NonZero,
        commands: vec![
            PathCommand::Move { to: [0.0; 2] },
            PathCommand::Line { to: [0.0; 2] },
            PathCommand::Line { to: [0.0; 2] },
            PathCommand::Close {},
        ],
    });
    if *underline {
        let old = std::mem::take(&mut path.commands);
        for glyph in &mut shaped.glyphs {
            let start = path.commands.len();
            path.commands
                .extend_from_slice(&old[glyph.commands.clone()]);
            let x = glyph.origin[0];
            let y = glyph.origin[1] + options.run.font_size * 0.1;
            let width = glyph.advance[0];
            let thickness = options.run.font_size / 18.0;
            path.commands.extend([
                PathCommand::Move { to: [x, y] },
                PathCommand::Line { to: [x + width, y] },
                PathCommand::Line {
                    to: [x + width, y + thickness],
                },
                PathCommand::Line {
                    to: [x, y + thickness],
                },
                PathCommand::Close {},
            ]);
            glyph.commands = start..path.commands.len();
        }
        if path.commands.is_empty() {
            path.commands = old;
        }
    }
    let ink = bounds(&path);
    let layout_height = options.run.font_size + shaped.advance[1];
    let anchor = typography.anchor;
    let (shift_x, shift_y) = if box_width > 0.0 && box_height > 0.0 {
        let a = anchor.unwrap_or([0.5, 0.5]);
        let left = origin[0] - box_width * a[0];
        let top = origin[1] - box_height * a[1];
        let align = match alignment {
            TextAlignment::Left => 0.0,
            TextAlignment::Center => 0.5,
            TextAlignment::Right => 1.0,
        };
        let vertical = match typography.vertical {
            veylvik_document::text::Vertical::Top => 0.0,
            veylvik_document::text::Vertical::Middle => 0.5,
            veylvik_document::text::Vertical::Bottom => 1.0,
        };
        (
            left + padding + width * align - origin[0],
            top + padding + (box_height - padding * 2.0 - layout_height).max(0.0) * vertical
                - ink[1],
        )
    } else if let Some(a) = anchor {
        (
            origin[0] - (ink[0] + (ink[2] - ink[0]) * a[0]),
            origin[1] - (ink[1] + (ink[3] - ink[1]) * a[1]),
        )
    } else {
        (0.0, 0.0)
    };
    let seconds = time.seconds().map_err(error)?;
    let mut clusters: Vec<_> = shaped.glyphs.iter().map(|g| g.cluster).collect();
    clusters.sort_unstable();
    clusters.dedup();
    let unit_of = |glyph: &crate::Glyph| {
        let prefix = &wrapped[..glyph.cluster as usize];
        match typography.unit {
            veylvik_document::text::TextUnit::Character => {
                clusters.binary_search(&glyph.cluster).unwrap_or(0)
            }
            veylvik_document::text::TextUnit::Word => prefix
                .split_whitespace()
                .count()
                .saturating_sub(usize::from(
                    prefix.chars().last().is_some_and(|c| !c.is_whitespace()),
                )),
            veylvik_document::text::TextUnit::Line => {
                prefix.bytes().filter(|b| *b == b'\n').count()
            }
            veylvik_document::text::TextUnit::Whole => 0,
        }
    };
    let mut pivots = std::collections::BTreeMap::<usize, [f32; 4]>::new();
    for glyph in &shaped.glyphs {
        let b = pivots.entry(unit_of(glyph)).or_insert([
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ]);
        b[0] = b[0].min(glyph.origin[0]);
        b[1] = b[1].min(glyph.origin[1] - options.run.font_size);
        b[2] = b[2].max(glyph.origin[0] + glyph.advance[0]);
        b[3] = b[3].max(glyph.origin[1]);
    }
    for glyph in &shaped.glyphs {
        let unit = unit_of(glyph);
        let p = typography.progress(seconds, unit);
        if p >= 1.0 {
            continue;
        }
        use veylvik_document::text::TextMotion;
        let b = pivots[&unit];
        let center = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
        for command in &mut path.commands[glyph.commands.clone()] {
            map_command(command, |point| {
                let mut d = [point[0] - center[0], point[1] - center[1]];
                match typography.motion {
                    TextMotion::Typewriter if p < 1.0 => d = [0.0, 0.0],
                    TextMotion::Rise => {
                        d[0] *= p;
                        d[1] = d[1] * p + (1.0 - p) * options.run.font_size;
                    }
                    TextMotion::Scale => {
                        d[0] *= p;
                        d[1] *= p;
                    }
                    TextMotion::Spin => {
                        let (sin, cos) = ((1.0 - p) * std::f32::consts::PI).sin_cos();
                        d = [p * (d[0] * cos - d[1] * sin), p * (d[0] * sin + d[1] * cos)];
                    }
                    _ => {}
                }
                [center[0] + d[0], center[1] + d[1]]
            });
        }
    }
    let angle = sample(rotation)?.to_radians();
    let (sin, cos) = angle.sin_cos();
    let sx = sample(scale_x)?;
    let sy = sample(scale_y)?;
    let point = |p: [f32; 2]| {
        let dx = (p[0] + shift_x - origin[0]) * sx;
        let dy = (p[1] + shift_y - origin[1]) * sy;
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
            .find(|n| {
                matches!(
                    n.operation,
                    Operation::Text(veylvik_document::nodes::Text { .. })
                )
            })
            .unwrap();
        node.operation
    }
    #[test]
    fn typography_is_portable_and_transformable_without_losing_wording() {
        let mut op = text();
        let original = outline(&op, Time::new(0, 1).unwrap()).unwrap();
        if let Operation::Text(veylvik_document::nodes::Text { scale_x, .. }) = &mut op {
            *scale_x = (-1.0).into();
        }
        assert_ne!(original, outline(&op, Time::new(0, 1).unwrap()).unwrap());
        if let Operation::Text(veylvik_document::nodes::Text { content, font, .. }) = &mut op {
            *content = "Åffi\nVeylvik".into();
            **font = TextFont::embedded(
                "Portable font".into(),
                0,
                include_bytes!("../assets/fonts/LiberationSans-Bold.ttf"),
            )
            .unwrap();
        }
        outline(&op, Time::new(0, 1).unwrap()).unwrap();
        if let Operation::Text(veylvik_document::nodes::Text { font, .. }) = &mut op {
            **font = TextFont::Display;
        }
        let bundled = outline(&op, Time::new(0, 1).unwrap()).unwrap();
        if let Operation::Text(veylvik_document::nodes::Text {
            underline,
            wrap_width,
            features,
            ..
        }) = &mut op
        {
            *underline = true;
            *wrap_width = 240.0.into();
            *features = "kern=0, liga=0".into();
        }
        assert_ne!(bundled, outline(&op, Time::new(0, 1).unwrap()).unwrap());
        if let Operation::Text(veylvik_document::nodes::Text { content, .. }) = &mut op {
            *content = String::new();
        }
        outline(&op, Time::new(0, 1).unwrap())
            .unwrap()
            .validate()
            .unwrap();
        if let Operation::Text(veylvik_document::nodes::Text { content, .. }) = &mut op {
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

fn map_command(command: &mut PathCommand, point: impl Fn([f32; 2]) -> [f32; 2]) {
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
/// Conservative contour bounds, including Bezier control points.
pub fn bounds(path: &PathMask) -> [f32; 4] {
    mapped_bounds(path, |p| p)
}
pub fn mapped_bounds(path: &PathMask, map: impl Fn([f32; 2]) -> [f32; 2]) -> [f32; 4] {
    let mut b = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    let mut add = |p| {
        let p = map(p);
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    };
    for c in &path.commands {
        match c {
            PathCommand::Move { to } | PathCommand::Line { to } => add(*to),
            PathCommand::Quad { control, to } => {
                add(*control);
                add(*to);
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                add(*control1);
                add(*control2);
                add(*to);
            }
            PathCommand::Close {} => {}
        }
    }
    b
}

pub fn font_data(font: &TextFont) -> Result<(Arc<[u8]>, u32)> {
    Ok(match font {
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
    })
}

#[cfg(test)]
mod authoring_tests {
    use super::*;
    fn text() -> Operation {
        veylvik_document::presets::showcase::build("gold_text")
            .unwrap()
            .composition
            .nodes
            .into_values()
            .find(|n| {
                matches!(
                    n.operation,
                    Operation::Text(veylvik_document::nodes::Text { .. })
                )
            })
            .unwrap()
            .operation
    }
    #[test]
    fn layout_boxes_fit_and_anchor_without_changing_legacy_baseline() {
        let mut op = text();
        let time = Time::new(0, 1).unwrap();
        let before = bounds(&outline(&op, time).unwrap());
        if let Operation::Text(veylvik_document::nodes::Text {
            typography, x, y, ..
        }) = &mut op
        {
            typography.anchor = Some([0.5, 0.5]);
            *x = 300.0.into();
            *y = 200.0.into();
        }
        let centered = bounds(&outline(&op, time).unwrap());
        assert!(((centered[0] + centered[2]) * 0.5 - 300.0).abs() < 0.01);
        assert!(((centered[1] + centered[3]) * 0.5 - 200.0).abs() < 0.01);
        assert!((centered[2] - centered[0] - (before[2] - before[0])).abs() < 0.01);
        if let Operation::Text(veylvik_document::nodes::Text {
            wrap_width,
            box_height,
            padding,
            typography,
            ..
        }) = &mut op
        {
            *wrap_width = 200.0.into();
            *box_height = 80.0.into();
            *padding = 10.0.into();
            typography.auto_fit = true;
        }
        let fitted = bounds(&outline(&op, time).unwrap());
        assert!(fitted[2] - fitted[0] <= 181.0);
        assert!(fitted[3] - fitted[1] <= 61.0);
    }
    #[test]
    fn staged_text_animation_is_stateless_and_preserves_complete_result() {
        let mut op = text();
        let end = outline(&op, Time::new(10, 1).unwrap()).unwrap();
        if let Operation::Text(veylvik_document::nodes::Text { typography, .. }) = &mut op {
            typography.motion = veylvik_document::text::TextMotion::Scale;
        }
        let start = outline(&op, Time::new(0, 1).unwrap()).unwrap();
        assert_ne!(start, end);
        assert_eq!(outline(&op, Time::new(10, 1).unwrap()).unwrap(), end);
        assert_eq!(outline(&op, Time::new(0, 1).unwrap()).unwrap(), start);
    }
}

#[cfg(test)]
mod rich_tests {
    use super::*;
    #[test]
    fn mixed_sizes_and_fonts_survive_wrapping_and_unicode_content() {
        let mut op = veylvik_document::presets::showcase::build("gold_text")
            .unwrap()
            .composition
            .nodes
            .into_values()
            .find(|n| {
                matches!(
                    n.operation,
                    Operation::Text(veylvik_document::nodes::Text { .. })
                )
            })
            .unwrap()
            .operation;
        if let Operation::Text(veylvik_document::nodes::Text {
            content,
            size,
            wrap_width,
            typography,
            ..
        }) = &mut op
        {
            *content = "Åbc title words".into();
            *size = 40.0.into();
            *wrap_width = 180.0.into();
            typography.spans.push(veylvik_document::text::TextSpan {
                start: 0,
                end: 4,
                font: Some(TextFont::Italic {}),
                scale: 1.5,
            });
        }
        let styled = outline(&op, Time::new(0, 1).unwrap()).unwrap();
        styled.validate().unwrap();
        if let Operation::Text(veylvik_document::nodes::Text { typography, .. }) = &mut op {
            typography.spans.clear();
        }
        assert_ne!(styled, outline(&op, Time::new(0, 1).unwrap()).unwrap());
    }
}
