//! Bake a licensed HDR environment into the renderer's GGX/diffuse lookup on Vulkan.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("HDR input required")?;
    let output = args.next().ok_or("output required")?;
    let neutral = args.any(|a| a == "--neutral");
    let image = image::ImageReader::open(input)?.decode()?.to_rgba32f();
    let pixels: Vec<[f32; 4]> = image.pixels().map(|p| p.0).collect();
    let renderer = veylvik_render::Renderer::new()?;
    let values = renderer.prefilter_environment(image.width(), image.height(), &pixels, neutral)?;
    let mut bytes = Vec::with_capacity(values.len() * 16);
    for pixel in values {
        for channel in pixel {
            bytes.extend_from_slice(&channel.to_le_bytes());
        }
    }
    std::fs::write(output, bytes)?;
    Ok(())
}
