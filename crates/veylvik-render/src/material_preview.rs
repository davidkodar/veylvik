//! Offline library sample rendering shares the composition material shader.
use crate::{Params, Renderer, Result, error};
impl Renderer {
    /// Render a 256px material sphere under the library's fixed lighting rig.
    /// Used by the asset generator, never on the UI thread.
    pub fn material_swatch(
        &self,
        kind: veylvik_document::MaterialKind,
        roughness: f32,
        grain: f32,
    ) -> Result<Vec<[f32; 4]>> {
        if !roughness.is_finite() || !grain.is_finite() {
            return Err(error("Invalid material sample"));
        }
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("material library sample"),
                source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("material library sample"),
                layout: None,
                module: &module,
                entry_point: Some("material_swatch"),
                compilation_options: Default::default(),
                cache: None,
            });
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("Resource pool unavailable"))?;
        if pool.failed {
            return Err(error("Renderer unavailable after GPU failure"));
        }
        let mut resources = pool.frame(self.budget_bytes);
        let uniform = resources.take(
            &self.device,
            48,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "material preview parameters",
        )?;
        let bytes = 256 * 256 * 16;
        let output = resources.take(
            &self.device,
            bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            "material preview",
        )?;
        let readback = resources.take(
            &self.device,
            bytes,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            "material preview readback",
        )?;
        let params = Params {
            size_op: [256, 256, 0, kind as u32],
            values: [80., 32., roughness, 0.],
            extra: [grain, 1., 1., 0.],
        };
        self.queue
            .write_buffer(&uniform, 0, bytemuck::bytes_of(&params));
        use wgpu::util::DeviceExt;
        let lighting = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sample lighting"),
                contents: bytemuck::bytes_of(&crate::studio_lighting::Lighting::new(
                    kind,
                    None,
                    &Default::default(),
                )),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material preview"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: self.studio_environment.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.ltc.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: lighting.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(32, 32, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        resources.submitted();
        let values = self.map_values(&readback, self.queue.submit([encoder.finish()]))?;
        resources.completed();
        Ok(values)
    }
}

/// Asset provenance for the material library. Changes to shading, GPU packing,
/// sample geometry or authored defaults require regenerated library previews.
pub fn material_preview_revision() -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut feed = |bytes: &[u8]| {
        for &byte in bytes {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    };
    feed(include_bytes!("kernels.wgsl"));
    feed(include_bytes!("material_preview.rs"));
    feed(include_bytes!("studio_lighting.rs"));
    feed(include_bytes!("../assets/ltc/ggx.bin"));
    feed(include_bytes!("../assets/studio/filtered.bin"));
    for kind in veylvik_document::MaterialKind::ALL {
        feed(&serde_json::to_vec(&kind.optics()).expect("finite optical defaults"));
    }
    feed(
        &serde_json::to_vec(&veylvik_document::optical::StudioRig::default())
            .expect("finite studio defaults"),
    );
    for entry in veylvik_document::presets::builtin::ALL
        .iter()
        .filter(|e| veylvik_document::presets::builtin::is_material(e.id))
    {
        let preset =
            veylvik_document::presets::builtin::build(entry.id).expect("valid built-in material");
        for node in preset.nodes.values() {
            if let veylvik_document::Operation::Material(material) = &node.operation {
                feed(&serde_json::to_vec(material).expect("finite built-in material"));
            }
        }
    }
    format!("{hash:016x}")
}
