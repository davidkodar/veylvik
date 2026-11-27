//! GPU resources the 3D renderer keeps between frames: meshes by geometry, and the panorama converted into the studio environment layout.
use crate::{ENVIRONMENT_TEXELS, Engine, GpuMesh, MESH_CACHE, error};
use veylvik_render::Result;
use veylvik_scene::Geometry;
use wgpu::util::DeviceExt;

impl Engine {
    /// Convert a premultiplied panorama of `width` x `height` (or, with `frame`,
    /// an ordinary frame of the shot) into the studio environment layout, on `encoder`; returns the buffer to light with.
    pub(crate) fn panorama(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        image: &wgpu::Buffer,
        [width, height]: [u32; 2],
        frame: bool,
        identity: Option<u64>,
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
        // The same picture as last time: what it was converted to is still there.
        let key = identity.map(|id| (id, [width, height], frame));
        {
            let mut converted = self
                .converted
                .lock()
                .map_err(|_| error("panorama lock poisoned"))?;
            if key.is_some() && *converted == key {
                return Ok(target);
            }
            // Another picture: what the buffer holds is no one's until it is made.
            *converted = None;
        }
        let params = |level: u32| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("panorama level"),
                contents: bytemuck::cast_slice(&[width, height, level, u32::from(frame)]),
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
        if let Ok(mut converted) = self.converted.lock() {
            *converted = key;
        }
        Ok(target)
    }
    /// Upload (or reuse) the mesh of an object; meshes are keyed by their geometry.
    pub(crate) fn mesh(
        &self,
        device: &wgpu::Device,
        object: &veylvik_scene::Object,
    ) -> Result<(wgpu::Buffer, wgpu::Buffer, u32)> {
        // A fluttering object's mesh changes with its waves and their moment.
        let key = format!("{:?}{:?}", object.geometry, object.flutter);
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
        let mesh = match &object.geometry {
            Geometry::Model {
                path,
                size,
                part: Some(part),
                ..
            } => {
                let models = self
                    .models
                    .lock()
                    .map_err(|_| error("model lock poisoned"))?;
                let mut mesh = models
                    .get(path)
                    .and_then(|m| m.parts.get(*part))
                    .map(|p| p.mesh.clone())
                    .unwrap_or_default();
                for v in &mut mesh.vertices {
                    v.position = v.position.map(|x| x * size);
                }
                // A model's parts wave as one: across the whole model's width.
                if let (Some(waves), Geometry::Model { extent, .. }) =
                    (&object.flutter, &object.geometry)
                {
                    let half = extent[0] * size * 0.5;
                    veylvik_scene::flutter(&mut mesh, waves, Some([-half, half]));
                }
                mesh
            }
            _ => veylvik_scene::mesh(object).map_err(error)?,
        };
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
    /// Hold this frame's models, and split each model object into its parts: each
    /// keeps the object's place and takes the part's material, unless the object has
    /// a library look. Models that are not loaded show nothing.
    pub(crate) fn expand(
        &self,
        scene: &veylvik_scene::Scene,
        models: &std::collections::BTreeMap<String, std::sync::Arc<veylvik_scene::Model>>,
    ) -> Result<veylvik_scene::Scene> {
        *self
            .models
            .lock()
            .map_err(|_| error("model lock poisoned"))? = models.clone();
        if !scene
            .objects
            .iter()
            .any(|o| matches!(o.geometry, Geometry::Model { .. }))
        {
            return Ok(scene.clone());
        }
        let mut out = scene.clone();
        out.objects.clear();
        for object in &scene.objects {
            let Geometry::Model {
                path,
                size,
                extent,
                part: None,
                play,
            } = &object.geometry
            else {
                out.objects.push(object.clone());
                continue;
            };
            let Some(model) = models.get(path) else {
                continue;
            };
            // Playing one of its animations: each part moved from its rest by it (in the
            // normalised frame, so its move is scaled to the model's size here).
            let poses =
                play.and_then(|(clip, seconds, looping)| model.poses(clip, seconds, looping));
            for (i, part) in model.parts.iter().enumerate() {
                let mut piece = object.clone();
                piece.geometry = Geometry::Model {
                    path: path.clone(),
                    size: *size,
                    extent: *extent,
                    part: Some(i),
                    // The part's mesh is the same at every moment: it is kept once.
                    play: None,
                };
                if let Some(pose) = poses.as_ref().and_then(|p| p.get(i)) {
                    let mut moved = *pose;
                    for row in moved.iter_mut().take(3) {
                        row[3] *= size;
                    }
                    piece.world = veylvik_scene::math::mul(&object.world, &moved);
                }
                if piece.surface.look.is_none() {
                    piece.surface.colour = [part.colour[0], part.colour[1], part.colour[2]];
                    piece.surface.metallic = part.metallic;
                    piece.surface.roughness = part.roughness;
                }
                out.objects.push(piece);
            }
        }
        Ok(out)
    }
    /// A part's base colour texture on the GPU (straight linear RGBA rows), cached by
    /// the loaded texture; None for an untextured part.
    pub(crate) fn part_texture(
        &self,
        device: &wgpu::Device,
        geometry: &Geometry,
    ) -> Result<Option<(wgpu::Buffer, u32, u32)>> {
        let Geometry::Model {
            path,
            part: Some(part),
            ..
        } = geometry
        else {
            return Ok(None);
        };
        let texture = {
            let models = self
                .models
                .lock()
                .map_err(|_| error("model lock poisoned"))?;
            models
                .get(path)
                .and_then(|m| m.parts.get(*part))
                .and_then(|p| p.texture.clone())
        };
        let Some(texture) = texture else {
            return Ok(None);
        };
        let key = std::sync::Arc::as_ptr(&texture) as usize;
        let mut cache = self
            .textures
            .lock()
            .map_err(|_| error("texture cache lock poisoned"))?;
        if let Some((buffer, _)) = cache.get(&key) {
            return Ok(Some((buffer.clone(), texture.width, texture.height)));
        }
        // Keep the cache to textures still alive in some loaded model.
        if cache.len() >= 256 {
            cache.clear();
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3D model texture"),
            contents: bytemuck::cast_slice(&texture.pixels),
            usage: wgpu::BufferUsages::STORAGE,
        });
        cache.insert(key, (buffer.clone(), texture.clone()));
        Ok(Some((buffer, texture.width, texture.height)))
    }
}
