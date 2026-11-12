//! Explicit SDR PNG or scene-linear RGBA32F OpenEXR output.
use crate::Result;
use std::{io::Write, path::Path};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    Png,
    Exr,
}
impl FrameFormat {
    pub fn from_path(path: &Path) -> Result<Self> {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png") => Ok(Self::Png),
            Some("exr") => Ok(Self::Exr),
            _ => Err("frame output filename must end in .png or .exr".into()),
        }
    }
}
pub fn save_frame(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
    match FrameFormat::from_path(path)? {
        FrameFormat::Png => crate::save_png(path, width, height, pixels),
        FrameFormat::Exr => save_exr(path, width, height, pixels),
    }
}
/// Lossless ZIP16 scanlines, 32-bit float RGBA, linear Rec.709/D65 chromaticities,
/// and premultiplied alpha. RGB is preserved, including HDR, negatives and emission
/// at zero alpha. Single-threaded block encoding avoids a second full-frame copy.
pub fn save_exr(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
    use exr::prelude::*;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || pixels.len() as u64 != width as u64 * height as u64
    {
        return Err("EXR needs matching pixels and dimensions within 1..8192".into());
    }
    if pixels
        .iter()
        .any(|p| p.iter().any(|c| !c.is_finite()) || !(0.0..=1.0).contains(&p[3]))
    {
        return Err("EXR needs finite RGBA values and alpha in [0,1]".into());
    }
    let channels = SpecificChannels::rgba(|position: Vec2<usize>| {
        let p = pixels[position.y() * width as usize + position.x()];
        (p[0], p[1], p[2], p[3])
    });
    let mut image = Image::from_encoded_channels(
        (width as usize, height as usize),
        Encoding::SMALL_LOSSLESS,
        channels,
    );
    image.attributes.chromaticities = Some(exr::meta::attribute::Chromaticities {
        red: Vec2(0.64, 0.33),
        green: Vec2(0.30, 0.60),
        blue: Vec2(0.15, 0.06),
        white: Vec2(0.3127, 0.3290),
    });
    veylvik_document::atomic_write_with(path, |file| {
        let mut writer = std::io::BufWriter::new(file);
        image
            .write()
            .non_parallel()
            .to_buffered(&mut writer)
            .map_err(std::io::Error::other)?;
        writer.flush()
    })?;
    Ok(())
}
