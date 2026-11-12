//! Explicit line layout. Each line is one directional/script run; no bidi
//! itemization, automatic wrapping or font fallback is performed.
use crate::*;
pub const MAX_LINES: usize = 64;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Debug, Clone)]
pub struct TextOptions {
    pub run: RunOptions,
    /// Horizontal alignment around the baseline origin's x coordinate.
    pub alignment: Alignment,
    /// Baseline distance in pixels. None uses 1.25 times the font size.
    pub line_height: Option<f32>,
}
impl Default for TextOptions {
    fn default() -> Self {
        Self {
            run: RunOptions::default(),
            alignment: Alignment::Left,
            line_height: None,
        }
    }
}
pub struct ShapedText {
    pub glyphs: Vec<Glyph>,
    pub outline: Option<PathMask>,
    /// Maximum line advance and vertical distance between first/last baselines.
    /// These are layout metrics, not an ink bounding box.
    pub advance: [f32; 2],
    pub line_count: usize,
}
impl Font {
    pub fn shape_lines(&self, text: &str, options: &TextOptions) -> Result<ShapedText> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(error("text exceeds 4096 UTF-8 bytes"));
        }
        let line_count = text.split('\n').count();
        if line_count > MAX_LINES {
            return Err(error("text exceeds 64 lines"));
        }
        let line_height = options.line_height.unwrap_or(options.run.font_size * 1.25);
        if !line_height.is_finite() || !(f32::MIN_POSITIVE..=4096.0).contains(&line_height) {
            return Err(error(
                "line height must be positive and at most 4096 pixels",
            ));
        }
        if options
            .run
            .origin
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 32768.0)
        {
            return Err(error("text origin exceeds ±32768 composition pixels"));
        }
        let mut glyphs = Vec::new();
        let mut commands = Vec::new();
        let mut width = 0.0_f32;
        let mut offset = 0;
        for (index, raw) in text.split('\n').enumerate() {
            // CRLF is one line separator; a standalone carriage return is rejected.
            let line = if index + 1 < line_count {
                raw.strip_suffix('\r').unwrap_or(raw)
            } else {
                raw
            };
            let mut run_options = options.run.clone();
            run_options.origin[1] += index as f32 * line_height;
            if options.alignment != Alignment::Left {
                run_options.origin[0] = 0.0;
            }
            let mut run = self.shape_run(line, &run_options)?;
            let shift = match options.alignment {
                Alignment::Left => 0.0,
                Alignment::Center => options.run.origin[0] - run.advance[0] * 0.5,
                Alignment::Right => options.run.origin[0] - run.advance[0],
            };
            if glyphs.len() + run.glyphs.len() > MAX_GLYPHS {
                return Err(error("text exceeds 512 positioned glyphs"));
            }
            for glyph in &mut run.glyphs {
                glyph.cluster += offset as u32;
                glyph.origin[0] += shift;
                if glyph
                    .origin
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 32768.0)
                {
                    return Err(error("text placement exceeds ±32768 composition pixels"));
                }
            }
            glyphs.extend(run.glyphs);
            width = width.max(run.advance[0].abs());
            if let Some(path) = run.outline {
                if commands.len() + path.commands.len() > MAX_PATH_COMMANDS {
                    return Err(error(
                        "text outline exceeds 4096 path commands; split it into nodes",
                    ));
                }
                let translate = |p: [f32; 2]| [p[0] + shift, p[1]];
                commands.extend(path.commands.into_iter().map(|command| match command {
                    PathCommand::Move { to } => PathCommand::Move { to: translate(to) },
                    PathCommand::Line { to } => PathCommand::Line { to: translate(to) },
                    PathCommand::Quad { control, to } => PathCommand::Quad {
                        control: translate(control),
                        to: translate(to),
                    },
                    PathCommand::Cubic {
                        control1,
                        control2,
                        to,
                    } => PathCommand::Cubic {
                        control1: translate(control1),
                        control2: translate(control2),
                        to: translate(to),
                    },
                    PathCommand::Close {} => PathCommand::Close {},
                }));
            }
            offset += raw.len() + 1;
        }
        let outline = if commands.is_empty() {
            None
        } else {
            let path = PathMask {
                fill_rule: FillRule::NonZero,
                commands,
            };
            path.validate().map_err(error)?;
            Some(path)
        };
        Ok(ShapedText {
            glyphs,
            outline,
            advance: [width, (line_count - 1) as f32 * line_height],
            line_count,
        })
    }
}
