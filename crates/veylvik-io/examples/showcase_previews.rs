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
    let filter = std::env::args().nth(2);
    let still = std::env::args().nth(3).as_deref() == Some("--still");
    for entry in builtin::ALL {
        if filter.as_deref().is_some_and(|id| id != entry.id) {
            continue;
        }
        let scene = showcase::build(entry.id)?;
        for (label, node) in [
            ("before", scene.before),
            ("after", scene.composition.output),
        ] {
            let plan = Plan::compile_output(&scene.composition, node)?;
            if still {
                let pixels = renderer.render(&plan, Time::new(2, 1)?)?;
                veylvik_io::save_png(
                    &output.join(format!("{}-{label}.png", entry.id)),
                    scene.composition.width,
                    scene.composition.height,
                    &pixels,
                )?;
                continue;
            }
            let mut atlas = image::RgbaImage::new(2560, 1080);
            for frame in 0..48 {
                let pixels = renderer.render(&plan, Time::new(frame, 12)?)?;
                let full = image::RgbaImage::from_raw(
                    scene.composition.width,
                    scene.composition.height,
                    to_srgba8(&pixels),
                )
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
