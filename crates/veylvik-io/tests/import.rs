use exr::prelude::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scratch() -> Scratch {
    let root = Scratch(std::env::temp_dir().join(format!(
            "veylvik-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&root.0).unwrap();
    root
}
#[test]
fn exr_import_preserves_hdr_float_bits_and_accepts_lossless_half_scanlines() {
    let root = scratch();
    let path = root.0.join("source.EXR");
    let pixels = [
        [3.25, -0.25, 0.125, 0.5],
        [-0.0, f32::MIN_POSITIVE, f32::MAX, 1.0],
        [0.0; 4],
        [-12.0, 16.0, 0.0, 0.25],
    ];
    veylvik_io::save_frame(&path, 2, 2, &pixels).unwrap();
    let original = std::fs::read(&path).unwrap();
    let image = veylvik_io::load_image(&path, 64).unwrap();
    assert_eq!((image.width(), image.height()), (2, 2));
    assert_eq!(
        image
            .pixels()
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        pixels
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
    assert!(veylvik_io::load_exr(&path, 63).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    for compression in [
        Compression::Uncompressed,
        Compression::RLE,
        Compression::ZIP1,
        Compression::ZIP16,
    ] {
        let image = Image::from_encoded_channels(
            (3, 2),
            Encoding {
                compression,
                blocks: Blocks::ScanLines,
                line_order: LineOrder::Increasing,
            },
            SpecificChannels::rgba(|_| {
                (
                    f16::from_f32(2.0),
                    f16::from_f32(-0.25),
                    f16::from_f32(0.125),
                    f16::from_f32(0.5),
                )
            }),
        );
        image.write().non_parallel().to_file(&path).unwrap();
        assert_eq!(
            veylvik_io::load_exr(&path, 96).unwrap().pixels(),
            &[[2.0, -0.25, 0.125, 0.5]; 6]
        );
    }
}
#[test]
fn exr_import_rejects_unsupported_metadata_pixels_and_malformed_lengths() {
    let root = scratch();
    let path = root.0.join("input.exr");
    for rgba in [
        (f32::NAN, 0.0, 0.0, 1.0),
        (0.0, 0.0, 0.0, 1.1),
        (1.0, 0.0, 0.0, 0.0),
    ] {
        let image = Image::from_encoded_channels(
            (1, 1),
            Encoding::SMALL_LOSSLESS,
            SpecificChannels::rgba(move |_| rgba),
        );
        image.write().non_parallel().to_file(&path).unwrap();
        assert!(veylvik_io::load_exr(&path, 16).is_err());
    }
    for change in 0..5 {
        let mut image = Image::from_encoded_channels(
            (2, 2),
            Encoding::SMALL_LOSSLESS,
            SpecificChannels::rgba(|_| (0.0f32, 0.0f32, 0.0f32, 1.0f32)),
        );
        match change {
            0 => image.attributes.pixel_aspect = 2.0,
            1 => image.layer_data.attributes.layer_position = Vec2(1, 0),
            2 => image.attributes.display_window.size = Vec2(3, 2),
            3 => {
                image.attributes.chromaticities = Some(exr::meta::attribute::Chromaticities {
                    red: Vec2(0.68, 0.32),
                    green: Vec2(0.265, 0.69),
                    blue: Vec2(0.15, 0.06),
                    white: Vec2(0.3127, 0.3290),
                })
            }
            _ => image.layer_data.encoding.blocks = Blocks::Tiles(Vec2(2, 2)),
        }
        image.write().non_parallel().to_file(&path).unwrap();
        assert!(
            veylvik_io::load_exr(&path, 64).is_err(),
            "unsupported case {change}"
        );
    }
    let image = Image::from_encoded_channels(
        (1, 1),
        Encoding::SMALL_LOSSLESS,
        SpecificChannels::rgb(|_| (1.0f32, 0.0f32, 0.0f32)),
    );
    image.write().non_parallel().to_file(&path).unwrap();
    assert!(veylvik_io::load_exr(&path, 16).is_err());
    // Attribute length claiming gigabytes must fail before decoder allocation.
    let mut malformed = vec![0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0];
    malformed.extend_from_slice(b"huge\0string\0");
    malformed.extend_from_slice(&i32::MAX.to_le_bytes());
    for bytes in [&malformed[..], &malformed[..10], b"not an EXR".as_slice()] {
        std::fs::write(&path, bytes).unwrap();
        assert!(veylvik_io::load_exr(&path, 64).is_err());
    }
}
