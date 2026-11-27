//! Path traced pictures (Render scene's Quality: path traced): the scene's meshes as trees kept between frames under the same keys as the drawn meshes, surfaces and lights handed to the tracer with the meanings the interactive renderer gives them, filmed through the same camera and lens.
use crate::{Engine, TREE_CACHE, error, look_scale};
use std::sync::Arc;
use veylvik_render::{Result, SceneJob, Trace};
use veylvik_scene::Geometry;
use veylvik_trace::{
    CARD, CATCHER, CUT, Environment, GLASS, Job, Look, MeshTree, Placed, STRAIGHT, Settings,
    Surface, Texture, Tracer, UNLIT, View,
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
        for object in &scene.objects {
            // Copies at particles and splats are not path traced yet.
            if object.instanced.is_some() || matches!(object.geometry, Geometry::Splats { .. }) {
                continue;
            }
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
            // A library look, as its material's own blocks for the shared surface code.
            let look = match &s.look {
                Some(material) if flags & (CARD | CATCHER) == 0 => {
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
            placed.push(Placed {
                tree: self.tree(object)?,
                world: object.world,
                surface: Surface {
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
                },
            });
        }
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
            })
            .map_err(error)
    }
    /// An object's mesh as a tree, kept between frames under its mesh's key.
    fn tree(&self, object: &veylvik_scene::Object) -> Result<Arc<MeshTree>> {
        let key = format!("{:?}{:?}", object.geometry, object.flutter);
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
        let tree = Arc::new(MeshTree::new(&self.cpu_mesh(object)?));
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
