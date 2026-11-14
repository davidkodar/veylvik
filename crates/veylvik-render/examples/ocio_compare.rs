//! Compare an external OCIO configuration's GPU output with its reference processor.
//! cargo run -p veylvik-render --example ocio_compare -- config.ocio source display view destination
use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("Expected config, source, display, view and destination".into());
    }
    let config = veylvik_color::Config::load(&args[0])?;
    let program = config.processor(&args[1], &args[4], &args[2], &args[3])?;
    let shader = program.shader()?;
    let renderer = Renderer::new()?;
    let processor = ocio::OcioProcessor::new(&renderer, &shader)?;
    let pixels: Vec<_> = (0..1024)
        .map(|i| {
            let a = if i % 7 == 0 { 0. } else { 0.5 };
            let x = (i % 32) as f32 / 31.;
            let y = (i / 32) as f32 / 31.;
            [(x * x * 16. - 0.05) * a, y * 4. * a, (1. - x) * y * a, a]
        })
        .collect();
    let mut doc = blank(32, 32, Time::new(24, 1)?)?;
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
        asset: "samples".into(),
    });
    let mut inputs = ImageInputs::default();
    inputs.insert("samples", Arc::new(Image::new(32, 32, pixels.clone())?));
    let frame = renderer.render_gpu(
        &Plan::compile(&doc)?,
        RenderRequest::full(Time::new(0, 1)?),
        &inputs,
    )?;
    let actual = renderer.read_frame(&processor.apply(&renderer, &frame)?)?;
    let mut maximum = 0f32;
    for (input, actual) in pixels.into_iter().zip(actual.pixels) {
        let expected = if input[3] == 0. {
            [0.; 4]
        } else {
            let p = program.reference([
                input[0] / input[3],
                input[1] / input[3],
                input[2] / input[3],
                1.,
            ])?;
            [p[0] * input[3], p[1] * input[3], p[2] * input[3], input[3]]
        };
        for (a, e) in actual.into_iter().zip(expected) {
            if !a.is_finite() {
                return Err("Non-finite GPU output".into());
            }
            maximum = maximum.max((a - e).abs());
        }
    }
    println!(
        "1024 HDR/negative/alpha samples; {} LUTs; maximum absolute error {maximum:.8}",
        shader.textures.len()
    );
    if maximum > 0.001 {
        return Err("OCIO GPU comparison exceeds tolerance".into());
    }
    Ok(())
}
