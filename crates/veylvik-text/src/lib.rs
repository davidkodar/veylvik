//! Font shaping and editable outlines, independent of GPU, filesystem and UI.
//! `shape_run` accepts one horizontal directional/script run, not a paragraph.
//! Paragraph bidi resolution, script itemization, font fallback and line wrapping
//! belong to a higher layout layer. Font variation axes currently use defaults.
use std::{collections::BTreeMap, sync::Arc};
use veylvik_document::vector::{FillRule, MAX_PATH_COMMANDS, PathCommand, PathMask};

pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 4096;
pub const MAX_GLYPHS: usize = 512;
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(String);
pub type Result<T> = std::result::Result<T, Error>;
fn error(value: impl ToString) -> Error {
    Error(value.to_string())
}

/// Immutable font data. The host selects and loads the font; no system font
/// lookup or implicit fallback can change a render on another machine.
pub struct Font {
    bytes: Arc<[u8]>,
    index: u32,
    shaping: harfrust::ShaperData,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub font_size: f32,
    /// Baseline origin in composition pixels; y increases downwards.
    pub origin: [f32; 2],
    pub direction: Direction,
    pub language: Option<String>,
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            font_size: 64.0,
            origin: [0.0, 64.0],
            direction: Direction::LeftToRight,
            language: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Glyph {
    pub id: u16,
    /// UTF-8 byte offset in the original run; ligatures may span several bytes.
    pub cluster: u32,
    pub origin: [f32; 2],
    pub advance: [f32; 2],
    pub missing: bool,
    /// False for spaces and fonts with no supported vector outline for a glyph.
    pub has_outline: bool,
}
pub struct ShapedRun {
    /// Glyphs in visual order as emitted by the shaper.
    pub glyphs: Vec<Glyph>,
    pub advance: [f32; 2],
    /// None for empty/whitespace-only output. No empty invalid path is invented.
    pub outline: Option<PathMask>,
}
impl Font {
    pub fn from_bytes(bytes: Arc<[u8]>, index: u32) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Err(error("font must contain 1..32 MiB"));
        }
        ttf_parser::Face::parse(&bytes, index).map_err(error)?;
        let font = harfrust::FontRef::from_index(&bytes, index).map_err(error)?;
        let shaping = harfrust::ShaperData::new(&font);
        Ok(Self {
            bytes,
            index,
            shaping,
        })
    }
    pub fn byte_size(&self) -> usize {
        self.bytes.len()
    }
    pub fn shape_run(&self, text: &str, options: &RunOptions) -> Result<ShapedRun> {
        if text.len() > MAX_TEXT_BYTES
            || text.chars().any(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(error(
                "text run exceeds 4096 UTF-8 bytes or contains paragraph/control characters",
            ));
        }
        if !options.font_size.is_finite()
            || !(1.0..=1024.0).contains(&options.font_size)
            || options
                .origin
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 32768.0)
        {
            return Err(error(
                "text size must be 1..1024 and origin finite within ±32768",
            ));
        }
        let face = ttf_parser::Face::parse(&self.bytes, self.index).map_err(error)?;
        let font = harfrust::FontRef::from_index(&self.bytes, self.index).map_err(error)?;
        let shaper = self.shaping.shaper(&font).build();
        let scale = options.font_size / f32::from(face.units_per_em());
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(match options.direction {
            Direction::LeftToRight => harfrust::Direction::LeftToRight,
            Direction::RightToLeft => harfrust::Direction::RightToLeft,
        });
        if let Some(language) = &options.language {
            if language.is_empty() || language.len() > 64 {
                return Err(error("language tag must contain 1..64 bytes"));
            }
            buffer.set_language(
                language
                    .parse()
                    .map_err(|_| error("invalid language tag"))?,
            );
        }
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
        if shaped.len() > MAX_GLYPHS {
            return Err(error("text run exceeds 512 positioned glyphs"));
        }
        let mut pen = options.origin;
        let mut glyphs = Vec::with_capacity(shaped.len());
        let mut commands = Vec::new();
        // Repeated glyphs share parsed font-unit contours within this run.
        let mut outlines = BTreeMap::<u16, Vec<PathCommand>>::new();
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            let id = u16::try_from(info.glyph_id)
                .map_err(|_| error("glyph id exceeds outline format"))?;
            if let std::collections::btree_map::Entry::Vacant(entry) = outlines.entry(id) {
                let mut builder = Outline::default();
                let bounds = face.outline_glyph(ttf_parser::GlyphId(id), &mut builder);
                if bounds.is_none() && !builder.commands.is_empty() {
                    return Err(error("font contains an incomplete glyph outline"));
                }
                if builder.exceeded {
                    return Err(error("glyph outline exceeds path command limit"));
                }
                entry.insert(builder.commands);
            }
            let outline = &outlines[&id];
            if commands.len() + outline.len() > MAX_PATH_COMMANDS {
                return Err(error(
                    "text outline exceeds 4096 path commands; split it into runs",
                ));
            }
            let origin = [
                pen[0] + position.x_offset as f32 * scale,
                pen[1] - position.y_offset as f32 * scale,
            ];
            let advance = [
                position.x_advance as f32 * scale,
                -(position.y_advance as f32) * scale,
            ];
            let transform = |p: [f32; 2]| [origin[0] + p[0] * scale, origin[1] - p[1] * scale];
            for command in outline {
                commands.push(match *command {
                    PathCommand::Move { to } => PathCommand::Move { to: transform(to) },
                    PathCommand::Line { to } => PathCommand::Line { to: transform(to) },
                    PathCommand::Quad { control, to } => PathCommand::Quad {
                        control: transform(control),
                        to: transform(to),
                    },
                    PathCommand::Cubic {
                        control1,
                        control2,
                        to,
                    } => PathCommand::Cubic {
                        control1: transform(control1),
                        control2: transform(control2),
                        to: transform(to),
                    },
                    PathCommand::Close {} => PathCommand::Close {},
                });
            }
            glyphs.push(Glyph {
                id,
                cluster: info.cluster,
                origin,
                advance,
                missing: id == 0,
                has_outline: !outline.is_empty(),
            });
            pen[0] += advance[0];
            pen[1] += advance[1];
            if pen
                .iter()
                .chain(origin.iter())
                .any(|v| !v.is_finite() || v.abs() > 32768.0)
            {
                return Err(error("text placement exceeds ±32768 composition pixels"));
            }
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
        Ok(ShapedRun {
            glyphs,
            advance: [pen[0] - options.origin[0], pen[1] - options.origin[1]],
            outline,
        })
    }
}
#[derive(Default)]
struct Outline {
    commands: Vec<PathCommand>,
    exceeded: bool,
}
impl Outline {
    fn push(&mut self, command: PathCommand) {
        if self.commands.len() < MAX_PATH_COMMANDS {
            self.commands.push(command);
        } else {
            self.exceeded = true;
        }
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push(PathCommand::Move { to: [x, y] });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.push(PathCommand::Line { to: [x, y] });
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.push(PathCommand::Quad {
            control: [x1, y1],
            to: [x, y],
        });
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.push(PathCommand::Cubic {
            control1: [x1, y1],
            control2: [x2, y2],
            to: [x, y],
        });
    }
    fn close(&mut self) {
        self.push(PathCommand::Close {});
    }
}
