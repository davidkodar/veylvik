//! Path traced pictures (Render scene's Quality: path traced): the scene's meshes as trees kept between frames under the same keys as the drawn meshes, surfaces and lights handed to the tracer with the meanings the interactive renderer gives them, filmed through the same camera and lens.
use crate::{Engine, TREE_CACHE, error, look_scale};
use std::sync::Arc;
use veylvik_render::{Result, SceneJob, Trace};
use veylvik_scene::Geometry;
use veylvik_trace::{
    CARD, CATCHER, CUT, Environment, GLASS, Job, Look, MeshTree, Placed, Projection, SPLATS,
    STRAIGHT, Settings, SplatBuffers, SplatColour, Surface, Texture, Tracer, UNLIT, View,
};

/// The brightest a bounce may add to a pixel: rare paths that find a small bright light
/// would otherwise show as speckles.
const CLAMP: f32 = 16.0;

impl Engine {
    pub(crate) fn traced(&self, job: SceneJob<'_>, trace: Trace) -> Result<()> {
        let SceneJob {
            device,
            queue,
            encoder,
            scene,
            camera,
            width,
            height,
            images,
            environment,
            blend,
            target,
            models,
            time,
            instances,
            splats,
            ..
        } = job;
        let expanded = self.expand(scene, models)?;
        let scene = &expanded;
        let (panorama, strength, turn) = self.surroundings(device, encoder, scene, images)?;
        // Looks see the scene's environment as the interactive renderer's looks do.
        let look_environment = scene
            .environment
            .map(|e| veylvik_render::EnvironmentOverride {
                strength: e.intensity,
                absolute: panorama.is_some(),
                rotation: e.rotation.to_radians(),
            });
        let mut looks: Vec<Look> = Vec::new();
        let mut placed = Vec::with_capacity(scene.objects.len());
        // The pictures surfaces read, each once: (buffer, width, height).
        let mut pictures: Vec<(wgpu::Buffer, u32, u32)> = Vec::new();
        let mut number = |buffer: &wgpu::Buffer, w: u32, h: u32| -> u32 {
            match pictures.iter().position(|(b, ..)| b == buffer) {
                Some(i) => i as u32,
                None => {
                    pictures.push((buffer.clone(), w, h));
                    (pictures.len() - 1) as u32
                }
            }
        };
        let picture_of = |source| images.iter().find(|(id, ..)| *id == source);
        // Each particle node's copies, read once: (node, where and size, turn, tint).
        let mut read: Vec<(veylvik_document::NodeId, Vec<[[f32; 4]; 3]>)> = Vec::new();
        // The splat sets shown (each on the GPU once, as the preview has it) and the
        // objects showing them: (set, its tree, placing, brightness).
        let mut sets: Vec<(Arc<veylvik_splat::Uploaded>, Arc<veylvik_splat::Splats>)> = Vec::new();
        let mut splatted: Vec<(usize, Arc<MeshTree>, veylvik_scene::math::Mat4, f32)> = Vec::new();
        for object in &scene.objects {
            if let Geometry::Splats {
                path,
                size,
                brightness,
            } = &object.geometry
            {
                let Some(data) = splats.get(path).filter(|d| d.count > 0) else {
                    continue;
                };
                if object.instanced.is_some() {
                    continue;
                }
                let set = self.splat_set(device, data)?;
                let at = match sets.iter().position(|(s, _)| Arc::ptr_eq(s, &set)) {
                    Some(at) => at,
                    None => {
                        sets.push((set, data.clone()));
                        sets.len() - 1
                    }
                };
                // Centred on the box most of its splats lie in, its largest side `size`, as
                // the preview places it.
                let (middle, side) = data.extent();
                let model = veylvik_scene::math::mul(
                    &object.world,
                    &veylvik_scene::math::mul(
                        &veylvik_scene::math::scale([size / side; 3]),
                        &veylvik_scene::math::translate(middle.map(|v| -v)),
                    ),
                );
                splatted.push((at, self.splat_tree(path, data)?, model, *brightness));
                continue;
            }
            // An object copied at particles is placed once for each living particle (and not
            // where it is), as the preview draws it.
            let copies = match object.instanced {
                Some(id) => {
                    if !read.iter().any(|(node, _)| *node == id) {
                        let copies = match instances.iter().find(|i| i.node == id) {
                            Some(found) => read_copies(device, queue, found)?,
                            None => Vec::new(),
                        };
                        read.push((id, copies));
                    }
                    let Some((_, copies)) = read.iter().find(|(node, _)| *node == id) else {
                        continue;
                    };
                    if copies.is_empty() {
                        continue;
                    }
                    Some(copies)
                }
                None => None,
            };
            let s = &object.surface;
            let mut flags = if s.unlit { UNLIT } else { 0 } | if s.catcher { CATCHER } else { 0 };
            // What it shows, as the interactive renderer reads it: a card's picture, a
            // model part's own texture, or a shape's Texture input.
            let mut texture = match &object.geometry {
                Geometry::Card { image, .. } => picture_of(*image).map(|(_, buffer, [w, h], _)| {
                    flags |= CARD;
                    number(buffer, *w, *h)
                }),
                _ if s.catcher => None,
                _ => match self.part_texture(device, &object.geometry)? {
                    Some((buffer, w, h)) => {
                        flags |= STRAIGHT;
                        Some(number(&buffer, w, h))
                    }
                    None => object
                        .texture
                        .and_then(picture_of)
                        .map(|(_, buffer, [w, h], _)| {
                            flags |= CUT;
                            number(buffer, *w, *h)
                        }),
                },
            };
            // A picture cast through a camera: in place of a texture or a look, as in the
            // preview.
            let projection = object
                .projection
                .filter(|_| flags & (CARD | CATCHER) == 0 && copies.is_none())
                .and_then(|cast| {
                    let (_, buffer, [w, h], _) = picture_of(cast.image)?;
                    let to_camera = veylvik_scene::math::rigid_inverse(&cast.camera.world);
                    let eye = veylvik_scene::math::point(&cast.camera.world, [0.0; 3]);
                    Some(Projection {
                        to_camera: [to_camera[0], to_camera[1], to_camera[2]],
                        eye,
                        focal: veylvik_scene::home_distance(
                            width,
                            cast.camera.focal_length,
                            cast.camera.sensor_width,
                        ),
                        picture: number(buffer, *w, *h),
                        lit: cast.lit,
                        hide_unseen: cast.hide_unseen,
                    })
                });
            if projection.is_some() {
                texture = None;
                flags &= !(CUT | STRAIGHT);
            }
            // A library look, as its material's own blocks for the shared surface code.
            let look = match &s.look {
                Some(material)
                    if flags & (CARD | CATCHER) == 0
                        && projection.is_none()
                        && copies.is_none() =>
                {
                    let (params, lighting) = veylvik_render::material_uniforms(
                        material,
                        width,
                        height,
                        time,
                        look_environment,
                    )?;
                    let vec4s = |bytes: &[u8]| -> Vec<[f32; 4]> {
                        bytes
                            .as_chunks::<16>()
                            .0
                            .iter()
                            .map(|c| bytemuck::pod_read_unaligned::<[f32; 4]>(c))
                            .collect()
                    };
                    let params = vec4s(&params[..48]);
                    // A look draws its own surface, as in the preview: a texture neither
                    // cuts nor tints it, but an Image texture look shows the shape's
                    // Texture input, once across it.
                    let pictured = material.kind == veylvik_document::MaterialKind::Texture
                        && flags & CUT != 0;
                    if !pictured {
                        texture = None;
                    }
                    flags &= !(CUT | STRAIGHT);
                    let [x, y] = look_scale(&object.geometry, pictured, [width, height]);
                    looks.push(Look {
                        params: [params[0], params[1], params[2]],
                        lighting: vec4s(&lighting),
                        uv_scale: [x, y],
                    });
                    if material.kind == veylvik_document::MaterialKind::Glass {
                        flags |= GLASS;
                    }
                    Some((looks.len() - 1) as u32)
                }
                _ => None,
            };
            let surface = Surface {
                colour: s.colour,
                roughness: s.roughness,
                metallic: s.metallic,
                emission: s.emission,
                opacity: s.opacity,
                flags,
                texture,
                shadow: s.shadow,
                finish: [
                    s.finish[0],
                    s.finish[1].to_radians(),
                    s.finish[2],
                    s.finish[3],
                    s.finish[4],
                ],
                look,
                projection,
                splats: None,
            };
            let tree = self.tree(object)?;
            match copies {
                None => placed.push(Placed {
                    tree,
                    world: object.world,
                    surface,
                    tint: [1.0; 3],
                }),
                Some(copies) => {
                    for [at, turn, tint] in copies.iter().filter(|c| c[0][3] > 0.0) {
                        placed.push(Placed {
                            tree: tree.clone(),
                            world: copied(&object.world, *at, *turn),
                            surface,
                            tint: [tint[0], tint[1], tint[2]],
                        });
                    }
                }
            }
        }
        // Every set the scene shows, one after another: one is read where it is; several
        // are copied into buffers of their own for this frame.
        let mut firsts = Vec::with_capacity(sets.len());
        let (mut splat_count, mut rest_count) = (0u64, 0u64);
        for (set, _) in &sets {
            let (data, rest) = set.buffers();
            firsts.push((splat_count as u32, rest_count as u32));
            splat_count += data.size() / 64;
            rest_count += rest.size() / 4;
        }
        if splat_count > u64::from(u32::MAX / 4) || rest_count > u64::from(u32::MAX) {
            return Err(error("too many splats to path trace together"));
        }
        for (at, tree, model, brightness) in splatted {
            let (set, data) = &sets[at];
            let _ = set;
            placed.push(Placed {
                tree,
                world: model,
                surface: Surface {
                    colour: [brightness; 3],
                    flags: SPLATS,
                    splats: Some(SplatColour {
                        first: firsts[at].0,
                        degree: data.degree,
                        coefficients: data.coefficients() as u32,
                        rest: firsts[at].1,
                    }),
                    ..Surface::default()
                },
                tint: [1.0; 3],
            });
        }
        let joined = (sets.len() > 1).then(|| {
            let make = |label, size| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            let data = make("path traced splats", splat_count * 64);
            let rest = make("path traced splat colours", rest_count * 4);
            for ((set, _), (first, rest_first)) in sets.iter().zip(&firsts) {
                let (d, r) = set.buffers();
                encoder.copy_buffer_to_buffer(d, 0, &data, u64::from(*first) * 64, d.size());
                encoder.copy_buffer_to_buffer(r, 0, &rest, u64::from(*rest_first) * 4, r.size());
            }
            (data, rest)
        });
        let splat_buffers = match (&joined, sets.first()) {
            (Some((data, rest)), _) => Some(SplatBuffers { data, rest }),
            (None, Some((set, _))) => {
                let (data, rest) = set.buffers();
                Some(SplatBuffers { data, rest })
            }
            (None, None) => None,
        };
        // The scene's volumes, as the preview reads them.
        let media: Vec<[[f32; 4]; 8]> = scene
            .volumes
            .iter()
            .filter_map(|v| crate::volumes::medium(v).packed())
            .collect();
        let textures: Vec<Texture<'_>> = pictures
            .iter()
            .map(|(buffer, width, height)| Texture {
                buffer,
                width: *width,
                height: *height,
            })
            .collect();
        let packed = veylvik_trace::pack(&placed);
        let lights: Vec<veylvik_trace::Light> =
            scene.lights.iter().map(|l| light(l, width)).collect();
        // A thin lens of the camera's aperture (scene units are millimetres for the lens),
        // sharp at the distance the camera focuses at.
        let lens_radius = if camera.f_stop > 0.0 {
            camera.focal_length / camera.f_stop * 0.5
        } else {
            0.0
        };
        let tracer = self.tracer.get_or_init(|| {
            Tracer::new(
                device,
                veylvik_render::SURFACE_WGSL,
                veylvik_render::ENVIRONMENT_WGSL,
            )
        });
        tracer
            .render(Job {
                device,
                queue,
                encoder,
                scene: &packed,
                textures: &textures,
                looks: &looks,
                lights: &lights,
                view: View {
                    camera: camera.world,
                    focal: veylvik_scene::home_distance(
                        width,
                        camera.focal_length,
                        camera.sensor_width,
                    ),
                    lens_radius,
                    focus: focus(&camera, scene, &packed),
                },
                environment: Environment {
                    map: panorama.as_ref().unwrap_or(environment),
                    strength,
                    turn,
                    backdrop: scene.environment.is_some_and(|e| e.background),
                },
                width,
                height,
                settings: Settings {
                    samples: trace.samples,
                    bounces: trace.bounces,
                    clamp: CLAMP,
                    sample_sky: true,
                    seed: trace.seed,
                    denoise: trace.denoise.map(|g| veylvik_trace::Gather {
                        gathered: g.gathered,
                        last: g.last,
                    }),
                },
                target,
                blend,
                splats: splat_buffers,
                media: &media,
            })
            .map_err(error)
    }
    /// An object's mesh as a tree, kept between frames under its mesh's key.
    fn tree(&self, object: &veylvik_scene::Object) -> Result<Arc<MeshTree>> {
        let key = format!("{:?}{:?}", object.geometry, object.flutter);
        self.cached_tree(key, || Ok(MeshTree::new(&self.cpu_mesh(object)?)))
    }
    /// The tree over a splat file's splats, in the file's own space (kept like meshes').
    fn splat_tree(&self, path: &str, data: &Arc<veylvik_splat::Splats>) -> Result<Arc<MeshTree>> {
        let key = format!("splats {path} {:p}", Arc::as_ptr(data));
        self.cached_tree(key, || {
            Ok(MeshTree::splats(&data.data, veylvik_splat::STRIDE))
        })
    }
    fn cached_tree(
        &self,
        key: String,
        build: impl FnOnce() -> Result<MeshTree>,
    ) -> Result<Arc<MeshTree>> {
        {
            let mut guard = self
                .trees
                .lock()
                .map_err(|_| error("tree cache lock poisoned"))?;
            let (trees, clock) = &mut *guard;
            *clock += 1;
            let now = *clock;
            if let Some((tree, used)) = trees.get_mut(&key) {
                *used = now;
                return Ok(tree.clone());
            }
        }
        // Built outside the lock: a large mesh takes a moment.
        let tree = Arc::new(build()?);
        let mut guard = self
            .trees
            .lock()
            .map_err(|_| error("tree cache lock poisoned"))?;
        let (trees, clock) = &mut *guard;
        if trees.len() >= TREE_CACHE
            && let Some(oldest) = trees
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            trees.remove(&oldest);
        }
        trees.insert(key, (tree.clone(), *clock));
        Ok(tree)
    }
    /// What the trees kept between frames hold on the CPU, and the tracer on the GPU.
    pub(crate) fn traced_bytes(&self) -> u64 {
        let trees = self.trees.lock().map_or(0, |t| {
            t.0.values()
                .map(|(t, _)| {
                    (t.nodes.len() * 32 + t.vertices.len() * 16 + t.triangles.len() * 16) as u64
                })
                .sum()
        });
        trees + self.tracer.get().map_or(0, Tracer::held_bytes)
    }
    pub(crate) fn release_traced(&self) {
        if let Ok(mut t) = self.trees.lock() {
            t.0.clear();
        }
        if let Some(tracer) = self.tracer.get() {
            tracer.release();
        }
    }
}
/// A light as the tracer reads it: the interactive renderer's light, without its shadow map.
fn light(light: &veylvik_scene::Light, width: u32) -> veylvik_trace::Light {
    use veylvik_document::nodes::LightKind;
    let p = light.position();
    let d = light.direction();
    let outer = (light.cone * 0.5).to_radians();
    let inner = outer * (1.0 - light.softness).max(0.0);
    let panel = light.panel();
    let strength = light.strength(width);
    veylvik_trace::Light {
        position: [
            p[0],
            p[1],
            p[2],
            match light.kind {
                LightKind::Point => 0.0,
                LightKind::Spot => 1.0,
                LightKind::Sun => 2.0,
                LightKind::Area => 3.0,
            },
        ],
        direction: [d[0], d[1], d[2], outer.cos()],
        colour: [
            light.colour[0] * strength,
            light.colour[1] * strength,
            light.colour[2] * strength,
            inner.cos().max(outer.cos() + 1e-4),
        ],
        right: [panel[0][0], panel[0][1], panel[0][2], light.feather],
        // How strongly it shows in shiny surfaces (0 to 1), plus 2 when it casts shadows (a
        // light with Shadows off lights through objects, as in the preview).
        up: [
            panel[1][0],
            panel[1][1],
            panel[1][2],
            light.reflections + if light.shadows { 2.0 } else { 0.0 },
        ],
    }
}
/// How far in front of the camera is sharp, as the interactive renderer takes it: an
/// object it focuses on or its focus distance; with neither (0), whatever is at the frame's
/// centre, or nowhere near (nothing there) so nothing blurs.
fn focus(
    camera: &veylvik_scene::Camera,
    scene: &veylvik_scene::Scene,
    packed: &veylvik_trace::Packed,
) -> f32 {
    if camera.focus_on.is_some() || camera.focus_distance > 0.0 {
        return camera.focus(scene);
    }
    let eye = veylvik_scene::math::point(&camera.world, [0.0; 3]);
    let ahead = veylvik_scene::math::direction(&camera.world, [0.0, 0.0, -1.0]);
    packed
        .hit(eye, ahead, f32::INFINITY)
        .map_or(1.0e8, |h| h.t.min(1.0e8))
}

/// A particle node's copies read back from the GPU: three values each, where and size,
/// turn (a quaternion) and tint.
fn read_copies(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    copies: &veylvik_render::Instances,
) -> Result<Vec<[[f32; 4]; 3]>> {
    if copies.count == 0 {
        return Ok(Vec::new());
    }
    let size = u64::from(copies.count) * 48;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle copies read back"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&copies.buffer, 0, &staging, 0, size);
    let sent = queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    let (tell, told) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tell.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(sent),
            timeout: None,
        })
        .map_err(|e| error(format!("reading particle copies: {e}")))?;
    told.recv()
        .map_err(|_| error("reading particle copies: no answer"))?
        .map_err(|e| error(format!("reading particle copies: {e}")))?;
    let copies = bytemuck::cast_slice::<u8, [[f32; 4]; 3]>(&slice.get_mapped_range()).to_vec();
    staging.unmap();
    Ok(copies)
}
/// An object's world placing copied at a particle: centred on itself as it is, turned by
/// `turn` and sized by `at.w`, at `at`, as the preview's vertex shader places a copy.
fn copied(world: &[[f32; 4]; 4], at: [f32; 4], turn: [f32; 4]) -> [[f32; 4]; 4] {
    let [x, y, z, w] = turn;
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let mut out = [[0.0; 4]; 4];
    for i in 0..3 {
        for j in 0..4 {
            out[i][j] = at[3] * (0..3).map(|k| r[i][k] * world[k][j]).sum::<f32>();
        }
        out[i][3] += at[i];
    }
    out[3] = [0.0, 0.0, 0.0, 1.0];
    out
}
