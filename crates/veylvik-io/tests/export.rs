use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use veylvik_io::{FrameFormat, save_frame};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn exr_preserves_float_bits_chromaticities_and_original_on_failure() {
    use exr::prelude::*;
    let root = Scratch(std::env::temp_dir().join(format!(
            "veylvik-exr-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&root.0).unwrap();
    let path = root.0.join("frame.EXR");
    let pixels = vec![
        [3.25, -0.25, 0.125, 0.5],
        [0.7, 0.4, 0.1, 0.0],
        [-0.0, f32::MIN_POSITIVE, f32::MAX, 1.0],
        [-12.0, 16.0, 0.0, 0.25],
    ];
    save_frame(&path, 2, 2, &pixels).unwrap();
    let image = read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |size, _| vec![vec![[0.0_f32; 4]; size.width()]; size.height()],
            |pixels: &mut Vec<Vec<[f32; 4]>>, position, rgba: (f32, f32, f32, f32)| {
                pixels[position.y()][position.x()] = [rgba.0, rgba.1, rgba.2, rgba.3];
            },
        )
        .first_valid_layer()
        .all_attributes()
        .non_parallel()
        .from_file(&path)
        .unwrap();
    assert_eq!(image.layer_data.size, Vec2(2, 2));
    assert_eq!(image.layer_data.encoding.compression, Compression::ZIP16);
    let actual: Vec<_> = image
        .layer_data
        .channel_data
        .pixels
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(
        actual
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
    let colour = image.attributes.chromaticities.unwrap();
    assert_eq!(colour.red, Vec2(0.64, 0.33));
    assert_eq!(colour.green, Vec2(0.30, 0.60));
    assert_eq!(colour.blue, Vec2(0.15, 0.06));
    assert_eq!(colour.white, Vec2(0.3127, 0.3290));
    let original = std::fs::read(&path).unwrap();
    for (w, h, p) in [
        (0, 2, pixels.clone()),
        (2, 3, pixels.clone()),
        (1, 1, vec![[f32::NAN, 0.0, 0.0, 1.0]]),
        (1, 1, vec![[0.0, 0.0, 0.0, 1.1]]),
    ] {
        assert!(save_frame(&path, w, h, &p).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let failed = veylvik_document::atomic_write_with(&path, |file| {
        use std::io::Write;
        file.write_all(b"partial")?;
        Err(std::io::Error::other("injected encoder failure"))
    });
    assert!(failed.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 1);
    assert_eq!(FrameFormat::from_path(&path).unwrap(), FrameFormat::Exr);
    assert!(save_frame(&root.0.join("unknown.bin"), 2, 2, &pixels).is_err());
    let png = root.0.join("frame.png");
    save_frame(&png, 2, 2, &pixels).unwrap();
    assert_eq!(&std::fs::read(png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
}
