//! Bounded linear OpenEXR inputs. Color conversion remains explicit at the IO boundary.
use crate::{MAX_IMAGE_BYTES, Result};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};
use veylvik_render::Image;

/// Load a PNG or a supported linear premultiplied OpenEXR image.
pub fn load_image(path: &Path, budget: u64) -> Result<Image> {
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("exr"))
    {
        load_exr(path, budget)
    } else {
        // Preserve PNG signature-based loading for existing asset keys.
        crate::load_png(path, budget)
    }
}
/// Single-part, origin-zero RGBA float scanlines with square pixels and Rec.709
/// primaries (explicit or the EXR default). Supports lossless ZIP, RLE and raw.
pub fn load_exr(path: &Path, budget: u64) -> Result<Image> {
    use exr::prelude::*;
    const MAX_FILE: u64 = 64 * 1024 * 1024;
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return Err("EXR needs a regular file of at most 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("EXR file exceeds 64 MiB".into());
    }
    preflight_header(&bytes)?;
    let metadata = MetaData::read_from_buffered(Cursor::new(&bytes), true)?;
    if metadata.headers.len() != 1 {
        return Err("EXR needs one flat image part".into());
    }
    let header = &metadata.headers[0];
    let size = header.layer_size;
    if size.width() == 0 || size.height() == 0 || size.width() > 8192 || size.height() > 8192 {
        return Err("EXR dimensions exceed 8192 pixels".into());
    }
    if size.width() as u64 * size.height() as u64 * 16 > budget.min(MAX_IMAGE_BYTES) {
        return Err("EXR dimensions exceed image input budget".into());
    }
    if header.deep || header.blocks != exr::meta::BlockDescription::ScanLines {
        return Err("EXR needs flat scanlines without tiles or resolution levels".into());
    }
    let lines = match header.compression {
        Compression::Uncompressed | Compression::RLE | Compression::ZIP1 => 1,
        Compression::ZIP16 => 16,
        _ => return Err("EXR input supports raw, RLE and ZIP compression".into()),
    };
    if header.chunk_count != size.height().div_ceil(lines) {
        return Err("EXR chunk count does not match scanline dimensions".into());
    }
    if header.own_attributes.layer_position != Vec2(0, 0)
        || header.shared_attributes.display_window.position != Vec2(0, 0)
        || header.shared_attributes.display_window.size != size
        || header.shared_attributes.pixel_aspect != 1.0
    {
        return Err("EXR needs matching origin-zero data/display windows and square pixels".into());
    }
    let expected = exr::meta::attribute::Chromaticities {
        red: Vec2(0.64, 0.33),
        green: Vec2(0.30, 0.60),
        blue: Vec2(0.15, 0.06),
        white: Vec2(0.3127, 0.3290),
    };
    if header
        .shared_attributes
        .chromaticities
        .is_some_and(|c| c != expected)
    {
        return Err(
            "EXR chromaticities require an unsupported input transform; supply linear Rec.709/D65"
                .into(),
        );
    }
    if header.channels.list.len() != 4
        || header
            .channels
            .list
            .iter()
            .zip(["A", "B", "G", "R"])
            .any(|(channel, name)| {
                channel.name != *name
                    || channel.sampling != Vec2(1, 1)
                    || !matches!(channel.sample_type, SampleType::F16 | SampleType::F32)
            })
    {
        return Err("EXR needs exactly full-resolution R, G, B and A float channels".into());
    }
    let width = size.width();
    // Decode the same immutable bytes that were checked above, serially.
    let decoded = read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            move |_, _| vec![[0.0f32; 4]; width * size.height()],
            move |pixels: &mut Vec<[f32; 4]>, position, (r, g, b, a): (f32, f32, f32, f32)| {
                pixels[position.y() * width + position.x()] = [r, g, b, a];
            },
        )
        .first_valid_layer()
        .all_attributes()
        .non_parallel()
        .pedantic()
        .from_buffered(Cursor::new(&bytes))?;
    Ok(veylvik_render::Image::new(
        width as u32,
        size.height() as u32,
        decoded.layer_data.channel_data.pixels,
    )?)
}

// Bound attribute parsing before the decoder sees lengths supplied by the file.
// Version 2 without feature flags is single-part, non-deep, short-name scanline EXR.
fn preflight_header(bytes: &[u8]) -> Result<()> {
    if bytes.get(..8) != Some(&[0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0]) {
        return Err("EXR input needs a single-part version-2 scanline file".into());
    }
    let mut offset = 8;
    for _ in 0..256 {
        if bytes.get(offset) == Some(&0) {
            return Ok(());
        }
        for _ in 0..2 {
            let remaining = bytes.get(offset..).ok_or("truncated EXR header")?;
            let end = remaining
                .iter()
                .take(32)
                .position(|b| *b == 0)
                .ok_or("EXR attribute name exceeds 31 bytes")?;
            if end == 0 {
                return Err("EXR attribute name/type is empty".into());
            }
            offset += end + 1;
        }
        let length = bytes
            .get(offset..offset + 4)
            .ok_or("truncated EXR attribute length")?;
        let length = i32::from_le_bytes(length.try_into().unwrap());
        if length < 0 {
            return Err("negative EXR attribute length".into());
        }
        offset = offset
            .checked_add(4 + length as usize)
            .ok_or("EXR header overflow")?;
        if offset > bytes.len() || offset > 1024 * 1024 {
            return Err("EXR header exceeds file or 1 MiB limit".into());
        }
    }
    Err("EXR header exceeds 256 attributes".into())
}
