//! The path tracer on the GPU, checked against what the light must do: an even sky lights a
//! convex shape to exactly its albedo, a sphere covers the pixels its projection says, and
//! a point light is cut off behind a blocker.
use std::sync::Arc;
use veylvik_mesh::{Primitive, primitive};
use veylvik_trace::{
    Environment, Gather, Job, Light, MeshTree, Placed, Settings, Surface, Tracer, View, pack,
};
use wgpu::util::DeviceExt;

/// An even sky: the first texel of the environment, whatever the direction.
const SKY: &str =
    "fn environment_layer(d: vec3<f32>, layer: u32) -> vec3<f32> { return environment_map[0].rgb; }
fn environment_specular(d: vec3<f32>, r: f32) -> vec3<f32> { return environment_map[0].rgb; }";
/// A dim sky with a small, very bright sun up and to the right (y is down).
const SUN_SKY: &str = "fn sun_sky(d: vec3<f32>) -> vec3<f32> {
    let sun = normalize(vec3<f32>(0.6, -0.6, 0.53));
    return vec3<f32>(select(0.02, 60.0, dot(normalize(d), sun) > 0.995)) * environment_map[0].rgb;
}
fn environment_layer(d: vec3<f32>, layer: u32) -> vec3<f32> { return sun_sky(d); }
fn environment_specular(d: vec3<f32>, r: f32) -> vec3<f32> { return sun_sky(d); }";
const W: u32 = 256;
const H: u32 = 256;
type Mat4 = [[f32; 4]; 4];
fn at(p: [f32; 3]) -> Mat4 {
    [
        [1.0, 0.0, 0.0, p[0]],
        [0.0, 1.0, 0.0, p[1]],
        [0.0, 0.0, 1.0, p[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
fn gpu() -> (wgpu::Device, wgpu::Queue) {
    let (_, _, device, queue) =
        pollster::block_on(veylvik_render::create_gpu()).expect("a Vulkan device");
    (device, queue)
}
struct Shot {
    placed: Vec<Placed>,
    lights: Vec<Light>,
    sky: f32,
    samples: u32,
    sample_sky: bool,
    seed: u32,
    /// Denoised, gathered with this many paths before this render's.
    denoise: Option<Gather>,
}
/// The picture the camera at (0, 0, 1000), looking down -z with a 500 px focal length, sees.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tracer: &Tracer,
    shot: &Shot,
) -> Vec<[f32; 4]> {
    let size = u64::from(W * H) * 16;
    let target = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    trace_into(device, queue, tracer, shot, &target);
    read_back(device, queue, &target)
}
/// Traces `shot` into `target` (premultiplied RGBA f32, W x H).
fn trace_into(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tracer: &Tracer,
    shot: &Shot,
    target: &wgpu::Buffer,
) {
    let scene = pack(&shot.placed);
    let map = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[[shot.sky, shot.sky, shot.sky, 1.0f32]]),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    tracer
        .render(Job {
            device,
            queue,
            encoder: &mut encoder,
            scene: &scene,
            textures: &[],
            looks: &[],
            lights: &shot.lights,
            view: View {
                camera: at([0.0, 0.0, 1000.0]),
                focal: 500.0,
                lens_radius: 0.0,
                focus: 1000.0,
            },
            environment: Environment {
                map: &map,
                strength: 1.0,
                turn: 0.0,
                backdrop: false,
            },
            width: W,
            height: H,
            settings: Settings {
                samples: shot.samples,
                bounces: 6,
                clamp: 0.0,
                sample_sky: shot.sample_sky,
                seed: shot.seed,
                denoise: shot.denoise,
            },
            target,
            blend: 1.0,
        })
        .unwrap();
    queue.submit([encoder.finish()]);
}
fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Buffer) -> Vec<[f32; 4]> {
    let size = target.size();
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(target, 0, &read, 0, size);
    queue.submit([encoder.finish()]);
    read.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = read.slice(..).get_mapped_range();
    bytemuck::cast_slice::<u8, [f32; 4]>(&data).to_vec()
}
fn sphere(radius: f32) -> Arc<MeshTree> {
    Arc::new(MeshTree::new(
        &primitive(Primitive::Sphere, [2.0 * radius; 3], 64).unwrap(),
    ))
}
fn matte(grey: f32) -> Surface {
    Surface {
        colour: [grey; 3],
        roughness: 1.0,
        metallic: 0.0,
        ..Default::default()
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn an_even_sky_lights_a_convex_shape_to_its_albedo() {
    let (device, queue) = gpu();
    let tracer = Tracer::new(&device, veylvik_render::SURFACE_WGSL, SKY);
    let shot = Shot {
        placed: vec![Placed {
            tree: sphere(200.0),
            world: at([0.0; 3]),
            surface: matte(0.5),
        }],
        lights: vec![],
        sky: 1.0,
        samples: 256,
        sample_sky: true,
        seed: 7,
        denoise: None,
    };
    let px = render(&device, &queue, &tracer, &shot);
    // Inside the sphere's disc, away from its rim.
    let (mut sum, mut n) = (0.0f64, 0);
    for y in 0..H {
        for x in 0..W {
            let (dx, dy) = (x as f32 + 0.5 - 128.0, y as f32 + 0.5 - 128.0);
            if dx * dx + dy * dy < 80.0 * 80.0 {
                sum += f64::from(px[(y * W + x) as usize][1]);
                n += 1;
            }
        }
    }
    let mean = sum / f64::from(n);
    eprintln!("furnace: albedo 0.5 under a sky of 1 shows {mean:.4}");
    // Half its light back, whatever way each point faces (a little more from the gloss
    // of a rough dielectric).
    assert!(
        (0.47..0.56).contains(&mean),
        "albedo 0.5 under a sky of 1 shows {mean}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_sphere_covers_the_pixels_its_projection_says() {
    let (device, queue) = gpu();
    let tracer = Tracer::new(&device, veylvik_render::SURFACE_WGSL, SKY);
    let shot = Shot {
        placed: vec![Placed {
            tree: sphere(200.0),
            world: at([0.0; 3]),
            surface: matte(0.5),
        }],
        lights: vec![],
        sky: 1.0,
        samples: 16,
        sample_sky: true,
        seed: 7,
        denoise: None,
    };
    let px = render(&device, &queue, &tracer, &shot);
    // Its outline from 1000 away with a 500 px lens: 500 * r / sqrt(d^2 - r^2) px.
    let rim = 500.0 * 200.0 / (1000.0f32 * 1000.0 - 200.0 * 200.0).sqrt();
    let alpha = |x: f32, y: f32| px[((128.0 + y) as u32 * W + (128.0 + x) as u32) as usize][3];
    assert_eq!(alpha(0.0, 0.0), 1.0);
    assert_eq!(alpha(rim - 3.0, 0.0), 1.0, "inside the rim at {rim}");
    assert_eq!(alpha(rim + 3.0, 0.0), 0.0, "outside the rim");
    assert_eq!(alpha(0.0, -(rim + 3.0)), 0.0);
    assert_eq!(alpha(0.0, -(rim - 3.0)), 1.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn a_point_light_is_cut_off_behind_a_blocker() {
    let (device, queue) = gpu();
    let tracer = Tracer::new(&device, veylvik_render::SURFACE_WGSL, SKY);
    let floor = Arc::new(MeshTree::new(
        &primitive(Primitive::Plane, [1000.0, 1000.0, 1.0], 1).unwrap(),
    ));
    let block = Arc::new(MeshTree::new(
        &primitive(Primitive::Cube, [60.0; 3], 1).unwrap(),
    ));
    let light = Light {
        position: [400.0, 0.0, 400.0, 0.0],
        colour: [0.1, 0.1, 0.1, 0.0],
        // It casts shadows (2) and shows fully in reflections (1).
        up: [0.0, 0.0, 0.0, 3.0],
        ..Default::default()
    };
    let shot = Shot {
        placed: vec![
            Placed {
                tree: floor,
                world: at([0.0; 3]),
                surface: matte(0.5),
            },
            Placed {
                tree: block,
                world: at([200.0, 0.0, 200.0]),
                surface: matte(0.5),
            },
        ],
        lights: vec![light],
        sky: 0.0,
        samples: 16,
        sample_sky: true,
        seed: 7,
        denoise: None,
    };
    let px = render(&device, &queue, &tracer, &shot);
    // The floor's middle is behind the block from the light; a point 150 down it is not.
    let shaded = px[(128 * W + 128) as usize][1];
    let open = px[((128 + 75) * W + 128) as usize][1];
    assert!(open > 0.05, "the open floor is lit: {open}");
    assert!(
        shaded < 0.1 * open,
        "behind the block: {shaded} against {open}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn drawing_light_from_a_bright_sun_keeps_the_picture_and_cuts_the_noise() {
    let (device, queue) = gpu();
    let tracer = Tracer::new(&device, veylvik_render::SURFACE_WGSL, SUN_SKY);
    let shot = |samples, sample_sky, seed| Shot {
        placed: vec![Placed {
            tree: sphere(200.0),
            world: at([0.0; 3]),
            surface: matte(0.6),
        }],
        lights: vec![],
        sky: 1.0,
        samples,
        sample_sky,
        seed,
        denoise: None,
    };
    // The sphere's sunlit side, away from the rim.
    let lit: Vec<usize> = (0..W * H)
        .filter(|&i| {
            let (dx, dy) = ((i % W) as f32 + 0.5 - 128.0, (i / W) as f32 + 0.5 - 128.0);
            dx * dx + dy * dy < 80.0 * 80.0 && dx > 10.0 && dy < -10.0
        })
        .map(|i| i as usize)
        .collect();
    let mean =
        |p: &[[f32; 4]]| lit.iter().map(|&i| f64::from(p[i][1])).sum::<f64>() / lit.len() as f64;
    let noise = |a: &[[f32; 4]], b: &[[f32; 4]]| {
        (lit.iter()
            .map(|&i| f64::from(a[i][1] - b[i][1]).powi(2))
            .sum::<f64>()
            / lit.len() as f64)
            .sqrt()
    };
    let reference = mean(&render(&device, &queue, &tracer, &shot(2048, false, 1)));
    let (a, b) = (
        render(&device, &queue, &tracer, &shot(64, true, 2)),
        render(&device, &queue, &tracer, &shot(64, true, 3)),
    );
    let (c, d) = (
        render(&device, &queue, &tracer, &shot(64, false, 2)),
        render(&device, &queue, &tracer, &shot(64, false, 3)),
    );
    let (drawn, bounced) = (noise(&a, &b), noise(&c, &d));
    eprintln!(
        "sun-lit side: reference {reference:.4}, drawn {:.4} (noise {drawn:.4}), bounced only noise {bounced:.4}",
        mean(&a)
    );
    assert!(reference > 0.05, "the sun lights the sphere: {reference}");
    assert!(
        (mean(&a) / reference - 1.0).abs() < 0.03,
        "the same picture on average: {} against {reference}",
        mean(&a)
    );
    assert!(
        drawn < 0.25 * bounced,
        "far less noise: {drawn} against {bounced}"
    );
}

/// A lit floor with a block on it, under one point light: shadows, an edge and a rim.
fn lit_block(samples: u32, seed: u32, denoise: Option<Gather>) -> Shot {
    let floor = Arc::new(MeshTree::new(
        &primitive(Primitive::Plane, [700.0, 700.0, 1.0], 1).unwrap(),
    ));
    let block = Arc::new(MeshTree::new(
        &primitive(Primitive::Cube, [160.0; 3], 1).unwrap(),
    ));
    Shot {
        placed: vec![
            Placed {
                tree: floor,
                world: at([0.0; 3]),
                surface: matte(0.5),
            },
            Placed {
                tree: block,
                world: at([60.0, 40.0, 80.0]),
                surface: matte(0.8),
            },
        ],
        lights: vec![Light {
            position: [-300.0, -250.0, 500.0, 0.0],
            colour: [0.6, 0.6, 0.6, 0.0],
            up: [0.0, 0.0, 0.0, 3.0],
            ..Default::default()
        }],
        sky: 0.3,
        samples,
        sample_sky: true,
        seed,
        denoise,
    }
}
/// Root mean square difference of two pictures' green, over the pixels where `a` is opaque.
fn error(a: &[[f32; 4]], b: &[[f32; 4]]) -> f64 {
    let (mut sum, mut n) = (0.0f64, 0);
    for (p, q) in a.iter().zip(b) {
        if p[3] > 0.5 {
            sum += f64::from(p[1] - q[1]).powi(2);
            n += 1;
        }
    }
    (sum / f64::from(n.max(1))).sqrt()
}
const ONCE: Option<Gather> = Some(Gather {
    gathered: 0,
    last: true,
});
#[test]
#[ignore = "requires Vulkan"]
fn denoising_brings_a_few_paths_close_to_many_and_keeps_edges_sharp() {
    let (device, queue) = gpu();
    let tracer = Tracer::new(&device, veylvik_render::SURFACE_WGSL, SKY);
    let truth = render(&device, &queue, &tracer, &lit_block(4096, 3, None));
    let noisy = render(&device, &queue, &tracer, &lit_block(8, 7, None));
    let clean = render(&device, &queue, &tracer, &lit_block(8, 7, ONCE));
    let (before, after) = (error(&noisy, &truth), error(&clean, &truth));
    eprintln!("8 paths a pixel: {before:.4} from the truth raw, {after:.4} denoised");
    assert!(
        after < before * 0.4,
        "denoised {after} against raw {before}"
    );
    // The picture stays where it was: coverage is the paths' own, and the block's outline
    // does not smear onto the floor (along a row through the block's side).
    for (p, q) in clean.iter().zip(&noisy) {
        assert_eq!(p[3], q[3]);
    }
    let edge = |px: &[[f32; 4]]| -> f32 {
        let row = 128 * W as usize;
        (100..156)
            .map(|x| (px[row + x + 1][1] - px[row + x][1]).abs())
            .fold(0.0, f32::max)
    };
    let (sharp, kept) = (edge(&truth), edge(&clean));
    eprintln!("steepest step along the row: {sharp:.4} in the truth, {kept:.4} denoised");
    assert!(kept > 0.6 * sharp, "edge {kept} against {sharp}");
}
#[test]
#[ignore = "requires Vulkan"]
fn a_picture_gathered_over_moments_is_the_picture_gathered_at_once() {
    let (device, queue) = gpu();
    let tracer = Tracer::new(&device, veylvik_render::SURFACE_WGSL, SKY);
    let whole = render(&device, &queue, &tracer, &lit_block(32, 5, ONCE));
    let target = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(W * H) * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    for (gathered, last) in [(0, false), (16, true)] {
        let shot = lit_block(16, 5, Some(Gather { gathered, last }));
        trace_into(&device, &queue, &tracer, &shot, &target);
    }
    let halves = read_back(&device, &queue, &target);
    // The same paths, gathered in two: only the guides' rounding between moments differs.
    let most = whole
        .iter()
        .zip(&halves)
        .flat_map(|(p, q)| (0..4).map(move |c| (p[c] - q[c]).abs() / p[c].abs().max(0.05)))
        .fold(0.0, f32::max);
    assert!(
        most < 5e-3,
        "gathered in two differs by up to {most} of the value"
    );
}
