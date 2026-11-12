//! Explicit font-file adapter for portable text-to-path conversion.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use veylvik_document::vector::PathMask;
use veylvik_text::{Font, MAX_FONT_BYTES, RunOptions, TextOptions};
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    LeftToRight,
    RightToLeft,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineRequest {
    pub font: String,
    pub text: String,
    #[serde(default)]
    pub font_index: u32,
    #[serde(default = "default_size")]
    pub font_size: f32,
    #[serde(default = "default_origin")]
    pub origin: [f32; 2],
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub alignment: Alignment,
    #[serde(default)]
    pub line_height: Option<f32>,
}
impl OutlineRequest {
    pub fn label(&self) -> String {
        self.text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Text")
            .chars()
            .take(80)
            .collect()
    }
}
fn default_size() -> f32 {
    64.0
}
fn default_origin() -> [f32; 2] {
    [100.0, 200.0]
}
#[derive(Serialize)]
pub struct OutlinedText {
    pub geometry: PathMask,
    pub glyph_count: usize,
    pub advance: [f32; 2],
    pub line_count: usize,
}
pub fn outline(base: &Path, request: &OutlineRequest) -> Result<OutlinedText> {
    let path = base.join(&request.font);
    if !std::fs::metadata(&path)?.is_file() {
        return Err("font must be a regular file".into());
    }
    let file = File::open(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FONT_BYTES as u64 {
        return Err("font must be a regular file no larger than 32 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FONT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let font = Font::from_bytes(Arc::from(bytes), request.font_index)?;
    let run = font.shape_lines(
        &request.text,
        &TextOptions {
            run: RunOptions {
                tracking: 0.0,
                features: Vec::new(),
                font_size: request.font_size,
                origin: request.origin,
                language: request.language.clone(),
                direction: match request.direction {
                    Direction::LeftToRight => veylvik_text::Direction::LeftToRight,
                    Direction::RightToLeft => veylvik_text::Direction::RightToLeft,
                },
            },
            alignment: match request.alignment {
                Alignment::Left => veylvik_text::Alignment::Left,
                Alignment::Center => veylvik_text::Alignment::Center,
                Alignment::Right => veylvik_text::Alignment::Right,
            },
            line_height: request.line_height,
        },
    )?;
    if let Some(glyph) = run.glyphs.iter().find(|glyph| glyph.missing) {
        return Err(format!(
            "font lacks a glyph at UTF-8 byte {}; choose a font covering this run",
            glyph.cluster
        )
        .into());
    }
    Ok(OutlinedText {
        geometry: run
            .outline
            .ok_or("text has no supported visible vector outlines")?,
        glyph_count: run.glyphs.len(),
        advance: run.advance,
        line_count: run.line_count,
    })
}
