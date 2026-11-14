//! Render library samples using the same material operations as compositions.
use std::sync::Arc;
use veylvik_document::{Node, Operation, Time, presets::builtin};
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output =
        std::path::PathBuf::from(std::env::args().nth(1).ok_or("output directory required")?);
    std::fs::create_dir_all(&output)?;
    let renderer = Renderer::new()?;
    let side = 256_u32;
    for entry in builtin::ALL.iter().filter(|e| builtin::is_material(e.id)) {
        let preset = builtin::build(entry.id)?;
        let mut operation = preset
            .nodes
            .values()
            .find_map(|n| {
                if let Operation::Material(m) = &n.operation {
                    Some(m.clone())
                } else {
                    None
                }
            })
            .ok_or("missing material")?;
        let sphere = matches!(
            entry.id,
            "gold_text" | "silver_text" | "aluminium_text" | "bronze_text" | "phosphor_text"
        ) || entry.id.starts_with("leather");
        if sphere {
            let pixels = renderer.material_swatch(
                operation.kind,
                operation.roughness.at(Time::new(0, 1)?)?,
                operation.grain.at(Time::new(0, 1)?)?,
            )?;
            veylvik_io::save_png(
                &output.join(format!("{}.png", entry.id)),
                side,
                side,
                &pixels,
            )?;
            continue;
        }
        operation.scale = 80.0.into();
        operation.bevel = 32.0.into();
        operation.lighting = 0.08.into();
        let mut doc = veylvik_document::blank(side, side, Time::new(24, 1)?)?;
        let source = doc.output;
        doc.nodes.get_mut(&source).unwrap().operation =
            Operation::Image(veylvik_document::nodes::Image {
                asset: "sample".into(),
            });
        let material = Node::new(
            "Material sample",
            Operation::Material(operation),
            &[("image", source)],
        );
        doc.output = material.id;
        doc.nodes.insert(material.id, material);
        let mut coverage = Vec::new();
        let pixels = (0..side * side)
            .map(|i| {
                let x = (i % side) as f32 + 0.5;
                let y = (i / side) as f32 + 0.5;
                let r = ((x - 128.).powi(2) + (y - 124.).powi(2)).sqrt() / 108.;
                let a = if sphere {
                    (1. - r * r).max(0.).sqrt()
                } else {
                    ((x - 20.).min(236. - x).min(y - 20.).min(236. - y) / 12.).clamp(0., 1.)
                };
                coverage.push(if sphere {
                    ((1. - r) * 108.).clamp(0., 1.)
                } else {
                    a
                });
                [a, a, a, a]
            })
            .collect();
        let mut inputs = ImageInputs::default();
        inputs.insert("sample", Arc::new(Image::new(side, side, pixels)?));
        let mask = inputs.get("sample").unwrap();
        let mut rendered = renderer
            .render_frame(
                &Plan::compile(&doc)?,
                RenderRequest::full(Time::new(0, 1)?),
                &inputs,
            )?
            .pixels;
        // The dome alpha supplies the material shader's bevel normals. Present
        // the resulting colour on an opaque sample, retaining silhouette AA.
        for ((pixel, mask), &coverage) in rendered.iter_mut().zip(mask.pixels()).zip(&coverage) {
            let a = mask[3];
            for channel in pixel.iter_mut().take(3) {
                *channel = if a > 0.0001 {
                    *channel / a * coverage
                } else {
                    0.
                };
            }
            pixel[3] = coverage;
        }
        veylvik_io::save_png(
            &output.join(format!("{}.png", entry.id)),
            side,
            side,
            &rendered,
        )?;
        println!("Rendered {}", entry.id);
    }
    std::fs::write(
        output.join("render-version.txt"),
        format!("{}\n", veylvik_render::material_preview_revision()),
    )?;
    Ok(())
}
