//! Regenerate bundled gallery loops: cargo run -p veylvik-io --example showcase_previews -- <output-directory>
use veylvik_document::{
    Time,
    presets::{builtin, showcase},
};
use veylvik_render::{Plan, Renderer, to_srgba8};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output =
        std::path::PathBuf::from(std::env::args().nth(1).ok_or("output directory required")?);
    std::fs::create_dir_all(&output)?;
    let renderer = Renderer::new()?;
    for entry in builtin::ALL {
        let scene = showcase::build(entry.id)?;
        for (label, node) in [
            ("before", scene.before),
            ("after", scene.composition.output),
        ] {
            let plan = Plan::compile_output(&scene.composition, node)?;
            let mut atlas = image::RgbaImage::new(2560, 1080);
            for frame in 0..48 {
                let pixels = renderer.render(&plan, Time::new(frame, 12)?)?;
                let full = image::RgbaImage::from_raw(960, 540, to_srgba8(&pixels))
                    .ok_or("invalid image")?;
                let thumb =
                    image::imageops::resize(&full, 320, 180, image::imageops::FilterType::Lanczos3);
                image::imageops::replace(&mut atlas, &thumb, (frame % 8) * 320, (frame / 8) * 180);
            }
            atlas.save(output.join(format!("{}-{label}.png", entry.id)))?;
            println!("Rendered {} {label}", entry.id);
        }
    }
    Ok(())
}
