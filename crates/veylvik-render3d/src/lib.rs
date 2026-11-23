//! Rasterises evaluated 3D scenes for Render scene: depth-tested, 4x antialiased PBR lit by the studio environment and scene lights.
//!
//! Registered with `Renderer::set_scene_renderer`; the render crate plans the pass
//! and hands over the scene, camera, card images and target buffer.
mod resources;
mod setup;
mod shadow;
use std::collections::HashMap;
use std::sync::Mutex;
use veylvik_render::{Result, SceneJob, SceneRenderer};
use veylvik_scene::{Geometry, Light, math};
use wgpu::util::DeviceExt;

const SAMPLES: u32 = 4;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Near plane in pixels; depth is reversed with an infinite far plane.
const NEAR: f32 = 1.0;
/// Meshes kept on the GPU between frames.
const MESH_CACHE: usize = 128;
/// Strength of the studio environment, as a material's default rig.
const ENVIRONMENT: f32 = 0.65;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    surface: [f32; 4],
    params: [f32; 4],
    counts: [f32; 4],
    /// Finish: brushing, the grain's direction (radians), imperfections, visible grain.
    finish: [f32; 4],
    /// Ridges, the frame's width over 1920 and a shadow catcher's strength: the finish's patterns and the
    /// edge softening are sized for a 1920-wide frame and scale with it, so a
    /// scene looks the same at every resolution.
    relief: [f32; 4],
    /// Copies at particles (1), drawn from the instances binding.
    instancing: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    position: [f32; 4],
    direction: [f32; 4],
    colour: [f32; 4],
    shadow: [[f32; 4]; 4],
    /// Shadow map layer (-1: none), normal offset, filter width in texels, and the
    /// light node's number (for the reflections view's colours).
    shadow_info: [f32; 4],
    /// An area light's half-width and half-height axes.
    right: [f32; 4],
    up: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AmbientView {
    matrix: [[f32; 4]; 4],
    direction: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AmbientUniform {
    views: [AmbientView; 16],
    info: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniform {
    model: [[f32; 4]; 4],
    light: [[f32; 4]; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LookUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    camera: [f32; 4],
    uv_scale: [f32; 4],
}
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    used: u64,
}
pub struct Engine {
    layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    look_layout: wgpu::BindGroupLayout,
    look: wgpu::RenderPipeline,
    lit: wgpu::RenderPipeline,
    catcher: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_maps: Mutex<Option<std::sync::Arc<shadow::Maps>>>,
    sky_maps: Mutex<Option<std::sync::Arc<shadow::Maps>>>,
    /// Depth maps made for particles lit by the scene's lights.
    particle_maps: Mutex<Option<shadow::ParticleMaps>>,
    /// The models of the frame being rendered, by file path.
    models: Mutex<std::collections::BTreeMap<String, std::sync::Arc<veylvik_scene::Model>>>,
    /// Model textures on the GPU, by the loaded texture they hold (kept alive with it).
    #[allow(clippy::type_complexity)]
    textures: Mutex<HashMap<usize, (wgpu::Buffer, std::sync::Arc<veylvik_scene::Texture>)>>,
    card: wgpu::RenderPipeline,
    card_solid: wgpu::RenderPipeline,
    copy: wgpu::ComputePipeline,
    meshes: Mutex<(HashMap<String, GpuMesh>, u64)>,
    /// Panorama to studio-layout conversion.
    resample: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    irradiance: wgpu::ComputePipeline,
    /// The converted panorama, reused between frames.
    panorama: Mutex<Option<wgpu::Buffer>>,
    /// What the panorama was converted from (identity, size, a frame of the shot or a
    /// panorama): converted again only when that changes.
    converted: Mutex<Option<(u64, [u32; 2], bool)>>,
    sky: wgpu::RenderPipeline,
    /// Render targets kept between frames, by size: a 4K frame's multisampled colour
    /// and depth are hundreds of MiB, made and let go again every frame before.
    targets: Mutex<Vec<Targets>>,
}
/// What one render draws into: multisampled colour and depth, and the resolved colour.
struct Targets {
    size: [u32; 2],
    colour: wgpu::TextureView,
    resolved: wgpu::TextureView,
    depth: wgpu::TextureView,
}
/// Targets kept for sizes rendered lately; a render takes its own while it draws, so
/// renders at the same time never share one.
const KEPT_TARGETS: usize = 2;
/// Texels in the studio environment layout (nine levels and the diffuse layer).
const ENVIRONMENT_TEXELS: u64 = 701_096;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniform {
    camera: [[f32; 4]; 4],
    view: [f32; 4],
    more: [f32; 4],
}
fn error(message: impl ToString) -> veylvik_render::Error {
    veylvik_render::Error::Message(message.to_string())
}
/// Row-major to WGSL's column-major layout.
fn columns(m: &math::Mat4) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| m[r][c]))
}
/// Inverse transpose of the upper 3x3, for normals under non-uniform scale.
fn normal_matrix(m: &math::Mat4) -> math::Mat4 {
    let a = |r: usize, c: usize| m[r][c];
    let cof = |r0, r1, c0, c1| a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
    let c = [
        [cof(1, 2, 1, 2), -cof(1, 2, 0, 2), cof(1, 2, 0, 1)],
        [-cof(0, 2, 1, 2), cof(0, 2, 0, 2), -cof(0, 2, 0, 1)],
        [cof(0, 1, 1, 2), -cof(0, 1, 0, 2), cof(0, 1, 0, 1)],
    ];
    // The cofactor matrix is the inverse transpose up to a positive scale for
    // proper transforms; its sign follows the determinant.
    let det = a(0, 0) * c[0][0] + a(0, 1) * c[0][1] + a(0, 2) * c[0][2];
    let s = if det < 0.0 { -1.0 } else { 1.0 };
    let mut out = math::IDENTITY;
    for (r, row) in c.iter().enumerate() {
        for (k, v) in row.iter().enumerate() {
            out[r][k] = v * s;
        }
    }
    out
}
/// Pixels per mesh uv unit: titles' uv is already in pixels; primitives wrap 0..1.
fn uv_scale(geometry: &Geometry) -> [f32; 4] {
    use veylvik_document::nodes::ShapeKind;
    match geometry {
        Geometry::Shape { kind, size, .. } => {
            let around = std::f32::consts::PI * size[0].max(size[2]);
            match kind {
                ShapeKind::Cube | ShapeKind::Plane => {
                    let side = size[0].max(size[1]).max(size[2]);
                    [side, side, 0.0, 0.0]
                }
                _ => [around, size[1], 0.0, 0.0],
            }
        }
        Geometry::Model { size, .. } => [*size, *size, 0.0, 0.0],
        _ => [1.0, 1.0, 0.0, 0.0],
    }
}
fn light_uniform(light: &Light, shadow: Option<(u32, math::Mat4, f32)>) -> LightUniform {
    use veylvik_document::nodes::LightKind;
    let p = light.position();
    let d = light.direction();
    let outer = (light.cone * 0.5).to_radians();
    let inner = outer * (1.0 - light.softness).max(0.0);
    let panel = light.panel();
    LightUniform {
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
            light.colour[0] * light.intensity,
            light.colour[1] * light.intensity,
            light.colour[2] * light.intensity,
            inner.cos().max(outer.cos() + 1e-4),
        ],
        shadow: shadow.map_or([[0.0; 4]; 4], |(_, m, _)| columns(&m)),
        shadow_info: match shadow {
            // A panel's shadow is soft however it is set.
            Some((layer, _, offset)) => [
                layer as f32,
                offset,
                if light.kind == LightKind::Area {
                    3.0
                } else {
                    1.0
                } + light.softness * 4.0,
                0.0,
            ],
            None => [-1.0, 0.0, 0.0, 0.0],
        },
        right: [panel[0][0], panel[0][1], panel[0][2], 0.0],
        up: [panel[1][0], panel[1][1], panel[1][2], 0.0],
    }
}
/// Perspective with y down (the scene frame) into wgpu clip space, reversed depth
/// with an infinite far plane.
fn projection(camera: &veylvik_scene::Camera, width: u32, height: u32) -> math::Mat4 {
    let f = veylvik_scene::home_distance(width, camera.focal_length, camera.sensor_width);
    let (fx, fy) = (f / (width as f32 * 0.5), f / (height as f32 * 0.5));
    [
        [fx, 0.0, 0.0, 0.0],
        [0.0, -fy, 0.0, 0.0],
        [0.0, 0.0, 0.0, NEAR],
        [0.0, 0.0, -1.0, 0.0],
    ]
}
/// The lens uniform of `copy.wgsl`. Depth of field treats one scene unit as a
/// millimetre: the blur radius of a point at distance d, focused at s, is
/// f^2 / N / (s - f) * |1 - s / d| on the sensor, scaled to pixels.
fn lens(
    camera: &veylvik_scene::Camera,
    scene: &veylvik_scene::Scene,
    width: u32,
    blend: f32,
    show_depth: bool,
) -> [f32; 8] {
    let f = camera.focal_length.max(1.0);
    // A depth output is exact: no lens blur.
    let optics = if camera.f_stop > 0.0 && !show_depth {
        f * f / camera.f_stop * (width as f32 / camera.sensor_width.max(1e-3)) * 0.5
    } else {
        0.0
    };
    let eye = math::point(&camera.world, [0.0; 3]);
    let nearest = shadow::bounds(scene).map_or(1.0e9, |(centre, radius)| {
        let d = [0, 1, 2].map(|k| centre[k] - eye[k]);
        ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - radius).max(NEAR)
    });
    let largest = 64.0 * width as f32 / 1920.0;
    [
        optics,
        f,
        camera.focus(scene),
        nearest,
        largest,
        blend,
        NEAR,
        f32::from(u8::from(show_depth)),
    ]
}
impl SceneRenderer for Engine {
    fn shadows(&self, job: veylvik_render::ShadowJob<'_>) -> Result<()> {
        self.particle_shadows(job)
    }
    fn render(&self, job: SceneJob<'_>) -> Result<()> {
        let (device, width, height) = (job.device, job.width, job.height);
        // This size's targets, kept from an earlier frame, or made now.
        let kept = self.targets.lock().ok().and_then(|mut kept| {
            let at = kept.iter().position(|t| t.size == [width, height])?;
            Some(kept.remove(at))
        });
        let targets = match kept {
            Some(targets) => targets,
            None => Self::targets(device, width, height),
        };
        let result = self.render_into(job, &targets);
        if let Ok(mut kept) = self.targets.lock() {
            kept.insert(0, targets);
            kept.truncate(KEPT_TARGETS);
        }
        result
    }
}
impl Engine {
    fn targets(device: &wgpu::Device, width: u32, height: u32) -> Targets {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let colour = texture(
            "3D colour",
            COLOUR,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolved = texture(
            "3D resolved",
            COLOUR,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = texture(
            "3D depth",
            DEPTH,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        Targets {
            size: [width, height],
            colour,
            resolved,
            depth,
        }
    }
    fn render_into(&self, job: SceneJob<'_>, targets: &Targets) -> Result<()> {
        let SceneJob {
            device,
            encoder,
            scene,
            camera,
            width,
            height,
            images,
            environment,
            ltc,
            time,
            occlusion,
            blend,
            target,
            models,
            depth: show_depth,
            also,
            instances,
            ..
        } = job;
        let expanded = self.expand(scene, models)?;
        let scene = &expanded;
        let Targets {
            colour,
            resolved,
            depth,
            ..
        } = targets;
        let view_proj = math::mul(
            &projection(&camera, width, height),
            &math::rigid_inverse(&camera.world),
        );
        let eye = math::point(&camera.world, [0.0; 3]);
        // Up to four lights cast shadows, each from its own depth map; a row of strip
        // lights shares one.
        let maps = {
            let mut slot = self
                .shadow_maps
                .lock()
                .map_err(|_| error("shadow map lock poisoned"))?;
            slot.get_or_insert_with(|| std::sync::Arc::new(shadow::Maps::new(device)))
                .clone()
        };
        let reach = shadow::bounds(scene);
        let mut layer = 0u32;
        let mut shadows: Vec<Option<(u32, math::Mat4, f32)>> = vec![None; scene.lights.len()];
        let mut cast = Vec::new();
        if let Some((centre, radius)) = reach {
            use veylvik_document::nodes::LightKind;
            let casts = |light: &veylvik_scene::Light| light.shadows && light.intensity > 0.0;
            // Sun, spot and point lights take the maps first.
            for (i, light) in scene.lights.iter().enumerate() {
                if light.kind == LightKind::Area || !casts(light) || layer >= shadow::MAX_SHADOWS {
                    continue;
                }
                if let Some((matrix, offset)) = shadow::view(light, centre, radius) {
                    shadows[i] = Some((layer, matrix, offset));
                    cast.push((layer, matrix));
                    layer += 1;
                }
            }
            // An area light casts from the middle of its panel, a row of strips as one.
            for i in 0..scene.lights.len() {
                let light = &scene.lights[i];
                if light.kind != LightKind::Area || !casts(light) {
                    continue;
                }
                let row =
                    |l: &veylvik_scene::Light| l.kind == LightKind::Area && l.node == light.node;
                if let Some(first) = (0..i).find(|j| row(&scene.lights[*j])) {
                    shadows[i] = shadows[first];
                    continue;
                }
                if layer >= shadow::MAX_SHADOWS {
                    continue;
                }
                let strips: Vec<[f32; 3]> = scene
                    .lights
                    .iter()
                    .filter(|l| row(l))
                    .map(|l| l.position())
                    .collect();
                let mut middle = *light;
                middle.kind = LightKind::Spot;
                middle.cone = 170.0;
                for k in 0..3 {
                    middle.world[k][3] =
                        strips.iter().map(|p| p[k]).sum::<f32>() / strips.len() as f32;
                }
                if let Some((matrix, offset)) = shadow::view(&middle, centre, radius) {
                    shadows[i] = Some((layer, matrix, offset));
                    cast.push((layer, matrix));
                    layer += 1;
                }
            }
        }
        for (layer, matrix) in &cast {
            self.depth_pass(
                device,
                encoder,
                scene,
                matrix,
                &maps.layers[*layer as usize],
            )?;
        }
        // Sky visibility for contact shading: the scene seen from 16 directions.
        let sky_maps = {
            let mut slot = self
                .sky_maps
                .lock()
                .map_err(|_| error("sky map lock poisoned"))?;
            slot.get_or_insert_with(|| {
                std::sync::Arc::new(shadow::Maps::sized(
                    device,
                    shadow::SKY_SIZE,
                    shadow::SKY_VIEWS,
                ))
            })
            .clone()
        };
        let mut ambient: AmbientUniform = bytemuck::Zeroable::zeroed();
        if let Some((centre, radius)) = reach.filter(|_| occlusion > 0.0) {
            for (k, toward) in shadow::sky_directions().iter().enumerate() {
                let (matrix, offset) =
                    shadow::parallel(toward.map(|v| -v), centre, radius, shadow::SKY_SIZE);
                self.depth_pass(device, encoder, scene, &matrix, &sky_maps.layers[k])?;
                ambient.views[k] = AmbientView {
                    matrix: columns(&matrix),
                    direction: [toward[0], toward[1], toward[2], offset],
                };
            }
            // Blockers count within a tenth of the scene's size; the depth maps span 3r.
            ambient.info = [
                shadow::SKY_VIEWS as f32,
                occlusion,
                radius * 0.1,
                radius * 3.0,
            ];
        }
        let ambient = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3D ambient occlusion"),
            contents: bytemuck::bytes_of(&ambient),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let lights: Vec<LightUniform> = scene
            .lights
            .iter()
            .zip(&shadows)
            .map(|(light, shadow)| light_uniform(light, *shadow))
            .collect();
        // For the reflections view: each light carries its node's number.
        let numbers = scene.light_numbers();
        let mut lights = lights;
        for (uniform, light) in lights.iter_mut().zip(&scene.lights) {
            uniform.shadow_info[3] = numbers
                .iter()
                .find(|(node, _)| *node == light.node)
                .map_or(0.0, |(_, n)| *n as f32);
        }
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3D lights"),
            contents: if lights.is_empty() {
                &[0; std::mem::size_of::<LightUniform>()]
            } else {
                bytemuck::cast_slice(&lights)
            },
            usage: wgpu::BufferUsages::STORAGE,
        });
        let empty = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("no image"),
            contents: &[0; 16],
            usage: wgpu::BufferUsages::STORAGE,
        });
        // Solid objects first; cards after them, far to near, blended over.
        let mut order: Vec<&veylvik_scene::Object> = scene.objects.iter().collect();
        let distance = |o: &veylvik_scene::Object| {
            let p = math::point(&o.world, [0.0; 3]);
            (p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)
        };
        // Solids, then shadow catchers over them, then cards far to near.
        let rank = |o: &veylvik_scene::Object| {
            if matches!(o.geometry, Geometry::Card { .. }) {
                2
            } else if o.surface.catcher {
                1
            } else {
                0
            }
        };
        order.sort_by(|a, b| {
            rank(a).cmp(&rank(b)).then(if rank(a) == 2 {
                distance(b).total_cmp(&distance(a))
            } else {
                std::cmp::Ordering::Equal
            })
        });
        // The scene's environment: a converted panorama, or the studio, with its
        // strength and turn for plain surfaces, looks and the backdrop.
        let surround = scene.environment;
        // The picture itself, for an unblurred backdrop of a panorama.
        let sharp = surround
            .filter(|e| e.background && !e.frame && e.blur <= 0.0)
            .and_then(|e| e.image)
            .and_then(|source| images.iter().find(|(id, ..)| *id == source))
            .map(|(_, image, size, _)| (*image, *size));
        let panorama = match surround.and_then(|e| e.image) {
            Some(source) => match images.iter().find(|(id, ..)| *id == source) {
                Some((_, image, [across, down], identity)) => Some(self.panorama(
                    device,
                    encoder,
                    image,
                    [*across, *down],
                    surround.is_some_and(|e| e.frame),
                    *identity,
                )?),
                None => None,
            },
            None => None,
        };
        let environment = panorama.as_ref().unwrap_or(environment);
        let strength = match surround {
            Some(e) if panorama.is_some() => e.intensity,
            Some(e) => ENVIRONMENT * e.intensity,
            None => ENVIRONMENT,
        };
        let turn = surround.map_or(0.0, |e| e.rotation.to_radians());
        let look_environment = surround.map(|e| veylvik_render::EnvironmentOverride {
            strength: e.intensity,
            absolute: panorama.is_some(),
            rotation: e.rotation.to_radians(),
        });
        let view = math::rigid_inverse(&camera.world);
        // Picture and depth picture from one render: the scene is drawn as for the
        // picture, and the cards are then drawn once more, solid, into the depth alone.
        let both = also.is_some_and(|(_, other)| other != show_depth);
        let mut solid_cards: Vec<usize> = Vec::new();
        let mut draws = Vec::new();
        let mut looks = Vec::new();
        for object in order {
            let (vertices, indices, count) = self.mesh(device, object)?;
            if count == 0 {
                continue;
            }
            // An object copied at particles is drawn once for each, from their places
            // (and not at all where there are none), in its plain surface.
            let copies = match object.instanced {
                Some(id) => match instances.iter().find(|i| i.node == id) {
                    Some(found) if found.count > 0 => Some(found),
                    _ => continue,
                },
                None => None,
            };
            if let Some(look) = object
                .surface
                .look
                .as_ref()
                .filter(|_| copies.is_none())
                // The reflections view reads every surface as a plain mirror.
                .filter(|_| !object.surface.catcher && !job.reflections)
            {
                let (params, lighting) =
                    veylvik_render::material_uniforms(look, width, height, time, look_environment)?;
                let buffer = |label, bytes: &[u8], usage| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytes,
                        usage,
                    })
                };
                let p = buffer("look parameters", &params, wgpu::BufferUsages::UNIFORM);
                let studio = buffer("look lighting", &lighting, wgpu::BufferUsages::UNIFORM);
                let scratch = buffer("look scratch", &[0; 16], wgpu::BufferUsages::STORAGE);
                let material = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("3D look material"),
                    layout: &self.material_layout,
                    entries: &[
                        (0, &p),
                        (1, &empty),
                        (2, &empty),
                        (3, &scratch),
                        (4, &empty),
                        (5, ltc),
                        (6, &studio),
                        (7, environment),
                        (8, &empty),
                    ]
                    .map(|(binding, buffer)| wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    }),
                });
                let uniform = LookUniform {
                    model: columns(&object.world),
                    normal: columns(&normal_matrix(&object.world)),
                    view_proj: columns(&view_proj),
                    view: columns(&view),
                    camera: [eye[0], eye[1], eye[2], 1.0],
                    uv_scale: {
                        let [x, y, ..] = uv_scale(&object.geometry);
                        [x, y, lights.len() as f32, 0.0]
                    },
                };
                let uniform = buffer(
                    "3D look object",
                    bytemuck::bytes_of(&uniform),
                    wgpu::BufferUsages::UNIFORM,
                );
                let object_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("3D look object"),
                    layout: &self.look_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: light_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&maps.array),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&maps.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&sky_maps.array),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: ambient.as_entire_binding(),
                        },
                    ],
                });
                looks.push((material, object_bind, vertices, indices, count));
                continue;
            }
            let texture = self.part_texture(device, &object.geometry)?;
            let (pipeline, image) = match &object.geometry {
                Geometry::Card { image, .. } => (
                    if show_depth && !both {
                        &self.card_solid
                    } else {
                        &self.card
                    },
                    images
                        .iter()
                        .find(|(id, ..)| id == image)
                        .map(|(_, b, s, _)| (*b, *s)),
                ),
                _ if object.surface.catcher => (&self.catcher, None),
                _ => (&self.lit, texture.as_ref().map(|(b, w, h)| (b, [*w, *h]))),
            };
            // A shape's Texture input: that node's picture, at its own size.
            let wrapped = object
                .texture
                .filter(|_| !object.surface.catcher && texture.is_none())
                .and_then(|source| images.iter().find(|(id, ..)| *id == source))
                .map(|(_, buffer, size, _)| (*buffer, *size));
            let image = image.or(wrapped);
            let picture = image.map_or([width as f32, height as f32], |(_, [w, h])| {
                [w as f32, h as f32]
            });
            let image = image.map(|(buffer, _)| buffer);
            let uniform = ObjectUniform {
                model: columns(&object.world),
                normal: columns(&normal_matrix(&object.world)),
                view_proj: columns(&view_proj),
                camera: [eye[0], eye[1], eye[2], 1.0],
                surface: [
                    object.surface.colour[0],
                    object.surface.colour[1],
                    object.surface.colour[2],
                    if matches!(object.geometry, Geometry::Card { .. }) {
                        object.surface.opacity
                    } else {
                        object.surface.roughness
                    },
                ],
                params: [
                    object.surface.metallic,
                    f32::from(object.surface.unlit),
                    picture[0],
                    picture[1],
                ],
                counts: [
                    lights.len() as f32,
                    strength,
                    turn,
                    // 1: a model part's own texture; 2: a shape's picture, whose alpha cuts.
                    if wrapped.is_some() {
                        2.0
                    } else {
                        f32::from(texture.is_some())
                    },
                ],
                finish: [
                    object.surface.finish[0],
                    object.surface.finish[1].to_radians(),
                    object.surface.finish[2],
                    object.surface.finish[3],
                ],
                relief: [
                    object.surface.finish[4],
                    job.width as f32 / 1920.0,
                    object.surface.shadow,
                    f32::from(u8::from(job.reflections)),
                ],
                instancing: [f32::from(u8::from(copies.is_some())), 0.0, 0.0, 0.0],
            };
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D object"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3D object"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: light_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: environment.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: image.unwrap_or(&empty).as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&maps.array),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(&maps.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&sky_maps.array),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: ambient.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: copies.map_or(&empty, |c| &c.buffer).as_entire_binding(),
                    },
                ],
            });
            if both && matches!(object.geometry, Geometry::Card { .. }) {
                solid_cards.push(draws.len());
            }
            let times = copies.map_or(1, |c| c.count);
            draws.push((pipeline, bind, vertices, indices, count, times));
        }
        let sky = surround.filter(|e| e.background).map(|e| {
            let uniform = SkyUniform {
                camera: columns(&camera.world),
                view: [
                    width as f32,
                    height as f32,
                    veylvik_scene::home_distance(width, camera.focal_length, camera.sensor_width),
                    strength,
                ],
                more: [
                    turn,
                    e.blur * 8.0,
                    sharp.map_or(0.0, |(_, size)| size[0] as f32),
                    sharp.map_or(0.0, |(_, size)| size[1] as f32),
                ],
            };
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D sky"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3D sky"),
                layout: &self.sky.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: environment.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: sharp
                            .map_or(environment, |(image, _)| image)
                            .as_entire_binding(),
                    },
                ],
            })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour,
                    depth_slice: None,
                    resolve_target: Some(resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(sky) = &sky {
                pass.set_pipeline(&self.sky);
                pass.set_bind_group(0, sky, &[]);
                pass.draw(0..3, 0..1);
            }
            pass.set_pipeline(&self.look);
            for (material, object, vertices, indices, count) in &looks {
                pass.set_bind_group(0, material, &[]);
                pass.set_bind_group(1, object, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
            for (pipeline, bind, vertices, indices, count, times) in &draws {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..*times);
            }
        }
        // Read the render out into a frame: the picture, or each pixel's distance.
        let read_out = |encoder: &mut wgpu::CommandEncoder, into: &wgpu::Buffer, as_depth: bool| {
            let lens = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D lens"),
                contents: bytemuck::cast_slice(&lens(&camera, scene, width, blend, as_depth)),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let copy = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3D resolve copy"),
                layout: &self.copy.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(resolved),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: into.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: lens.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.copy);
            pass.set_bind_group(0, &copy, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        };
        let Some((second, second_depth)) = also else {
            read_out(encoder, target, show_depth);
            return Ok(());
        };
        if !both {
            read_out(encoder, target, show_depth);
            read_out(encoder, second, second_depth);
            return Ok(());
        }
        // The picture first, while the depth holds the solid objects only, as a picture's
        // lens needs it; then the cards' solid parts go into the depth, and that is read.
        let (picture, distance) = if show_depth {
            (second, target)
        } else {
            (target, second)
        };
        read_out(encoder, picture, false);
        if !solid_cards.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D cards into the depth"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.card_solid);
            for at in &solid_cards {
                let (_, bind, vertices, indices, count, times) = &draws[*at];
                pass.set_bind_group(0, bind, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..*times);
            }
        }
        read_out(encoder, distance, true);
        Ok(())
    }
}
