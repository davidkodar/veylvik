use crate::*;
use veylvik_document::text::TextSpan;
/// Shapes each contiguous style run, then lets the shared paragraph layout align it.
pub(crate) fn shape(
    font: &Font,
    text: &str,
    original: &str,
    options: &TextOptions,
    spans: &[TextSpan],
) -> Result<ShapedText> {
    if spans.is_empty() {
        return font.shape_lines(text, options);
    }
    let fonts: Vec<_> = spans
        .iter()
        .map(|span| {
            span.font
                .as_ref()
                .map(|font| {
                    let (bytes, index) = font_data(font)?;
                    Font::from_bytes(bytes, index)
                })
                .transpose()
        })
        .collect::<Result<_>>()?;
    // Wrapping changes whitespace only. Map shaped offsets back to authored text.
    let mut original_chars = original.char_indices().peekable();
    let mut mapping = std::collections::BTreeMap::new();
    for (offset, c) in text.char_indices() {
        while let Some(&(i, o)) = original_chars.peek() {
            original_chars.next();
            if o == c || (o.is_whitespace() && c.is_whitespace()) {
                mapping.insert(offset, i);
                break;
            }
            if !o.is_whitespace() {
                return Err(error("Unable to map styled text layout"));
            }
        }
    }
    font.shape_lines_with(text, options, |line, run, offset| {
        let style_at = |byte| {
            mapping.get(&(offset + byte)).and_then(|original| {
                spans
                    .iter()
                    .position(|span| *original >= span.start && *original < span.end)
            })
        };
        let mut chunks = Vec::new();
        let mut start = 0;
        let mut current = style_at(0);
        for (byte, _) in line.char_indices() {
            let style = style_at(byte);
            if style != current {
                chunks.push((start, byte, current));
                start = byte;
                current = style;
            }
        }
        chunks.push((start, line.len(), current));
        let mut glyphs = Vec::new();
        let mut commands = Vec::new();
        let mut advance = 0.0;
        for (start, end, index) in chunks {
            if start == end {
                continue;
            }
            let mut options = run.clone();
            options.origin[0] += advance;
            let selected = index.and_then(|i| fonts[i].as_ref()).unwrap_or(font);
            if let Some(i) = index {
                options.font_size = (options.font_size * spans[i].scale).clamp(1.0, 1024.0);
            }
            let mut shaped = selected.shape_run(&line[start..end], &options)?;
            for glyph in &mut shaped.glyphs {
                glyph.cluster += start as u32;
                glyph.commands.start += commands.len();
                glyph.commands.end += commands.len();
            }
            advance += shaped.advance[0];
            glyphs.extend(shaped.glyphs);
            if let Some(outline) = shaped.outline {
                commands.extend(outline.commands);
            }
        }
        Ok(ShapedRun {
            glyphs,
            advance: [advance, 0.0],
            outline: (!commands.is_empty()).then_some(PathMask {
                fill_rule: FillRule::NonZero,
                commands,
            }),
        })
    })
}
