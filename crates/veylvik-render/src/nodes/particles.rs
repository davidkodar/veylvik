//! GPU passes and uniforms for particles: prepare and fields passes pack the inputs, a splat pass runs over particle threads, and a resolve pass over pixels. The node's settings travel in a data image as rows through time, so a particle reads the settings of its own birth.
use super::{Cx, From, Passes, RenderNode, Step};
use crate::{Kernel, Params, Result, error};
use veylvik_document::{Scalar, Time, nodes::Particles};

/// Most particle threads from a box, and the threads each emitter cell has.
const MAX_BOX: f32 = 2_000_000.0;
const SLOTS: f32 = 4.0;
/// Threads in one dispatch row (1024 workgroups of 64).
const ROW: u32 = 65536;
/// Settings in a row, and rows through the last life (as in particle_system.wgsl).
const COUNT: usize = 45;
const ROWS: usize = 16;
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
    ];
    row[..values.len()].copy_from_slice(&values);
    Ok(row)
}
fn scalars(node: &Particles) -> [&Scalar; 39] {
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
    ]
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
/// The camera filming the particles at the frame, as a last row: the world-to-camera
/// matrix's three rows, the focal length in pixels, then its lens: the blur in
/// pixels of a point infinitely far away (0 without depth of field), the distance
/// in focus (0: whatever is at the frame centre) and the largest blur. With a
/// shutter, the same camera as it was that long before follows from value 18 (its
/// matrix and focal length), with the shutter itself at 17.
fn camera(node: &Particles, cx: &Cx<'_>) -> Result<[f32; COUNT]> {
    let home = veylvik_scene::home_camera(cx.width, cx.height);
    let mut focus = 0.0;
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
static STEPS: [Step; 4] = [
    Step {
        kernel: Kernel::ParticlePrepare,
        inputs: [
            From::Port(TARGET),
            From::Port(SPRITE),
            From::Port(DEPTH),
            From::None,
        ],
    },
    Step {
        kernel: Kernel::ParticleFields,
        inputs: [
            From::Port(FIELD),
            From::Port(OBSTACLE),
            From::Step(0),
            From::Data,
        ],
    },
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
        kernel: Kernel::ParticleResolve,
        inputs: [From::Step(2), From::Step(1), From::None, From::None],
    },
];
impl RenderNode for Particles {
    fn passes(&self) -> Passes {
        Passes::chain(&STEPS)
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
        rows.push(camera(self, cx)?);
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
        let image = std::sync::Arc::new(crate::Image::new(
            (COUNT / 3) as u32,
            ROWS as u32 + 1,
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
        let (box_threads, stride) = layout(&rows);
        let wired = |port: &str| cx.node.inputs.contains_key(port);
        let pixels = u64::from(cx.width) * u64::from(cx.height);
        let tile = tile(pixels);
        let tiles = u64::from(cx.width.div_ceil(tile) * cx.height.div_ceil(tile));
        if pixels < tiles + (COUNT * (ROWS + 1)) as u64 {
            return Err(error("Particles need a frame of at least 32 x 32 pixels"));
        }
        let threads = if wired("emitter") {
            cx.width.div_ceil(stride) * cx.height.div_ceil(stride) * SLOTS as u32
        } else {
            box_threads
        };
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
                    | u8::from(wired("depth")) << 3,
            ),
            rows[ROWS - 1][0],
        ];
        Ok(())
    }
}
