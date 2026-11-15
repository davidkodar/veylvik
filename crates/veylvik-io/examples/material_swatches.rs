//! Render library samples using the same material operations as compositions.
use veylvik_document::{Operation, presets::builtin};
use veylvik_render::Renderer;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output =
        std::path::PathBuf::from(std::env::args().nth(1).ok_or("output directory required")?);
    std::fs::create_dir_all(&output)?;
    let filmic = std::env::args().any(|a| a == "--filmic");
    let view = |pixels: Vec<[f32; 4]>| {
        if filmic {
            veylvik_render::display::apply(
                veylvik_document::media::DisplayTransform::Filmic,
                &pixels,
            )
        } else {
            pixels
        }
    };
    let renderer = Renderer::new()?;
    let side = 256_u32;
    for entry in builtin::ALL.iter().filter(|e| builtin::is_material(e.id)) {
        let preset = builtin::build(entry.id)?;
        let material = preset
            .nodes
            .values()
            .find_map(|n| match &n.operation {
                Operation::Material(m) => Some(m),
                _ => None,
            })
            .ok_or("missing material")?;
        let pixels = renderer.material_sample(material)?;
        veylvik_io::save_png(
            &output.join(format!("{}.png", entry.id)),
            side,
            side,
            &view(pixels),
        )?;
        println!("Rendered {}", entry.id);
    }
    std::fs::write(
        output.join("render-version.txt"),
        format!(
            "{}{}\n",
            veylvik_render::material_preview_revision(),
            if filmic { ":filmic" } else { "" }
        ),
    )?;
    Ok(())
}
