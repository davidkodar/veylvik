//! Bounded linear OpenEXR inputs, including one layer of a multi-layer file (such as a Blender pass). Color conversion remains explicit at the IO boundary.
use crate::{MAX_IMAGE_BYTES, Result};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};
use veylvik_render::Image;

/// Largest EXR file read (multi-layer renders carry many passes).
const MAX_FILE: u64 = 256 * 1024 * 1024;

/// Load a PNG or a supported linear premultiplied OpenEXR image.
pub fn load_image(path: &Path, budget: u64) -> Result<Image> {
    load_image_layer(path, None, budget)
}
/// As `load_image`; `layer` picks a layer of a multi-layer OpenEXR (see `exr_layers`).
pub fn load_image_layer(path: &Path, layer: Option<&str>, budget: u64) -> Result<Image> {
    if is_exr(path) {
        load_exr_layer(path, layer, budget)
    } else if let Some(layer) = layer {
        Err(format!("only OpenEXR files have layers, not {layer}").into())
    } else if is_photo(path) {
        load_photo(path, budget)
    } else {
        // Preserve PNG signature-based loading for existing asset keys.
        crate::load_png(path, budget)
    }
}
/// JPEG, TIFF and WebP stills, by extension.
pub fn is_photo(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["jpg", "jpeg", "tif", "tiff", "webp"]
            .iter()
            .any(|e| s.eq_ignore_ascii_case(e))
    })
}
/// A JPEG, TIFF or WebP still, read as sRGB (these formats' convention; an embedded
/// profile is not applied) into linear premultiplied pixels. Up to 8192 pixels a side.
fn load_photo(path: &Path, budget: u64) -> Result<Image> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(budget.min(crate::MAX_IMAGE_BYTES));
    reader.limits(limits);
    let (width, height) = reader.into_dimensions()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("image dimensions exceed 8192 pixels".into());
    }
    if u64::from(width) * u64::from(height) * 16 > budget.min(crate::MAX_IMAGE_BYTES) {
        return Err("image dimensions exceed image input budget".into());
    }
    let decoded = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .into_rgba32f();
    let linear = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    let pixels = decoded
        .pixels()
        .map(|p| {
            let a = p.0[3].clamp(0.0, 1.0);
            [
                linear(p.0[0]) * a,
                linear(p.0[1]) * a,
                linear(p.0[2]) * a,
                a,
            ]
        })
        .collect();
    Ok(Image::new(width, height, pixels)?)
}
pub fn is_exr(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("exr"))
}
/// The main image: top-level R, G, B (and A), else a layer called Combined.
pub fn load_exr(path: &Path, budget: u64) -> Result<Image> {
    load_exr_layer(path, None, budget)
}

/// A file's bytes and its checked header.
struct Opened {
    bytes: Vec<u8>,
    header: exr::meta::header::Header,
}
/// Single-part, origin-zero float scanlines with square pixels and Rec.709 primaries
/// (explicit or the EXR default), any compression the decoder reads.
fn open(path: &Path) -> Result<Opened> {
    use exr::prelude::*;
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return Err("EXR needs a regular file of at most 256 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("EXR file exceeds 256 MiB".into());
    }
    preflight_header(&bytes)?;
    let metadata = MetaData::read_from_buffered(Cursor::new(&bytes), true)?;
    if metadata.headers.len() != 1 {
        return Err("EXR needs one flat image part".into());
    }
    let header = metadata.headers.into_iter().next().unwrap();
    let size = header.layer_size;
    if size.width() == 0 || size.height() == 0 || size.width() > 8192 || size.height() > 8192 {
        return Err("EXR dimensions exceed 8192 pixels".into());
    }
    if header.deep || header.blocks != exr::meta::BlockDescription::ScanLines {
        return Err("EXR needs flat scanlines without tiles or resolution levels".into());
    }
    if header.chunk_count
        != size
            .height()
            .div_ceil(header.compression.scan_lines_per_block())
    {
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
    if header.channels.list.len() > 1024
        || header.channels.list.iter().any(|c| {
            c.sampling != Vec2(1, 1) || !matches!(c.sample_type, SampleType::F16 | SampleType::F32)
        })
    {
        return Err("EXR needs full-resolution float channels".into());
    }
    Ok(Opened { bytes, header })
}

/// A layer: channel names grouped by everything before their last dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// "" for the top-level channels, else such as "ViewLayer.Depth".
    pub name: String,
    /// The short channel names, such as R, G, B, A or Z.
    pub channels: Vec<String>,
}
fn split(full: &str) -> (&str, &str) {
    full.rsplit_once('.').unwrap_or(("", full))
}
fn layers_of(header: &exr::meta::header::Header) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for channel in &header.channels.list {
        let full = channel.name.to_string();
        let (layer, short) = split(&full);
        match layers.iter_mut().find(|l| l.name == layer) {
            Some(l) => l.channels.push(short.to_string()),
            None => layers.push(Layer {
                name: layer.to_string(),
                channels: vec![short.to_string()],
            }),
        }
    }
    layers
}
/// The layers of an OpenEXR file, in file order; reads only its header (at most 4 MiB).
pub fn exr_layers(path: &Path) -> Result<Vec<Layer>> {
    let mut head = Vec::new();
    File::open(path)?
        .take(4 * 1024 * 1024 + 8)
        .read_to_end(&mut head)?;
    preflight_header(&head)?;
    let metadata = exr::meta::MetaData::read_from_buffered(Cursor::new(&head), false)?;
    let header = metadata.headers.first().ok_or("EXR has no image part")?;
    Ok(layers_of(header))
}
/// How a layer becomes RGBA: its channels in order, and whether the last is alpha.
fn plan(layer: &Layer) -> Result<(Vec<String>, bool)> {
    let has = |c: &str| layer.channels.iter().any(|x| x == c);
    let full = |c: &str| {
        if layer.name.is_empty() {
            c.to_string()
        } else {
            format!("{}.{c}", layer.name)
        }
    };
    // Only the main image is premultiplied colour; passes are data with alpha 1.
    let main = layer.name.is_empty() || split(&layer.name).1 == "Combined";
    let pick = |names: &[&str]| names.iter().map(|c| full(c)).collect::<Vec<_>>();
    if has("R") && has("G") && has("B") {
        return Ok(if main && has("A") {
            (pick(&["R", "G", "B", "A"]), true)
        } else {
            (pick(&["R", "G", "B"]), false)
        });
    }
    if has("X") && has("Y") && has("Z") {
        return Ok((pick(&["X", "Y", "Z"]), false));
    }
    match layer.channels.as_slice() {
        [one] => Ok((vec![full(one)], false)),
        [a, b] => Ok((vec![full(a), full(b)], false)),
        _ => Err(format!(
            "EXR layer {} has channels {} that do not map to an image",
            layer.name,
            layer.channels.join(", ")
        )
        .into()),
    }
}
/// The display name of a layer for messages.
fn shown(name: &str) -> &str {
    if name.is_empty() { "(main)" } else { name }
}
/// Load one layer of an OpenEXR file as RGBA. Colour layers give R, G, B (with A only
/// for the main image or a Combined pass, which are premultiplied); X, Y, Z data
/// (normals, positions, motion) and two-channel layers fill R, G (and B); a single
/// channel (such as depth Z) fills R, G and B. Passes get alpha 1. With no layer, the
/// main image: top-level R, G, B, else a layer whose last part is Combined.
pub fn load_exr_layer(path: &Path, layer: Option<&str>, budget: u64) -> Result<Image> {
    use exr::prelude::*;
    let Opened { bytes, header } = open(path)?;
    let size = header.layer_size;
    if size.width() as u64 * size.height() as u64 * 16 > budget.min(MAX_IMAGE_BYTES) {
        return Err("EXR dimensions exceed image input budget".into());
    }
    let layers = layers_of(&header);
    let names = || {
        layers
            .iter()
            .map(|l| shown(&l.name).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let chosen = match layer {
        Some(name) => layers
            .iter()
            .find(|l| l.name == name)
            .ok_or_else(|| format!("EXR has no layer {name}; it has {}", names()))?,
        None => layers
            .iter()
            .find(|l| {
                l.name.is_empty()
                    && ["R", "G", "B"]
                        .iter()
                        .all(|c| l.channels.iter().any(|x| x == c))
            })
            .or_else(|| layers.iter().find(|l| split(&l.name).1 == "Combined"))
            .ok_or_else(|| {
                format!(
                    "EXR has no main R, G, B image or Combined pass; choose a layer: {}",
                    names()
                )
            })?,
    };
    let (channels, alpha) = plan(chosen)?;
    let (width, height) = (size.width(), size.height());
    // Decode the same immutable bytes that were checked above, serially, reading
    // only the chosen channels.
    macro_rules! decode {
        ($reader:expr, $pattern:pat, $ty:ty => $value:expr) => {{
            let decoded = $reader
                .collect_pixels(
                    move |_, _| vec![[0.0f32, 0.0, 0.0, 1.0]; width * height],
                    move |pixels: &mut Vec<[f32; 4]>, position, $pattern: $ty| {
                        pixels[position.y() * width + position.x()] = $value;
                    },
                )
                .first_valid_layer()
                .all_attributes()
                .non_parallel()
                .pedantic()
                .from_buffered(Cursor::new(&bytes))?;
            decoded.layer_data.channel_data.pixels
        }};
    }
    let read = || {
        read()
            .no_deep_data()
            .largest_resolution_level()
            .specific_channels()
    };
    let c = &channels;
    let pixels = match (c.len(), alpha) {
        (4, true) => decode!(
            read()
                .required(c[0].as_str())
                .required(c[1].as_str())
                .required(c[2].as_str())
                .required(c[3].as_str()),
            (r, g, b, a), (f32, f32, f32, f32) => [r, g, b, a]
        ),
        (1, _) => decode!(read().required(c[0].as_str()), (v,), (f32,) => [v, v, v, 1.0]),
        (2, _) => decode!(
            read().required(c[0].as_str()).required(c[1].as_str()),
            (x, y), (f32, f32) => [x, y, 0.0, 1.0]
        ),
        _ => decode!(
            read().required(c[0].as_str()).required(c[1].as_str()).required(c[2].as_str()),
            (r, g, b), (f32, f32, f32) => [r, g, b, 1.0]
        ),
    };
    if let Some(bad) = pixels.iter().position(|p| p.iter().any(|v| !v.is_finite())) {
        return Err(format!(
            "EXR layer {} has a value that is not a number at pixel {}, {}",
            shown(&chosen.name),
            bad % width,
            bad / width
        )
        .into());
    }
    Ok(veylvik_render::Image::new(
        width as u32,
        height as u32,
        pixels,
    )?)
}

// Bound attribute parsing before the decoder sees lengths supplied by the file.
// Version 2, single-part, non-deep scanlines; long (up to 255-byte) names allowed.
fn preflight_header(bytes: &[u8]) -> Result<()> {
    if bytes.get(..4) != Some(&[0x76, 0x2f, 0x31, 0x01])
        || bytes.get(4) != Some(&2)
        || !matches!(bytes.get(5), Some(0 | 0x04))
        || bytes.get(6..8) != Some(&[0, 0])
    {
        return Err(
            "EXR input needs a single-part version-2 scanline file (not tiled, deep or multi-part)"
                .into(),
        );
    }
    let longest = if bytes[5] & 0x04 != 0 { 256 } else { 32 };
    let mut offset = 8;
    for _ in 0..4096 {
        if bytes.get(offset) == Some(&0) {
            return Ok(());
        }
        for _ in 0..2 {
            let remaining = bytes.get(offset..).ok_or("truncated EXR header")?;
            let end = remaining
                .iter()
                .take(longest)
                .position(|b| *b == 0)
                .ok_or("EXR attribute name is too long")?;
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
        if offset > bytes.len() || offset > 4 * 1024 * 1024 {
            return Err("EXR header exceeds file or 4 MiB limit".into());
        }
    }
    Err("EXR header exceeds 4096 attributes".into())
}
