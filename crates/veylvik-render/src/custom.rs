//! Custom shader nodes: the user's checked code is compiled into its own compute pipeline on the kernels' bind group layout, validated by naga first so a mistake is an error with a line, never a device fault. Pipelines are cached by source.
use crate::{Result, error};
use std::sync::Mutex;
use wgpu::naga;

/// Compiled shaders kept, most recently used last.
const KEPT: usize = 24;
#[derive(Default)]
pub(crate) struct Cache(Mutex<Vec<(String, wgpu::ComputePipeline)>>);

/// The full program for `code`, after the text rules and a naga parse and validation.
/// Errors name the user's own line.
fn program(code: &str) -> std::result::Result<String, String> {
    let controls = veylvik_shader::check(code)?;
    let (program, before) = veylvik_shader::assemble(code, &controls);
    let line = |location: Option<naga::SourceLocation>| {
        location
            .map(|l| (l.line_number as usize).saturating_sub(before))
            .filter(|l| *l >= 1 && *l <= code.lines().count().max(1))
    };
    let at = |line: Option<usize>, message: String| match line {
        Some(line) => format!("line {line}: {message}"),
        None => message,
    };
    let module = naga::front::wgsl::parse_str(&program)
        .map_err(|e| at(line(e.location(&program)), e.message().to_string()))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| {
        // The innermost cause says what is wrong; the outer ones only where.
        let mut cause: &dyn std::error::Error = e.as_inner();
        let mut message = cause.to_string();
        while let Some(next) = cause.source() {
            cause = next;
            message = format!("{message}: {cause}");
        }
        at(line(e.location(&program)), message)
    })?;
    Ok(program)
}
/// Whether a custom shader's code is acceptable; the error names the line.
pub fn check(code: &str) -> std::result::Result<(), String> {
    program(code).map(|_| ())
}
impl Cache {
    /// The pipeline for `code`, compiled on first use.
    pub(crate) fn pipeline(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        code: &str,
    ) -> Result<wgpu::ComputePipeline> {
        let mut kept = self
            .0
            .lock()
            .map_err(|_| error("shader cache lock poisoned"))?;
        if let Some(i) = kept.iter().position(|(source, _)| source == code) {
            let entry = kept.remove(i);
            let pipeline = entry.1.clone();
            kept.push(entry);
            return Ok(pipeline);
        }
        let program = program(code).map_err(|e| error(format!("Custom shader: {e}")))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("custom shader"),
            source: wgpu::ShaderSource::Wgsl(program.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("custom shader"),
            layout: Some(layout),
            module: &module,
            entry_point: Some("custom"),
            compilation_options: Default::default(),
            cache: None,
        });
        if kept.len() >= KEPT {
            kept.remove(0);
        }
        kept.push((code.to_owned(), pipeline.clone()));
        Ok(pipeline)
    }
}
