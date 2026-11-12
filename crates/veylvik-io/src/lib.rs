//! Filesystem and PNG adapter; the compositor only sees immutable linear images.
pub mod sequence;
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};
use veylvik_document::{Composition, Operation};
use veylvik_render::{Image, ImageInputs, Plan};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;

pub fn document_base(path: &Path) -> Result<PathBuf> {
    Ok(path
        .canonicalize()?
        .parent()
        .ok_or("composition has no parent")?
        .to_owned())
}
pub fn load_png(path: &Path, budget: u64) -> Result<Image> {
    let file = File::open(path)?;
    if file.metadata()?.len() > 64 * 1024 * 1024 {
        return Err("PNG file exceeds 64 MiB".into());
    }
    let mut decoder = png::Decoder::new_with_limits(
        BufReader::new(file),
        png::Limits {
            bytes: 64 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("PNG dimensions exceed 8192 pixels".into());
    }
    let bytes = width as u64 * height as u64 * 16;
    if bytes > budget.min(MAX_IMAGE_BYTES) {
        return Err("PNG dimensions exceed image input budget".into());
    }
    if info.animation_control.is_some() {
        return Err("animated PNG inputs are not supported".into());
    }
    let srgb_primaries =
        png::SourceChromaticities::new((0.3127, 0.3290), (0.64, 0.33), (0.30, 0.60), (0.15, 0.06));
    if info.icc_profile.is_some()
        || info.coding_independent_code_points.is_some()
        || (info.srgb.is_none()
            && (info.gama_chunk.is_some_and(|g| g.into_scaled() != 45455)
                || info
                    .source_chromaticities
                    .is_some_and(|c| c != srgb_primaries)))
    {
        return Err(
            "PNG colour metadata requires an unsupported input transform; supply an sRGB PNG"
                .into(),
        );
    }
    let buffer_size = reader
        .output_buffer_size()
        .ok_or("PNG output size overflow")?;
    if buffer_size as u64 > bytes {
        return Err("unexpected PNG buffer size".into());
    }
    let mut raw = vec![0; buffer_size];
    let frame = reader.next_frame(&mut raw)?;
    let channels = frame.color_type.samples();
    let sample_bytes = match frame.bit_depth {
        png::BitDepth::Eight => 1,
        png::BitDepth::Sixteen => 2,
        _ => return Err("unsupported expanded PNG depth".into()),
    };
    let decode = |x: f32| {
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for pixel in raw[..frame.buffer_size()].chunks_exact(channels * sample_bytes) {
        let sample = |i: usize| {
            if sample_bytes == 1 {
                pixel[i] as f32 / 255.0
            } else {
                u16::from_be_bytes([pixel[i * 2], pixel[i * 2 + 1]]) as f32 / 65535.0
            }
        };
        let (rgb, a) = match frame.color_type {
            png::ColorType::Grayscale => ([sample(0); 3], 1.0),
            png::ColorType::GrayscaleAlpha => ([sample(0); 3], sample(1)),
            png::ColorType::Rgb => ([sample(0), sample(1), sample(2)], 1.0),
            png::ColorType::Rgba => ([sample(0), sample(1), sample(2)], sample(3)),
            _ => return Err("PNG palette was not expanded".into()),
        };
        pixels.push([
            decode(rgb[0]) * a,
            decode(rgb[1]) * a,
            decode(rgb[2]) * a,
            a,
        ]);
    }
    Ok(Image::new(width, height, pixels)?)
}
pub fn load_inputs(plan: &Plan, base: &Path, budget: u64) -> Result<ImageInputs> {
    let mut remaining = budget
        .checked_sub(plan.working_bytes())
        .ok_or("render exceeds image input budget")?;
    let mut inputs = ImageInputs::default();
    for key in plan.required_assets() {
        let image =
            load_png(&base.join(key), remaining).map_err(|e| format!("image {key}: {e}"))?;
        remaining -= image.byte_size();
        inputs.insert(key, Arc::new(image));
    }
    Ok(inputs)
}
/// Save-as rebases relative assets without changing the live document or its origin.
pub fn save(composition: &Composition, base: &Path, destination: &Path) -> Result<()> {
    save_tracked(composition, base, destination, None).map(|_| ())
}
/// Save a snapshot and return exactly the bytes written, checking a known file
/// stamp immediately before replacement when provided.
pub fn save_tracked(
    composition: &Composition,
    base: &Path,
    destination: &Path,
    previous: Option<&FileStamp>,
) -> Result<FileStamp> {
    protect_inputs(composition, base, destination)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let mut saved = composition.clone();
    for node in saved.nodes.values_mut() {
        if let Operation::Image { asset } = &mut node.operation
            && Path::new(asset).is_relative()
        {
            let resolved = base.join(&*asset);
            let relative = pathdiff::diff_paths(&resolved, &parent).ok_or("cannot rebase asset")?;
            *asset = relative
                .to_str()
                .ok_or("asset path is not UTF-8")?
                .to_owned();
        }
    }
    saved.validate()?;
    let bytes = serde_json::to_vec_pretty(&saved)?;
    if bytes.len() > veylvik_document::limits::MAX_DOCUMENT_BYTES {
        return Err("composition exceeds 16 MiB".into());
    }
    if let Some(previous) = previous {
        previous.verify_destination(destination)?;
    }
    veylvik_document::atomic_write(destination, &bytes)?;
    Ok(FileStamp {
        path: parent.join(
            destination
                .file_name()
                .ok_or("destination needs a filename")?,
        ),
        bytes: bytes.into(),
    })
}

/// Export and save must not replace an image used by the composition.
pub fn protect_inputs(composition: &Composition, base: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let target = destination.canonicalize().unwrap_or(
        parent.join(
            destination
                .file_name()
                .ok_or("destination needs a filename")?,
        ),
    );
    for node in composition.nodes.values() {
        if let Operation::Image { asset } = &node.operation {
            let input = base.join(asset);
            if input.canonicalize().unwrap_or(input) == target {
                return Err("destination would replace an input image".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, Time};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "veylvik-assets-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn png(path: &Path, depth: png::BitDepth, bytes: &[u8], gamma: Option<f32>) {
        let mut encoder = png::Encoder::new(File::create(path).unwrap(), 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(depth);
        if let Some(g) = gamma {
            encoder.set_source_gamma(png::ScaledFloat::new(g));
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(bytes)
            .unwrap();
    }
    #[test]
    fn png_decodes_linear_premultiplied_and_preserves_16bit_precision() {
        let temp = Temp::new();
        let path = temp.0.join("source.png");
        png(
            &path,
            png::BitDepth::Eight,
            &[128, 255, 0, 128, 255, 0, 255, 0],
            None,
        );
        let image = load_png(&path, 1024).unwrap();
        assert!((image.pixels()[0][0] - 0.2158605 * (128.0 / 255.0)).abs() < 1e-6);
        assert_eq!(image.pixels()[0][1], 128.0 / 255.0);
        assert_eq!(image.pixels()[1], [0.0; 4]);
        let samples = [32768u16, 0, 0, 65535, 32769, 0, 0, 65535];
        let bytes: Vec<_> = samples.into_iter().flat_map(u16::to_be_bytes).collect();
        png(&path, png::BitDepth::Sixteen, &bytes, None);
        let image = load_png(&path, 1024).unwrap();
        let delta = image.pixels()[1][0] - image.pixels()[0][0];
        assert!(delta > 0.0 && delta < 0.0001);
    }
    #[test]
    fn invalid_inputs_fail_without_replacing_the_source() {
        let temp = Temp::new();
        let path = temp.0.join("source.png");
        png(&path, png::BitDepth::Eight, &[0; 8], Some(1.0));
        assert!(
            load_png(&path, 1024)
                .err()
                .unwrap()
                .to_string()
                .contains("colour metadata")
        );
        png(&path, png::BitDepth::Eight, &[0; 8], None);
        assert!(load_png(&path, 1).is_err());
        std::fs::write(&path, b"not a PNG").unwrap();
        assert!(load_png(&path, 1024).is_err());
    }
    #[test]
    fn save_as_preserves_asset_resolution_and_live_document() {
        let temp = Temp::new();
        let image_path = temp.0.join("source.png");
        png(&image_path, png::BitDepth::Eight, &[255; 8], None);
        std::fs::create_dir(temp.0.join("sub")).unwrap();
        let node = Node::new(
            "image",
            Operation::Image {
                asset: "source.png".into(),
            },
            &[],
        );
        let doc = Composition {
            schema: 1,
            width: 2,
            height: 1,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into_iter().collect(),
        };
        let saved = temp.0.join("sub/moved.veylvik");
        save(&doc, &temp.0, &saved).unwrap();
        let restored = Composition::load(&saved).unwrap();
        let inputs = load_inputs(
            &Plan::compile(&restored).unwrap(),
            &document_base(&saved).unwrap(),
            1024,
        )
        .unwrap();
        assert_eq!(inputs.get("../source.png").unwrap().pixels()[0], [1.0; 4]);
        assert!(
            matches!(&doc.nodes[&doc.output].operation,Operation::Image {asset} if asset=="source.png")
        );
        let before = std::fs::read(&image_path).unwrap();
        assert!(save(&doc, &temp.0, &image_path).is_err());
        assert_eq!(std::fs::read(image_path).unwrap(), before);
    }
}

/// Exact bounded bytes from the last load/save, used to detect external edits.
#[derive(Clone)]
pub struct FileStamp {
    path: PathBuf,
    bytes: Arc<[u8]>,
}
impl FileStamp {
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn read(path: &Path) -> Result<Vec<u8>> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(veylvik_document::limits::MAX_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > veylvik_document::limits::MAX_DOCUMENT_BYTES {
            return Err("composition exceeds 16 MiB".into());
        }
        Ok(bytes)
    }
    pub fn load(path: &Path) -> Result<(Composition, Self)> {
        let path = path.canonicalize()?;
        let bytes = Self::read(&path)?;
        let composition: Composition = serde_json::from_slice(&bytes)?;
        composition.validate()?;
        Ok((
            composition,
            Self {
                path,
                bytes: bytes.into(),
            },
        ))
    }
    /// Check immediately before saving. This detects changed or deleted files,
    /// but is not a filesystem transaction against unrelated concurrent writers.
    pub fn verify_destination(&self, destination: &Path) -> Result<()> {
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let normalized = parent.join(
            destination
                .file_name()
                .ok_or("destination needs a filename")?,
        );
        let same =
            normalized == self.path || destination.canonicalize().ok().as_ref() == Some(&self.path);
        if same && Self::read(&self.path).ok().as_deref() != Some(self.bytes.as_ref()) {
            return Err("composition changed or was removed outside Veylvik; reopen it or save to a different path".into());
        }
        Ok(())
    }
}
/// PNG output uses the renderer's explicit scene-linear to SDR boundary.
pub fn save_png(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
    veylvik_document::atomic_write(path, &encode_png(width, height, pixels)?)?;
    Ok(())
}
fn encode_png(width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    if pixels.len() as u64 != width as u64 * height as u64 {
        return Err("PNG pixel count does not match dimensions".into());
    }
    let rgba = veylvik_render::to_srgba8(pixels);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(png)
}

#[cfg(test)]
mod stamp_tests {
    use super::*;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "veylvik-stamp-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn tracked_saves_detect_external_changes_and_allow_save_as() {
        let root = Scratch::new();
        let path = root.0.join("scene.veylvik");
        let doc = veylvik_document::demo();
        let stamp = save_tracked(&doc, &root.0, &path, None).unwrap();
        stamp.verify_destination(&path).unwrap();
        let (_, loaded) = FileStamp::load(&path).unwrap();
        std::fs::write(&path, b"external edit").unwrap();
        assert!(save_tracked(&doc, &root.0, &path, Some(&loaded)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external edit");
        let other = root.0.join("copy.veylvik");
        save_tracked(&doc, &root.0, &other, Some(&loaded)).unwrap();
        Composition::load(&other).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(loaded.verify_destination(&path).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&other, &path).unwrap();
            std::fs::write(&other, b"different target").unwrap();
            assert!(loaded.verify_destination(&path).is_err());
        }
    }
}
