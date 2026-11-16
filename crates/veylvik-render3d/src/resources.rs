//! GPU resources the 3D renderer keeps between frames: meshes by geometry, and the panorama converted into the studio environment layout.
use crate::{ENVIRONMENT_TEXELS, Engine, GpuMesh, MESH_CACHE, error};
use veylvik_render::Result;
use wgpu::util::DeviceExt;

impl Engine {
    /// Convert a premultiplied panorama of `width` x `height` into the studio
    /// environment layout, on `encoder`; returns the buffer to light with.
    pub(crate) fn panorama(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        image: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> Result<wgpu::Buffer> {
        let target = {
            let mut slot = self
                .panorama
                .lock()
                .map_err(|_| error("panorama lock poisoned"))?;
            slot.get_or_insert_with(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("3D panorama"),
                    size: ENVIRONMENT_TEXELS * 16,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
            .clone()
        };
        let params = |level: u32| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("panorama level"),
                contents: bytemuck::cast_slice(&[width, height, level, 0u32]),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        let mut run = |pipeline: &wgpu::ComputePipeline,
                       bindings: &[(u32, &wgpu::Buffer)],
                       size: (u32, u32)| {
            let entries: Vec<wgpu::BindGroupEntry> = bindings
                .iter()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("panorama"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(8), size.1.div_ceil(8), 1);
        };
        let first = params(0);
        run(
            &self.resample,
            &[(0, &first), (1, image), (2, &target)],
            (1024, 512),
        );
        for level in 1..=8u32 {
            let p = params(level);
            run(
                &self.reduce,
                &[(0, &p), (2, &target)],
                ((1024 >> level).max(1), (512 >> level).max(1)),
            );
        }
        run(&self.irradiance, &[(2, &target)], (64, 32));
        Ok(target)
    }
    /// Upload (or reuse) the mesh of an object; meshes are keyed by their geometry.
    pub(crate) fn mesh(
        &self,
        device: &wgpu::Device,
        object: &veylvik_scene::Object,
    ) -> Result<(wgpu::Buffer, wgpu::Buffer, u32)> {
        let key = format!("{:?}", object.geometry);
        let mut guard = self
            .meshes
            .lock()
            .map_err(|_| error("mesh cache lock poisoned"))?;
        let (meshes, clock) = &mut *guard;
        *clock += 1;
        if let Some(m) = meshes.get_mut(&key) {
            m.used = *clock;
            return Ok((m.vertices.clone(), m.indices.clone(), m.count));
        }
        let mesh = veylvik_scene::mesh(object).map_err(error)?;
        let vertices: Vec<f32> = mesh
            .vertices
            .iter()
            .flat_map(|v| v.position.into_iter().chain(v.normal).chain(v.uv))
            .collect();
        let mut indices = mesh.indices;
        if indices.is_empty() {
            indices = vec![0, 0, 0];
        }
        let vertex_bytes: &[u8] = if vertices.is_empty() {
            &[0; 32]
        } else {
            bytemuck::cast_slice(&vertices)
        };
        let entry = GpuMesh {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D vertices"),
                contents: vertex_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: if mesh.vertices.is_empty() {
                0
            } else {
                indices.len() as u32
            },
            used: *clock,
        };
        if meshes.len() >= MESH_CACHE
            && let Some(oldest) = meshes
                .iter()
                .min_by_key(|(_, m)| m.used)
                .map(|(k, _)| k.clone())
        {
            meshes.remove(&oldest);
        }
        let out = (entry.vertices.clone(), entry.indices.clone(), entry.count);
        meshes.insert(key, entry);
        Ok(out)
    }
}
