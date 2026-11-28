//! Explicit SDR PNG or scene-linear RGBA32F OpenEXR output.
use crate::Result;
use std::{io::Write, path::Path};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameFormat {
    #[default]
    Png,
    Exr,
}
impl FrameFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Exr => "exr",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG (SDR)",
            Self::Exr => "OpenEXR (HDR)",
        }
    }

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
/// Pixels to write to `path`: an SDR target (PNG) gets the composition's display transform,
/// while EXR stays scene-linear.
pub fn sdr_pixels<'a>(
    path: &Path,
    pixels: &'a [[f32; 4]],
    display: veylvik_document::media::DisplayTransform,
) -> std::borrow::Cow<'a, [[f32; 4]]> {
    match FrameFormat::from_path(path) {
        Ok(FrameFormat::Png) if !display.is_standard() => {
            std::borrow::Cow::Owned(veylvik_render::display::apply(display, pixels))
        }
        _ => std::borrow::Cow::Borrowed(pixels),
    }
}
pub fn save_frame(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
    match FrameFormat::from_path(path)? {
        FrameFormat::Png => crate::save_png(path, width, height, pixels),
        FrameFormat::Exr => save_exr(path, width, height, pixels),
    }
    .map_err(|e| format!("Cannot write {}: {e}", path.display()).into())
}
/// Publish a new frame without replacing an existing file or symlink.
pub fn save_frame_new(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
    match FrameFormat::from_path(path)? {
        FrameFormat::Png => {
            veylvik_document::atomic_create(path, &crate::encode_png(width, height, pixels)?)?;
            Ok(())
        }
        FrameFormat::Exr => write_exr(path, width, height, pixels, false),
    }
}
/// Lossless ZIP16 scanlines, 32-bit float RGBA, linear Rec.709/D65 chromaticities,
/// and premultiplied alpha. RGB is preserved, including HDR, negatives and emission
/// at zero alpha. Single-threaded block encoding avoids a second full-frame copy.
pub fn save_exr(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
    write_exr(path, width, height, pixels, true)
}
/// A layer of a layered EXR beside the picture: its name and what it holds.
pub struct ExrLayer<'a> {
    pub name: &'a str,
    pub kind: LayerKind,
    pub pixels: &'a [[f32; 4]],
}
/// How a layer's four values are named in the file (as compositing programs expect).
#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    /// NAME.R, .G, .B, .A
    Picture,
    /// NAME.Z: the distance from the camera.
    Depth,
    /// NAME.X, .Y, .Z: a direction or a place.
    Vector,
    /// forward.u, forward.v, backward.u, backward.v: Nuke's motion vectors.
    Motion,
    /// NAME00.R .G .B .A (rank 0, the two objects covering most) or NAME01 (rank 1, the
    /// next two), with Cryptomatte's metadata and the objects' manifest.
    Objects { rank: u32, names: Vec<String> },
}
/// A layered EXR: the picture as R, G, B, A (32-bit float, premultiplied, linear
/// Rec.709) and beside it each layer under its own channel names; objects layers carry
/// Cryptomatte's metadata, so Cryptomatte readers can pick objects by name.
pub fn save_layered_exr(
    path: &Path,
    width: u32,
    height: u32,
    picture: &[[f32; 4]],
    layers: &[ExrLayer<'_>],
) -> Result<()> {
    use exr::prelude::*;
    let pixels = width as usize * height as usize;
    if width == 0 || height == 0 || width > 8192 || height > 8192 || picture.len() != pixels {
        return Err("EXR needs matching pixels and dimensions within 1..8192".into());
    }
    if picture
        .iter()
        .any(|p| p.iter().any(|c| !c.is_finite()) || !(0.0..=1.0).contains(&p[3]))
    {
        return Err("EXR needs finite RGBA values and alpha in [0,1]".into());
    }
    let mut channels: Vec<AnyChannel<FlatSamples>> = Vec::new();
    let mut names: std::collections::BTreeSet<String> = Default::default();
    let mut add = |name: String, values: Vec<f32>| -> crate::Result<()> {
        if !names.insert(name.clone()) {
            return Err(format!("two layers write the channel {name}").into());
        }
        channels.push(AnyChannel::new(name.as_str(), FlatSamples::F32(values)));
        Ok(())
    };
    let column = |source: &[[f32; 4]], k: usize| source.iter().map(|p| p[k]).collect::<Vec<_>>();
    for (k, c) in ["R", "G", "B", "A"].iter().enumerate() {
        add((*c).to_owned(), column(picture, k))?;
    }
    let mut attributes = ImageAttributes::new(IntegerBounds::from_dimensions((
        width as usize,
        height as usize,
    )));
    attributes.chromaticities = Some(exr::meta::attribute::Chromaticities {
        red: Vec2(0.64, 0.33),
        green: Vec2(0.30, 0.60),
        blue: Vec2(0.15, 0.06),
        white: Vec2(0.3127, 0.3290),
    });
    let mut manifests: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for layer in layers {
        if layer.pixels.len() != pixels {
            return Err(format!("the {} layer is not the picture's size", layer.name).into());
        }
        if layer
            .pixels
            .iter()
            .any(|p| p.iter().any(|c| !c.is_finite()))
        {
            return Err(format!("the {} layer has values that are not finite", layer.name).into());
        }
        let name = layer.name.trim();
        if name.is_empty() || name.contains(['.', '/', '\0']) || name.len() > 64 {
            return Err(
                format!("{name:?} cannot name an EXR layer (1 to 64 characters, no dots)").into(),
            );
        }
        let p = layer.pixels;
        match &layer.kind {
            LayerKind::Picture => {
                for (k, c) in ["R", "G", "B", "A"].iter().enumerate() {
                    add(format!("{name}.{c}"), column(p, k))?;
                }
            }
            LayerKind::Depth => add(format!("{name}.Z"), column(p, 0))?,
            LayerKind::Vector => {
                for (k, c) in ["X", "Y", "Z"].iter().enumerate() {
                    add(format!("{name}.{c}"), column(p, k))?;
                }
            }
            LayerKind::Motion => {
                for (k, c) in ["forward.u", "forward.v", "backward.u", "backward.v"]
                    .iter()
                    .enumerate()
                {
                    add((*c).to_owned(), column(p, k))?;
                }
            }
            LayerKind::Objects { rank, names } => {
                for (k, c) in ["R", "G", "B", "A"].iter().enumerate() {
                    add(format!("{name}{rank:02}.{c}"), column(p, k))?;
                }
                manifests
                    .entry(name.to_owned())
                    .or_default()
                    .extend(names.iter().cloned());
            }
        }
    }
    for (name, objects) in &manifests {
        let key = veylvik_render::cryptomatte::layer_key(name);
        let text = |s: &str| AttributeValue::Text(Text::from(s));
        let manifest = veylvik_render::cryptomatte::manifest(objects.iter().map(String::as_str));
        for (field, value) in [
            ("name", name.as_str()),
            ("hash", "MurmurHash3_32"),
            ("conversion", "uint32_to_float32"),
            ("manifest", manifest.as_str()),
        ] {
            attributes.other.insert(
                Text::from(format!("cryptomatte/{key}/{field}").as_str()),
                text(value),
            );
        }
    }
    let layer = Layer::new(
        (width as usize, height as usize),
        LayerAttributes::default(),
        Encoding::SMALL_LOSSLESS,
        AnyChannels::sort(channels.into()),
    );
    let mut image = Image::from_layer(layer);
    image.attributes = attributes;
    veylvik_document::atomic_create_with(path, |file| {
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
fn write_exr(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[[f32; 4]],
    replace: bool,
) -> Result<()> {
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
    let write = |file: &mut std::fs::File| {
        let mut writer = std::io::BufWriter::new(file);
        image
            .write()
            .non_parallel()
            .to_buffered(&mut writer)
            .map_err(std::io::Error::other)?;
        writer.flush()
    };
    if replace {
        veylvik_document::atomic_write_with(path, write)?;
    } else {
        veylvik_document::atomic_create_with(path, write)?;
    }
    Ok(())
}
