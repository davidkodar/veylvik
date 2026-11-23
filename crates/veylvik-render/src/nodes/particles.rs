//! GPU passes and uniforms for particles: prepare and fields passes pack the inputs, a splat pass runs over particle threads, and a resolve pass over pixels. The node's settings travel in a data image as rows through time, so a particle reads the settings of its own birth.
use super::{Cx, From, Passes, RenderNode, Step};
use crate::{Kernel, Params, Result, error};
use veylvik_document::{Operation, Scalar, Time, nodes::Particles};

/// Most particle threads from a box, and the threads each emitter cell has.
const MAX_BOX: f32 = 2_000_000.0;
const SLOTS: f32 = 4.0;
/// Threads in one dispatch row (1024 workgroups of 64).
const ROW: u32 = 65536;
/// Settings in a row, and rows through the last life (as in particle_system.wgsl).
const COUNT: usize = 126;
pub(crate) const ROWS: usize = 16;
/// The target's tile size for a frame (as in particle_system.wgsl).
fn tile(pixels: u64) -> u32 {
    if pixels > 2_500_000 { 32 } else { 16 }
}
const MIP: u32 = 64;

fn settings(node: &Particles, time: Time) -> Result<[f32; COUNT]> {
    let at = |scalar: &Scalar| scalar.at(time).map_err(error);
    let mut row = [0.0; COUNT];
    let values = [
        at(&node.life)?,
        at(&node.speed)?,
        at(&node.direction)?,
        at(&node.spread)?,
        at(&node.gravity)?,
        at(&node.wind)?,
        at(&node.turbulence)?,
        at(&node.turbulence_scale)?,
        at(&node.size)?,
        at(&node.glow)?,
        at(&node.variation)?,
        at(&node.fade)?,
        at(&node.start)?,
        at(&node.duration)?,
        at(&node.rate)?,
        at(&node.density)?,
        at(&node.x)?,
        at(&node.y)?,
        at(&node.width)?,
        at(&node.height)?,
        node.colour[0],
        node.colour[1],
        node.colour[2],
        node.end_colour[0],
        node.end_colour[1],
        node.end_colour[2],
        node.colour[3],
        node.end_colour[3],
        at(&node.inherit)?,
        at(&node.homing)?,
        at(&node.streak)?,
        at(&node.field_push)?,
        at(&node.field_flow)?,
        at(&node.bounce)?,
        at(&node.pull_x)?,
        at(&node.pull_y)?,
        at(&node.pull)?,
        at(&node.vortex)?,
        at(&node.spin)?,
        at(&node.grow)?,
        at(&node.colour_variation)?,
        at(&node.depth)?,
        at(&node.z)?,
        at(&node.reach)?,
        at(&node.towards)?,
        at(&node.burst)?,
        at(&node.burst_speed)?,
        at(&node.burst_spread)?,
        at(&node.burst_life)?,
        at(&node.burst_size)?,
        node.burst_colour[0],
        node.burst_colour[1],
        node.burst_colour[2],
        node.burst_colour[3],
        at(&node.trail)?,
        at(&node.trail_width)?,
        0.0,
        at(&node.link_distance)?,
        at(&node.link_opacity)?,
        at(&node.link_width)?,
    ];
    row[..values.len()].copy_from_slice(&values);
    // Curves over the life (not keyed): which are set, then their values and colours.
    let flags = u8::from(node.size_over_life.is_some())
        | u8::from(node.opacity_over_life.is_some()) << 1
        | u8::from(node.speed_over_life.is_some()) << 2
        | u8::from(node.colour_over_life.is_some()) << 3;
    row[60] = f32::from(flags);
    for (base, curve) in [
        (61, node.size_over_life),
        (69, node.opacity_over_life),
        (77, node.speed_over_life),
    ] {
        if let Some(curve) = curve {
            row[base..base + 8].copy_from_slice(&curve);
        }
    }
    row[117] = at(&node.lit)?;
    row[118] = at(&node.ambient)?;
    row[119] = at(&node.scatter)?;
    row[120] = at(&node.flock_radius)?;
    row[121] = at(&node.cohesion)?;
    row[122] = at(&node.separation)?;
    row[123] = at(&node.alignment)?;
    row[124] = at(&node.top_speed)?;
    if let Some(ramp) = node.colour_over_life {
        for (k, colour) in ramp.iter().enumerate() {
            row[85 + 4 * k..89 + 4 * k].copy_from_slice(colour);
        }
    }
    Ok(row)
}
fn scalars(node: &Particles) -> [&Scalar; 57] {
    [
        &node.life,
        &node.speed,
        &node.direction,
        &node.spread,
        &node.gravity,
        &node.wind,
        &node.turbulence,
        &node.turbulence_scale,
        &node.size,
        &node.glow,
        &node.variation,
        &node.fade,
        &node.start,
        &node.duration,
        &node.rate,
        &node.density,
        &node.x,
        &node.y,
        &node.width,
        &node.height,
        &node.inherit,
        &node.homing,
        &node.streak,
        &node.field_push,
        &node.field_flow,
        &node.bounce,
        &node.pull_x,
        &node.pull_y,
        &node.pull,
        &node.vortex,
        &node.spin,
        &node.grow,
        &node.colour_variation,
        &node.depth,
        &node.z,
        &node.reach,
        &node.towards,
        &node.rate,
        &node.density,
        &node.burst,
        &node.burst_speed,
        &node.burst_spread,
        &node.burst_life,
        &node.burst_size,
        &node.trail,
        &node.trail_width,
        &node.link_distance,
        &node.link_opacity,
        &node.link_width,
        &node.lit,
        &node.ambient,
        &node.scatter,
        &node.flock_radius,
        &node.cohesion,
        &node.separation,
        &node.alignment,
        &node.top_speed,
    ]
}
/// Most particles lines may join: each is checked against every later one.
pub(crate) const LINKED_MOST: u32 = 8192;
/// Whether this node may draw lines at all (then it runs the passes that draw them).
fn linking(node: &Particles) -> bool {
    !matches!(node.link_distance, Scalar::Constant { value } if value <= 0.0)
}
/// The settings in force over the last life before `time`, oldest first. Unkeyed
/// settings give equal rows without sampling each.
fn rows(node: &Particles, time: Time) -> Result<Vec<[f32; COUNT]>> {
    let now = settings(node, time)?;
    if scalars(node)
        .iter()
        .all(|s| matches!(s, Scalar::Constant { .. }))
    {
        return Ok(vec![now; ROWS]);
    }
    let life = f64::from(now[0]);
    let seconds = time.seconds().map_err(error)?;
    (0..ROWS)
        .map(|r| {
            if r == ROWS - 1 {
                return Ok(now);
            }
            let earlier = seconds - life + life * r as f64 / (ROWS - 1) as f64;
            settings(
                node,
                Time::new((earlier * 24000.0).round() as i64, 24000).map_err(error)?,
            )
        })
        .collect()
}
/// Threads from a box, and the emitter cell's stride, for these rows.
fn layout(rows: &[[f32; COUNT]]) -> (u32, u32) {
    let life = rows[ROWS - 1][0];
    let most = |k: usize| rows.iter().map(|r| r[k]).fold(0.0f32, f32::max);
    let threads = (most(14) * life).ceil().clamp(1.0, MAX_BOX);
    // A cell of stride x stride pixels holds SLOTS threads; make cells as large as the
    // density allows, so sparse systems cost little.
    let per_pixel = most(15) * life / 10_000.0;
    let stride = if per_pixel > 0.0 {
        (SLOTS / per_pixel).sqrt().floor().clamp(2.0, 16.0)
    } else {
        16.0
    };
    (threads as u32, stride as u32)
}
/// Most scene lights that light particles (each may have a shadow map).
const LIGHTS: usize = crate::scene_hook::SHADOW_LIGHTS;
/// The scene's lights that light particles, by index: those that shine, the first six.
pub(crate) fn shining(lights: &[veylvik_scene::Light]) -> Vec<usize> {
    (0..lights.len())
        .filter(|&i| lights[i].intensity > 0.0)
        .take(LIGHTS)
        .collect()
}
/// Whether particles may be lit by a scene, and so shadowed by its solids.
fn shadowing(node: &Particles, wiring: &veylvik_document::Node) -> bool {
    wiring.inputs.contains_key("scene")
        && !matches!(node.lit, Scalar::Constant { value } if value <= 0.0)
}
/// Where the prepare pass keeps the shadow maps: after the tile table and the sprite.
pub(crate) fn shadows_at(width: u32, height: u32) -> u32 {
    let t = tile(u64::from(width) * u64::from(height));
    width.div_ceil(t) * height.div_ceil(t) + MIP * MIP
}
/// The shadow maps' side for `lights` maps in a frame: as large as fits after their
/// header in the prepare pass's frame-sized buffer, a power of two from 64 to 1024.
pub(crate) fn shadow_side(width: u32, height: u32, lights: usize) -> u32 {
    let pixels = u64::from(width) * u64::from(height);
    let room = pixels.saturating_sub(u64::from(shadows_at(width, height)) + 128);
    let mut side = 1024u32;
    while side > 64 && (lights.max(1) as u64) * u64::from(side) * u64::from(side) > room {
        side /= 2;
    }
    side
}
/// A light as the particles read it, as the 3D renderer packs it: position and kind (0
/// point, 1 spot, 2 sun, 3 area), the way it shines and its cone's outer cosine, its
/// colour times its intensity and the cone's inner cosine.
fn light_values(light: &veylvik_scene::Light) -> [f32; 12] {
    use veylvik_document::nodes::LightKind;
    let p = light.position();
    let d = light.direction();
    let outer = (light.cone * 0.5).to_radians();
    let inner = outer * (1.0 - light.softness).max(0.0);
    let kind = match light.kind {
        LightKind::Point => 0.0,
        LightKind::Spot => 1.0,
        LightKind::Sun => 2.0,
        LightKind::Area => 3.0,
    };
    [
        p[0],
        p[1],
        p[2],
        kind,
        d[0],
        d[1],
        d[2],
        outer.cos(),
        light.colour[0] * light.intensity,
        light.colour[1] * light.intensity,
        light.colour[2] * light.intensity,
        inner.cos().max(outer.cos() + 1e-4),
    ]
}
/// Most triangles of a surface particles are born on.
const SURFACE_MOST: usize = 100_000;
/// The surface particles are born on, as data after the camera row: how many triangles
/// and objects; each object's world rows (three of four values) at the ROWS moments of
/// the last life, as settings are kept; then each triangle in its object's own space:
/// object, a corner, its two edges from that corner, and the running total of the areas
/// in the world at the frame. Empty without an object to emit from.
// The moment `r` decides both when the scene is sampled and where its rows go.
#[allow(clippy::needless_range_loop)]
fn surface(node: &Particles, cx: &Cx<'_>, life: f32) -> Result<Vec<f32>> {
    let (Some(object), Some(root)) = (node.emit_from, cx.node.inputs.get("scene")) else {
        return Ok(Vec::new());
    };
    let by_id: std::collections::BTreeMap<_, _> = cx.nodes.iter().map(|n| (n.id, n)).collect();
    let lookup = |id| by_id.get(&id).copied();
    let scene_at = |time: Time| {
        veylvik_scene::evaluate_in(
            &veylvik_scene::Graph {
                width: cx.width,
                height: cx.height,
                node: &lookup,
            },
            *root,
            time,
        )
        .map_err(error)
    };
    let now = scene_at(cx.time)?;
    let pieces: Vec<&veylvik_scene::Object> =
        now.objects.iter().filter(|o| o.node == object).collect();
    if pieces.is_empty() {
        return Ok(Vec::new());
    }
    // Each piece's place at the moments a particle may have been born.
    let seconds = cx.time.seconds().map_err(error)?;
    let mut worlds = vec![vec![[[0.0f32; 4]; 3]; ROWS]; pieces.len()];
    for r in 0..ROWS {
        let at = seconds - f64::from(life) + f64::from(life) * r as f64 / (ROWS - 1) as f64;
        let scene = if r == ROWS - 1 {
            None
        } else {
            Some(scene_at(
                Time::new((at * 24000.0).round() as i64, 24000).map_err(error)?,
            )?)
        };
        let then: Vec<&veylvik_scene::Object> = scene.as_ref().map_or_else(
            || pieces.clone(),
            |s| s.objects.iter().filter(|o| o.node == object).collect(),
        );
        for (k, piece) in pieces.iter().enumerate() {
            let world = then.get(k).map_or(piece.world, |o| o.world);
            worlds[k][r] = [world[0], world[1], world[2]];
        }
    }
    let mut triangles: Vec<[f32; 11]> = Vec::new();
    let mut total = 0.0f32;
    for (k, piece) in pieces.iter().enumerate() {
        let mesh = veylvik_scene::mesh(piece).map_err(error)?;
        let in_world = |p: [f32; 3]| veylvik_scene::math::point(&piece.world, p);
        for corners in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[corners[i] as usize].position);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let [wa, wb, wc] = [in_world(a), in_world(b), in_world(c)];
            let u = [wb[0] - wa[0], wb[1] - wa[1], wb[2] - wa[2]];
            let v = [wc[0] - wa[0], wc[1] - wa[1], wc[2] - wa[2]];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let area =
                0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            if area <= 0.0 {
                continue;
            }
            total += area;
            triangles.push([
                k as f32, a[0], a[1], a[2], e1[0], e1[1], e1[2], e2[0], e2[1], e2[2], total,
            ]);
        }
    }
    // Too many to carry: every few, their areas added up again.
    let room = (u64::from(cx.width) * u64::from(cx.height))
        .saturating_sub(u64::from(tiles_of(cx.width, cx.height)) + (COUNT * (ROWS + 1)) as u64 + 2)
        .saturating_sub((pieces.len() * ROWS * 12) as u64)
        / 11;
    let most = (room as usize).min(SURFACE_MOST);
    if triangles.len() > most {
        let step = triangles.len().div_ceil(most.max(1));
        let mut kept: Vec<[f32; 11]> = Vec::new();
        let mut before = 0.0;
        let mut running = 0.0;
        for (n, t) in triangles.iter().enumerate() {
            let own = t[10] - before;
            before = t[10];
            if n % step == 0 {
                running += own;
                let mut t = *t;
                t[10] = running;
                kept.push(t);
            }
        }
        triangles = kept;
    }
    if triangles.is_empty() {
        return Ok(Vec::new());
    }
    let mut values = vec![triangles.len() as f32, pieces.len() as f32];
    for piece in &worlds {
        for rows in piece {
            for row in rows {
                values.extend_from_slice(row);
            }
        }
    }
    for t in &triangles {
        values.extend_from_slice(t);
    }
    Ok(values)
}
pub(crate) fn tiles_of(width: u32, height: u32) -> u32 {
    let t = tile(u64::from(width) * u64::from(height));
    width.div_ceil(t) * height.div_ceil(t)
}
/// Most threads in one splat, burst particles included.
const MAX_THREADS: u64 = 4_000_000;
/// The splat's threads: (particles, burst particles each, all threads). Particles come
/// first, then each one's burst; a burst too large for the limit is made smaller.
/// Most particles in a flock (two values each of the frame's buffer hold its state).
pub(crate) const FLOCK_MOST: u32 = 65_536;
/// Whether particles steer by one another, and so are simulated.
pub(crate) fn simulating(node: &Particles) -> bool {
    !matches!(node.flock_radius, Scalar::Constant { value } if value <= 0.0)
}
fn threads(cx: &Cx<'_>, rows: &[[f32; COUNT]]) -> (u32, u32, u32) {
    let (box_threads, stride) = layout(rows);
    if let Operation::Particles(node) = &cx.node.operation
        && simulating(node)
    {
        // A flock has no bursts, and its state must fit in a frame's buffer.
        let most = FLOCK_MOST.min(cx.width * cx.height / 2);
        let parents = box_threads.min(most);
        return (parents, 0, parents);
    }
    let parents = if cx.node.inputs.contains_key("emitter") {
        cx.width.div_ceil(stride) * cx.height.div_ceil(stride) * SLOTS as u32
    } else {
        box_threads
    };
    let wanted = rows
        .iter()
        .map(|r| r[45].round())
        .fold(0.0f32, f32::max)
        .clamp(0.0, 64.0) as u64;
    let room = (MAX_THREADS / u64::from(parents.max(1))).saturating_sub(1);
    let each = wanted.min(room) as u32;
    (parents, each, parents + parents * each)
}
/// The camera filming the particles at the frame, as a last row: the world-to-camera
/// matrix's three rows, the focal length in pixels, then its lens: the blur in
/// pixels of a point infinitely far away (0 without depth of field), the distance
/// in focus (0: whatever is at the frame centre) and the largest blur. With a
/// shutter, the same camera as it was that long before follows from value 18 (its
/// matrix and focal length), with the shutter itself at 17.
fn camera(node: &Particles, cx: &Cx<'_>) -> Result<[f32; COUNT]> {
    let home = veylvik_scene::home_camera(cx.width, cx.height);
    let mut focus = 0.0;
    let mut lights = Vec::new();
    let shutter = node.shutter.at(cx.time).map_err(error)?.clamp(0.0, 0.5);
    let mut earlier = None;
    let camera = match cx.node.inputs.get("scene") {
        Some(root) => {
            let by_id: std::collections::BTreeMap<_, _> =
                cx.nodes.iter().map(|n| (n.id, n)).collect();
            let lookup = |id| by_id.get(&id).copied();
            let scene = veylvik_scene::evaluate_in(
                &veylvik_scene::Graph {
                    width: cx.width,
                    height: cx.height,
                    node: &lookup,
                },
                *root,
                cx.time,
            )
            .map_err(error)?;
            let index = node
                .camera
                .at(cx.time)
                .map_err(error)?
                .round()
                .clamp(0.0, 64.0);
            let camera = scene.camera(index as usize, cx.width, cx.height);
            focus = camera.focus(&scene);
            lights = scene.lights.clone();
            if shutter > 0.0 {
                // The camera as it was when the exposure began.
                let seconds = cx.time.seconds().map_err(error)? - f64::from(shutter);
                let before = Time::new((seconds * 24000.0).round() as i64, 24000).map_err(error)?;
                let scene = veylvik_scene::evaluate_in(
                    &veylvik_scene::Graph {
                        width: cx.width,
                        height: cx.height,
                        node: &lookup,
                    },
                    *root,
                    before,
                )
                .map_err(error)?;
                earlier = Some(scene.camera(index as usize, cx.width, cx.height));
            }
            camera
        }
        None => home,
    };
    let view = veylvik_scene::math::rigid_inverse(&camera.world);
    let mut row = [0.0; COUNT];
    for (i, line) in view.iter().take(3).enumerate() {
        row[i * 4..i * 4 + 4].copy_from_slice(line);
    }
    row[12] = veylvik_scene::home_distance(cx.width, camera.focal_length, camera.sensor_width);
    // As the 3D render's lens: one scene unit is a millimetre, and a point at
    // distance d blurs by f^2 / N / (s - f) * |1 - s / d| on the sensor.
    // In focus: the lens leaves these particles alone.
    if camera.f_stop > 0.0 && !node.in_focus {
        let f = camera.focal_length.max(1.0);
        row[13] = f * f / camera.f_stop * (cx.width as f32 / camera.sensor_width.max(1e-3)) * 0.5;
        row[14] = focus;
        row[15] = f;
        row[16] = 64.0 * cx.width as f32 / 1920.0;
    }
    row[31] = f32::from(node.points);
    // The scene's lights that light the particles: from 40, twelve values each.
    let shining = shining(&lights);
    row[36] = shining.len() as f32;
    for (i, &light) in shining.iter().enumerate() {
        row[40 + 12 * i..52 + 12 * i].copy_from_slice(&light_values(&lights[light]));
    }
    // Their shadow maps, when the scene's solids are in their way: the maps' side, where
    // the prepare pass keeps them, and whether it does.
    if shadowing(node, cx.node) {
        row[37] = shadow_side(cx.width, cx.height, shining.len()) as f32;
        row[38] = shadows_at(cx.width, cx.height) as f32;
        row[39] = 1.0;
    }
    // Row 32 on (particles, burst particles each, set off by impact) are the
    // splat's thread layout, filled in by `data` from the rows.
    if let Some(before) = earlier {
        row[17] = shutter;
        let view = veylvik_scene::math::rigid_inverse(&before.world);
        for (i, line) in view.iter().take(3).enumerate() {
            row[18 + i * 4..22 + i * 4].copy_from_slice(line);
        }
        row[30] = veylvik_scene::home_distance(cx.width, before.focal_length, before.sensor_width);
    }
    Ok(row)
}
const EMITTER: usize = 0;
const TARGET: usize = 1;
const FIELD: usize = 2;
const OBSTACLE: usize = 3;
const SPRITE: usize = 4;
const DEPTH: usize = 6;
const PREPARE: Step = Step {
    kernel: Kernel::ParticlePrepare,
    inputs: [
        From::Port(TARGET),
        From::Port(SPRITE),
        From::Port(DEPTH),
        From::None,
    ],
};
const FIELDS: Step = Step {
    kernel: Kernel::ParticleFields,
    inputs: [
        From::Port(FIELD),
        From::Port(OBSTACLE),
        From::Step(0),
        From::Data,
    ],
};
const RESOLVE: Step = Step {
    kernel: Kernel::ParticleResolve,
    inputs: [From::Step(2), From::Step(1), From::None, From::None],
};
static STEPS: [Step; 4] = [
    PREPARE,
    FIELDS,
    Step {
        kernel: Kernel::ParticleSplat,
        inputs: [
            From::Port(EMITTER),
            From::Port(TARGET),
            From::Step(1),
            From::Step(0),
        ],
    },
    RESOLVE,
];
/// The same, for depth without a target: the splat reads the depth picture in the
/// target's place instead of a copy the prepare pass makes over the whole frame.
static DEPTH_STEPS: [Step; 4] = [
    PREPARE,
    FIELDS,
    Step {
        kernel: Kernel::ParticleSplat,
        inputs: [
            From::Port(EMITTER),
            From::Port(DEPTH),
            From::Step(1),
            From::Step(0),
        ],
    },
    RESOLVE,
];
/// With lines: positions of the linked particles after the splat, then the lines, whose
/// light the resolve adds.
const LINKS: Step = Step {
    kernel: Kernel::ParticleLinks,
    inputs: [From::None, From::Step(3), From::Step(1), From::None],
};
const RESOLVE_LINKED: Step = Step {
    kernel: Kernel::ParticleResolve,
    inputs: [From::Step(2), From::Step(1), From::Step(4), From::None],
};
static LINK_STEPS: [Step; 6] = [
    PREPARE,
    FIELDS,
    Step {
        kernel: Kernel::ParticleSplat,
        inputs: [
            From::Port(EMITTER),
            From::Port(TARGET),
            From::Step(1),
            From::Step(0),
        ],
    },
    Step {
        kernel: Kernel::ParticlePositions,
        inputs: [
            From::Port(EMITTER),
            From::Port(TARGET),
            From::Step(1),
            From::Step(0),
        ],
    },
    LINKS,
    RESOLVE_LINKED,
];
static DEPTH_LINK_STEPS: [Step; 6] = [
    PREPARE,
    FIELDS,
    Step {
        kernel: Kernel::ParticleSplat,
        inputs: [
            From::Port(EMITTER),
            From::Port(DEPTH),
            From::Step(1),
            From::Step(0),
        ],
    },
    Step {
        kernel: Kernel::ParticlePositions,
        inputs: [
            From::Port(EMITTER),
            From::Port(DEPTH),
            From::Step(1),
            From::Step(0),
        ],
    },
    LINKS,
    RESOLVE_LINKED,
];
impl RenderNode for Particles {
    /// No particle is alive: none is born before Start, and with a Duration the last
    /// born is gone one Life after it ends, and its burst one Burst life after that.
    /// Only said when those are not keyed, since a particle lives by the settings of its
    /// own birth.
    fn empty(&self, _: &dyn Fn(&str) -> Result<f32>, time: veylvik_document::Time) -> Result<bool> {
        use veylvik_document::Scalar::Constant;
        let (Constant { value: start }, Constant { value: duration }, Constant { value: life }) =
            (&self.start, &self.duration, &self.life)
        else {
            return Ok(false);
        };
        let after = match (&self.burst, &self.burst_life) {
            (Constant { value: burst }, _) if *burst < 0.5 => 0.0,
            (Constant { .. }, Constant { value: burst_life }) => f64::from(*burst_life),
            _ => return Ok(false),
        };
        let now = time.seconds()?;
        Ok(now < f64::from(*start)
            || (*duration > 0.0
                && now >= f64::from(*start) + f64::from(*duration) + f64::from(*life) + after))
    }
    fn passes(&self) -> Passes {
        Passes::chain(&STEPS)
    }
    fn passes_for(&self, node: &veylvik_document::Node) -> Passes {
        let passes = plain_passes(self, node);
        if !simulating(self) {
            return passes;
        }
        // A flock: its simulated state first, read in place of the emitter.
        let Passes::Chain(steps) = passes else {
            return passes;
        };
        let mut chain = vec![Step {
            kernel: Kernel::ParticleSimulation,
            inputs: [From::None; 4],
        }];
        chain.extend(steps.iter().map(|step| Step {
            kernel: step.kernel,
            inputs: step.inputs.map(|from| match from {
                From::Port(EMITTER) => From::Step(0),
                From::Step(n) => From::Step(n + 1),
                other => other,
            }),
        }));
        Passes::Chain(std::borrow::Cow::Owned(chain))
    }
    fn has_data(&self) -> bool {
        true
    }
    /// The settings rows at `time` as an image, three values a texel; equal settings
    /// share one image, so an unkeyed system stays cached.
    fn data(&self, cx: &Cx<'_>) -> Result<Option<std::sync::Arc<crate::Image>>> {
        let time = cx.time;
        type Kept = Vec<(Vec<u32>, std::sync::Arc<crate::Image>)>;
        static BLOCKS: std::sync::Mutex<Kept> = std::sync::Mutex::new(Vec::new());
        let mut rows = rows(self, time)?;
        let (parents, each, _) = threads(cx, &rows);
        let mut last = camera(self, cx)?;
        last[32] = parents as f32;
        last[33] = each as f32;
        last[34] = f32::from(self.burst_on_impact);
        // The first particles (two values each in a frame's buffer) are joined.
        let surface = surface(self, cx, rows[ROWS - 1][0])?;
        // The solids particles bounce off, in the room the frame's buffer has left.
        let solids = match (self.collide, cx.node.inputs.get("scene")) {
            (true, Some(root)) => {
                let room = (u64::from(cx.width) * u64::from(cx.height)).saturating_sub(
                    u64::from(tiles_of(cx.width, cx.height))
                        + (COUNT * (ROWS + 1)) as u64
                        + surface.len() as u64,
                );
                let scenes = super::particle_solids::scenes(cx, *root, rows[ROWS - 1][0])?;
                super::particle_solids::fitting(&scenes, room as usize)
            }
            _ => Vec::new(),
        };
        let forces = super::particle_forces::forces(cx, rows[ROWS - 1][0])?;
        // How much data follows the camera row: the surface, the solids, the forces.
        last[112] = (surface.len() + solids.len() + forces.len()) as f32;
        last[113] = f32::from(!surface.is_empty());
        last[114] = surface.len() as f32;
        last[115] = f32::from(!solids.is_empty());
        last[116] = (surface.len() + solids.len()) as f32;
        last[117] = f32::from(!forces.is_empty());
        // A flock is simulated; it is born only in the box or on a 3D object.
        last[118] = f32::from(simulating(self));
        if simulating(self)
            && let Some(port) = ["emitter", "target", "field", "obstacle"]
                .into_iter()
                .find(|port| cx.node.inputs.contains_key(*port))
        {
            return Err(error(format!(
                "{}: a flock (Flock radius above 0) cannot use the {port} input yet; disconnect it or set Flock radius to 0",
                cx.node.label
            )));
        }
        last[35] = if linking(self) {
            parents.min(LINKED_MOST).min(cx.width * cx.height / 2) as f32
        } else {
            0.0
        };
        rows.push(last);
        let extra: Vec<f32> = surface.into_iter().chain(solids).chain(forces).collect();
        for chunk in extra.chunks(COUNT) {
            let mut row = [0.0; COUNT];
            row[..chunk.len()].copy_from_slice(chunk);
            rows.push(row);
        }
        let key: Vec<u32> = rows.iter().flatten().map(|v| v.to_bits()).collect();
        let mut kept = BLOCKS
            .lock()
            .map_err(|_| error("particle settings lock poisoned"))?;
        if let Some((_, image)) = kept.iter().find(|(k, _)| *k == key) {
            return Ok(Some(image.clone()));
        }
        let texels: Vec<[f32; 4]> = rows
            .iter()
            .flat_map(|row| row.chunks(3).map(|v| [v[0], v[1], v[2], 1.0]))
            .collect();
        let image = std::sync::Arc::new(crate::Image::data(
            (COUNT / 3) as u32,
            rows.len() as u32,
            texels,
        )?);
        if kept.len() >= 32 {
            kept.remove(0);
        }
        kept.push((key, image.clone()));
        Ok(Some(image))
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        _sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let rows = rows(self, cx.time)?;
        let (_, stride) = layout(&rows);
        let (_, _, threads) = threads(cx, &rows);
        let wired = |port: &str| cx.node.inputs.contains_key(port);
        let pixels = u64::from(cx.width) * u64::from(cx.height);
        let tile = tile(pixels);
        let tiles = u64::from(cx.width.div_ceil(tile) * cx.height.div_ceil(tile));
        if pixels < tiles + (COUNT * (ROWS + 1)) as u64 {
            return Err(error("Particles need a frame of at least 46 x 46 pixels"));
        }
        // The sprite's small copy sits after the tile table; without room, dots are drawn.
        let sprite = wired("sprite") && pixels >= tiles + u64::from(MIP * MIP);
        p.size_op[2] = (cx.time.seconds()? as f32).to_bits();
        p.size_op[3] = self.seed;
        p.values = [
            threads as f32,
            threads.min(ROW) as f32,
            stride as f32,
            SLOTS,
        ];
        p.extra = [
            // 1: the emitter as it is; 2 to 4: an atlas of its past, that many tiles a side.
            if wired("emitter") {
                crate::history::grid(self.history).max(1) as f32
            } else {
                0.0
            },
            f32::from(u8::from(wired("target"))),
            f32::from(
                u8::from(wired("field"))
                    | u8::from(wired("obstacle")) << 1
                    | u8::from(sprite) << 2
                    | u8::from(wired("depth") && wired("target")) << 3
                    | u8::from(wired("depth") && !wired("target")) << 4
                    | u8::from(shadowing(self, cx.node)) << 5,
            ),
            rows[ROWS - 1][0],
        ];
        Ok(())
    }
}
fn plain_passes(this: &Particles, node: &veylvik_document::Node) -> Passes {
    let direct_depth = node.inputs.contains_key("depth") && !node.inputs.contains_key("target");
    if !shadowing(this, node) {
        return match (direct_depth, linking(this)) {
            (true, false) => Passes::chain(&DEPTH_STEPS),
            (false, false) => Passes::chain(&STEPS),
            (true, true) => Passes::chain(&DEPTH_LINK_STEPS),
            (false, true) => Passes::chain(&LINK_STEPS),
        };
    }
    // Shadows: the scene's depth maps first, which the prepare pass takes in.
    let seen = if direct_depth { DEPTH } else { TARGET };
    let mut steps = vec![
        Step {
            kernel: Kernel::ParticleShadows,
            inputs: [From::None; 4],
        },
        Step {
            kernel: Kernel::ParticlePrepare,
            inputs: [
                From::Port(TARGET),
                From::Port(SPRITE),
                From::Port(DEPTH),
                From::Step(0),
            ],
        },
        Step {
            kernel: Kernel::ParticleFields,
            inputs: [
                From::Port(FIELD),
                From::Port(OBSTACLE),
                From::Step(1),
                From::Data,
            ],
        },
        Step {
            kernel: Kernel::ParticleSplat,
            inputs: [
                From::Port(EMITTER),
                From::Port(seen),
                From::Step(2),
                From::Step(1),
            ],
        },
    ];
    if linking(this) {
        steps.extend([
            Step {
                kernel: Kernel::ParticlePositions,
                inputs: [
                    From::Port(EMITTER),
                    From::Port(seen),
                    From::Step(2),
                    From::Step(1),
                ],
            },
            Step {
                kernel: Kernel::ParticleLinks,
                inputs: [From::None, From::Step(4), From::Step(2), From::None],
            },
            Step {
                kernel: Kernel::ParticleResolve,
                inputs: [From::Step(3), From::Step(2), From::Step(5), From::None],
            },
        ]);
    } else {
        steps.push(Step {
            kernel: Kernel::ParticleResolve,
            inputs: [From::Step(3), From::Step(2), From::None, From::None],
        });
    }
    Passes::Chain(std::borrow::Cow::Owned(steps))
}
