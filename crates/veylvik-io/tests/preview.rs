use veylvik_io::preview::thumbnail;
fn decoded(pixels: &[[f32; 4]], width: u32, height: u32, edge: u32) -> image::RgbaImage {
    let preview = thumbnail(width, height, pixels, edge).unwrap();
    let image = image::load_from_memory(&preview.png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (preview.width, preview.height));
    image
}
#[test]
fn preview_filters_linear_premultiplied_pixels_and_preserves_fractional_coverage() {
    let transparent_edge = decoded(&[[1.0, 0.0, 0.0, 1.0], [0.0; 4]], 2, 1, 1);
    assert_eq!(transparent_edge.get_pixel(0, 0).0, [255, 0, 0, 128]);
    let gray = decoded(&[[0.0, 0.0, 0.0, 1.0], [1.0; 4]], 2, 1, 1);
    assert_eq!(gray.get_pixel(0, 0).0, [188, 188, 188, 255]);
    let uneven = decoded(
        &[[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0], [1.0; 4]],
        3,
        1,
        2,
    );
    assert_eq!(uneven.dimensions(), (2, 1));
    assert_eq!(uneven.get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert_eq!(uneven.get_pixel(1, 0).0, [213, 213, 213, 255]);
    let full = decoded(&[[1.0, 0.0, 0.0, 0.5], [0.0; 4]], 2, 1, 512);
    assert_eq!(full.dimensions(), (2, 1));
    assert_eq!(full.get_pixel(0, 0).0, [255, 0, 0, 128]);
    assert_eq!(full.get_pixel(1, 0).0, [0; 4]);
    let tall = decoded(&vec![[1.0; 4]; 7 * 19], 7, 19, 8);
    assert_eq!(tall.dimensions(), (3, 8));
}
#[test]
fn previews_reject_invalid_bounds_pixels_and_nonfinite_input() {
    for (width, height, edge) in [(0, 1, 1), (8193, 1, 1), (1, 1, 0), (1, 1, 1025), (2, 1, 1)] {
        assert!(thumbnail(width, height, &[[1.0; 4]], edge).is_err());
    }
    for pixel in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, -1.0],
        [0.0, 0.0, 0.0, 2.0],
    ] {
        assert!(thumbnail(1, 1, &[pixel], 1).is_err());
    }
    let extreme = decoded(&[[f32::MAX, -f32::MAX, 0.0, 1.0]; 100], 10, 10, 1);
    assert_eq!(extreme.get_pixel(0, 0).0, [255, 0, 0, 255]);
}
