// Particles, in a module of their own on the kernels' bind group layout. Four passes:
// prepare (tile sums of the target's alpha, and a small copy of the sprite), fields
// (force field, obstacle, the target's cumulative tile table and the settings, packed
// into one image), splat (one thread per particle adds fixed-point light into the
// output with atomics) and resolve (sums to an image). A particle is a pure function
// of (thread, generation, time).
// p.size_op: width, height, time (f32 bits), seed. p.values: threads, threads per
// dispatch row, emitter cell stride, threads per cell. p.extra: emitter connected,
// target connected, flags (1 field, 2 obstacle, 4 sprite), nominal life in seconds.
// Settings: ROWS rows of COUNT values, row r holding the settings in force at time
// now - life + life * r / (ROWS - 1), so a particle reads those of its own birth. One more
// row holds the camera: the world-to-camera matrix's three rows and the focal length in
// pixels. Particles live in the scenes' space: the frame at depth 0, z towards the viewer.
struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> first: array<vec4<u32>>;
@group(0) @binding(2) var<storage, read> second: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read> third: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read> fourth: array<vec4<f32>>;
const SCALE: f32 = 4096.0;
// The target is divided into tiles of 16 pixels, or 32 in frames too large for that.
fn tile() -> u32 {
    if p.size_op.x * p.size_op.y > 2500000u { return 32u; }
    return 16u;
}
const MIP: u32 = 64u;
const COUNT: u32 = 45u;
const ROWS: u32 = 16u;
// Setting numbers.
const SPEED = 1u; const DIRECTION = 2u; const SPREAD = 3u; const GRAVITY = 4u; const WIND = 5u;
const TURBULENCE = 6u; const TURBULENCE_SCALE = 7u; const SIZE = 8u; const GLOW = 9u;
const VARIATION = 10u; const FADE = 11u; const START = 12u; const DURATION = 13u; const RATE = 14u;
const DENSITY = 15u; const BOX_X = 16u; const BOX_Y = 17u; const BOX_W = 18u; const BOX_H = 19u;
const COLOUR = 20u; const END_COLOUR = 23u; const COLOUR_A = 26u; const END_A = 27u;
const INHERIT = 28u; const HOMING = 29u; const STREAK = 30u; const PUSH = 31u; const FLOW = 32u;
const BOUNCE = 33u; const PULL_X = 34u; const PULL_Y = 35u; const PULL = 36u; const VORTEX = 37u;
const SPIN = 38u; const GROW = 39u; const HUES = 40u; const DEPTH = 41u;
fn tiles() -> u32 {
    let t = tile();
    return ((p.size_op.x + t - 1u) / t) * ((p.size_op.y + t - 1u) / t);
}
fn store(i: u32, value: vec4<f32>) {
    atomicStore(&result[i * 4u], bitcast<u32>(value.x)); atomicStore(&result[i * 4u + 1u], bitcast<u32>(value.y));
    atomicStore(&result[i * 4u + 2u], bitcast<u32>(value.z)); atomicStore(&result[i * 4u + 3u], bitcast<u32>(value.w));
}
fn hash(value: u32) -> u32 {
    var v = value; v = (v ^ (v >> 16u)) * 0x7feb352du; v = (v ^ (v >> 15u)) * 0x846ca68bu; return v ^ (v >> 16u);
}
// A random number in 0..1 for a particle (thread, generation) and a purpose.
fn random(thread: u32, generation: u32, purpose: u32) -> f32 {
    return f32(hash(hash(hash(thread + p.size_op.w * 0x9e3779b9u) ^ generation) + purpose * 0x85ebca6bu) >> 8u) / 16777216.0;
}
fn lattice(c: vec3<i32>) -> f32 {
    return f32(hash(hash(hash(u32(c.x)) ^ u32(c.y)) ^ (u32(c.z) + p.size_op.w)) >> 8u) / 16777216.0;
}
fn value_noise(q: vec3<f32>) -> f32 {
    let c = vec3<i32>(floor(q)); let f = fract(q); let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(lattice(c), lattice(c + vec3<i32>(1, 0, 0)), u.x);
    let x10 = mix(lattice(c + vec3<i32>(0, 1, 0)), lattice(c + vec3<i32>(1, 1, 0)), u.x);
    let x01 = mix(lattice(c + vec3<i32>(0, 0, 1)), lattice(c + vec3<i32>(1, 0, 1)), u.x);
    let x11 = mix(lattice(c + vec3<i32>(0, 1, 1)), lattice(c + vec3<i32>(1, 1, 1)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}
fn potential(q: vec3<f32>) -> f32 {
    return value_noise(q) + 0.5 * value_noise(q * 2.03 + vec3<f32>(17.0, 31.0, 5.0));
}

// ---- prepare: source = target, second = sprite ----
@compute @workgroup_size(8, 8)
fn particle_prepare(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    let count = tiles();
    var out = vec4<f32>(0.0);
    if i < count {
        if p.extra.y > 0.5 {
            // The target's alpha summed over tile i.
            let t = tile();
            let across = (p.size_op.x + t - 1u) / t;
            let x0 = (i % across) * t; let y0 = (i / across) * t;
            var sum = 0.0;
            for (var y = y0; y < min(y0 + t, p.size_op.y); y++) {
                for (var x = x0; x < min(x0 + t, p.size_op.x); x++) {
                    sum += clamp(bitcast<f32>(first[y * p.size_op.x + x].w), 0.0, 1.0);
                }
            }
            out.x = sum;
        }
    } else if i < count + MIP * MIP && (u32(p.extra.z) & 4u) != 0u {
        // The sprite's frame averaged down to MIP x MIP.
        let cell = i - count;
        let x0 = (cell % MIP) * p.size_op.x / MIP; let x1 = max(((cell % MIP) + 1u) * p.size_op.x / MIP, x0 + 1u);
        let y0 = (cell / MIP) * p.size_op.y / MIP; let y1 = max(((cell / MIP) + 1u) * p.size_op.y / MIP, y0 + 1u);
        var sum = vec4<f32>(0.0);
        for (var y = y0; y < min(y1, p.size_op.y); y++) {
            for (var x = x0; x < min(x1, p.size_op.x); x++) { sum += second[y * p.size_op.x + x]; }
        }
        out = sum / f32((x1 - x0) * (y1 - y0));
    }
    store(i, out);
}
// ---- fields: source = force field, second = obstacle, third = prepare, fourth = settings ----
fn field_value(x: i32, y: i32) -> f32 {
    let c = bitcast<vec4<f32>>(first[u32(clamp(y, 0, i32(p.size_op.y) - 1)) * p.size_op.x + u32(clamp(x, 0, i32(p.size_op.x) - 1))]);
    return dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}
@compute @workgroup_size(8, 8)
fn particle_fields(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    let flags = u32(p.extra.z);
    var out = vec4<f32>(0.0);
    if (flags & 1u) != 0u {
        // The slope of the field's brightness, over a few pixels so it is smooth.
        let x = i32(gid.x); let y = i32(gid.y);
        out.x = (field_value(x + 3, y) - field_value(x - 3, y)) / 6.0;
        out.y = (field_value(x, y + 3) - field_value(x, y - 3)) / 6.0;
    }
    if (flags & 2u) != 0u { out.z = clamp(second[i].a, 0.0, 1.0); }
    let count = tiles();
    if i < count {
        // Cumulative alpha up to and including tile i.
        var sum = 0.0;
        for (var k = 0u; k <= i; k++) { sum += third[k].x; }
        out.w = sum;
    } else if i < count + COUNT * (ROWS + 1u) {
        let k = i - count;
        out.w = fourth[k / 3u][k % 3u];
    }
    store(i, out);
}
// ---- splat: source = emitter, second = target, third = fields, fourth = prepare ----
// Setting `k` as it was at time `seconds` (held at the ends of the window).
fn setting(k: u32, seconds: f32) -> f32 {
    let life = p.extra.w;
    let row = clamp((seconds - (bitcast<f32>(p.size_op.z) - life)) / life, 0.0, 1.0) * f32(ROWS - 1u);
    let a = u32(floor(row)); let b = min(a + 1u, ROWS - 1u);
    let base = tiles() + k;
    return mix(third[base + a * COUNT].w, third[base + b * COUNT].w, row - floor(row));
}
struct Forces { pull: vec2<f32>, turbulence: f32, scale: f32, push: f32, flow: f32, centre: vec2<f32>, attract: f32, vortex: f32, bounce: f32 }
fn forces_at(seconds: f32) -> Forces {
    var f: Forces;
    f.pull = vec2<f32>(setting(WIND, seconds), setting(GRAVITY, seconds));
    f.turbulence = setting(TURBULENCE, seconds); f.scale = setting(TURBULENCE_SCALE, seconds);
    f.push = setting(PUSH, seconds); f.flow = setting(FLOW, seconds);
    f.centre = vec2<f32>(setting(PULL_X, seconds), setting(PULL_Y, seconds));
    f.attract = setting(PULL, seconds); f.vortex = setting(VORTEX, seconds);
    f.bounce = setting(BOUNCE, seconds);
    return f;
}
// The curl of a noise potential: a swirling flow with no sources or sinks.
fn swirl(at: vec2<f32>, seconds: f32, f: Forces) -> vec2<f32> {
    let q = vec3<f32>(at / f.scale, seconds * 0.35);
    let e = 0.02;
    let dx = potential(q + vec3<f32>(e, 0.0, 0.0)) - potential(q - vec3<f32>(e, 0.0, 0.0));
    let dy = potential(q + vec3<f32>(0.0, e, 0.0)) - potential(q - vec3<f32>(0.0, e, 0.0));
    return vec2<f32>(dy, -dx) / (2.0 * e) * f.turbulence;
}
fn field_at(at: vec2<f32>) -> vec4<f32> {
    let x = u32(clamp(at.x, 0.0, f32(p.size_op.x) - 1.0)); let y = u32(clamp(at.y, 0.0, f32(p.size_op.y) - 1.0));
    return third[y * p.size_op.x + x];
}
fn solid(at: vec2<f32>) -> f32 {
    if at.x < 0.0 || at.y < 0.0 || at.x >= f32(p.size_op.x) || at.y >= f32(p.size_op.y) { return 0.0; }
    return field_at(at).z;
}
// Where a particle is `age` seconds after its birth at `origin`: launched at `launch`,
// then gravity, wind, the painted field, the pull point and the swirl act on it, in
// fixed steps of the nominal life so its path is the same at every frame; it bounces
// off the obstacle, and is steered onto `goal` over the last part of its life.
fn place(origin: vec3<f32>, launch: vec3<f32>, birth: f32, age: f32, own_life: f32, goal: vec3<f32>, homing: f32, deep: bool) -> vec3<f32> {
    var at = origin; var velocity = launch;
    let flags = u32(p.extra.z);
    var steps = 32u;
    if (flags & 2u) != 0u { steps = 96u; }
    let step = p.extra.w / f32(steps);
    var done = 0.0;
    var f: Forces;
    for (var k = 0u; k < steps; k++) {
        let dt = min(step, age - done);
        if dt <= 0.0 { break; }
        // Forces follow their keys, re-read a few times over the life.
        if k % 8u == 0u { f = forces_at(birth + done); }
        var push = f.pull;
        if (flags & 1u) != 0u && at.x >= 0.0 && at.y >= 0.0 && at.x < f32(p.size_op.x) && at.y < f32(p.size_op.y) {
            let slope = field_at(at.xy).xy * 100.0;
            push += -slope * f.push + vec2<f32>(-slope.y, slope.x) * f.flow;
        }
        if f.attract != 0.0 || f.vortex != 0.0 {
            let to = f.centre - at.xy; let far = length(to);
            if far > 0.001 {
                let toward = to / far;
                push += (toward * f.attract + vec2<f32>(-toward.y, toward.x) * f.vortex) * (far / (far + 40.0));
            }
        }
        velocity += vec3<f32>(push, 0.0) * dt;
        var drift = velocity;
        if f.turbulence > 0.0 {
            // In depth the swirl also carries particles towards and away from the viewer.
            var through = 0.0;
            if deep { through = (value_noise(vec3<f32>(at.xy / f.scale * 1.3 + vec2<f32>(91.0, 7.0), (birth + done) * 0.35 + 13.0)) - 0.5) * 2.0 * f.turbulence; }
            drift += vec3<f32>(swirl(at.xy, birth + done, f), through);
        }
        var next = at + drift * dt;
        // Look along the move every two pixels or so, so a thin obstacle is not stepped over.
        var hit = false;
        if (flags & 2u) != 0u && solid(at.xy) <= 0.5 {
            let checks = clamp(u32(ceil(length(next.xy - at.xy) / 2.0)), 1u, 12u);
            var clear = at;
            for (var c = 1u; c <= checks; c++) {
                let probe = mix(at, next, f32(c) / f32(checks));
                if solid(probe.xy) > 0.5 { hit = true; next = probe; break; }
                clear = probe;
            }
            if hit { at = clear; }
        }
        if hit {
            // Bounce off the obstacle's edge: reflect about the way out of it.
            var out = vec2<f32>(solid(next.xy - vec2<f32>(2.0, 0.0)) - solid(next.xy + vec2<f32>(2.0, 0.0)),
                                solid(next.xy - vec2<f32>(0.0, 2.0)) - solid(next.xy + vec2<f32>(0.0, 2.0)));
            if dot(out, out) < 1e-6 { out = -drift.xy; }
            out = normalize(out);
            velocity = vec3<f32>(velocity.xy - 2.0 * min(dot(velocity.xy, out), 0.0) * out, velocity.z) * f.bounce;
            next = at;
        }
        at = next;
        done += dt;
    }
    if p.extra.y > 0.5 && homing > 0.0 {
        at = mix(at, goal, smoothstep(1.0 - homing, 1.0, age / own_life));
    }
    return at;
}
// A colour with its hue turned by `angle` radians (about the grey axis).
fn turned(c: vec3<f32>, angle: f32) -> vec3<f32> {
    let k = vec3<f32>(0.57735027);
    let cs = cos(angle);
    return max(c * cs + cross(k, c) * sin(angle) + k * dot(k, c) * (1.0 - cs), vec3<f32>(0.0));
}
// Where a point of the scene lands in the frame, and how large a pixel-sized thing
// there is drawn (0 when it is behind the camera).
fn filmed(at: vec3<f32>) -> vec3<f32> {
    let base = tiles() + ROWS * COUNT;
    let v = vec3<f32>(
        dot(vec4<f32>(third[base].w, third[base + 1u].w, third[base + 2u].w, third[base + 3u].w), vec4<f32>(at, 1.0)),
        dot(vec4<f32>(third[base + 4u].w, third[base + 5u].w, third[base + 6u].w, third[base + 7u].w), vec4<f32>(at, 1.0)),
        dot(vec4<f32>(third[base + 8u].w, third[base + 9u].w, third[base + 10u].w, third[base + 11u].w), vec4<f32>(at, 1.0)));
    let focal = third[base + 12u].w;
    let away = -v.z;
    if away <= 1.0 { return vec3<f32>(0.0); }
    return vec3<f32>(vec2<f32>(p.size_op.xy) * 0.5 + focal * v.xy / away, focal / away);
}
fn unpremultiplied(c: vec4<f32>) -> vec3<f32> {
    if c.a <= 0.0 { return vec3<f32>(1.0); }
    return c.rgb / c.a;
}
fn add_light(pixel: vec2<i32>, amount: vec4<f32>) {
    if pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(p.size_op.x) || pixel.y >= i32(p.size_op.y) { return; }
    let base = (u32(pixel.y) * p.size_op.x + u32(pixel.x)) * 4u;
    let add = vec4<u32>(clamp(amount * SCALE + vec4<f32>(0.5), vec4<f32>(0.0), vec4<f32>(1048576.0)));
    if add.w == 0u && add.x + add.y + add.z == 0u { return; }
    atomicAdd(&result[base], add.x); atomicAdd(&result[base + 1u], add.y);
    atomicAdd(&result[base + 2u], add.z); atomicAdd(&result[base + 3u], add.w);
}
// A soft dot of `light` at `at`, added into the output.
fn stamp(at: vec2<f32>, radius: f32, light: vec4<f32>) {
    let sigma = max(radius * 0.5, 0.45);
    let reach = i32(ceil(max(radius, 1.0)));
    let centre = vec2<i32>(floor(at));
    for (var j = -reach; j <= reach; j++) {
        for (var i = -reach; i <= reach; i++) {
            let pixel = centre + vec2<i32>(i, j);
            let d = vec2<f32>(pixel) + vec2<f32>(0.5) - at;
            add_light(pixel, light * exp(-dot(d, d) / (2.0 * sigma * sigma)));
        }
    }
}
fn sprite_texel(x: i32, y: i32) -> vec4<f32> {
    if x < 0 || y < 0 || x >= i32(MIP) || y >= i32(MIP) { return vec4<f32>(0.0); }
    return fourth[tiles() + u32(y) * MIP + u32(x)];
}
// The sprite image, `radius` wide either side of `at` and turned by `angle`, tinted.
fn stamp_sprite(at: vec2<f32>, radius: f32, angle: f32, tint: vec4<f32>) {
    let half = vec2<f32>(radius, radius * f32(p.size_op.y) / f32(p.size_op.x));
    let reach = i32(ceil(length(half)));
    let centre = vec2<i32>(floor(at));
    let c = cos(angle); let s = sin(angle);
    for (var j = -reach; j <= reach; j++) {
        for (var i = -reach; i <= reach; i++) {
            let pixel = centre + vec2<i32>(i, j);
            let d = vec2<f32>(pixel) + vec2<f32>(0.5) - at;
            let local = vec2<f32>(c * d.x + s * d.y, -s * d.x + c * d.y) / half;
            if abs(local.x) >= 1.0 || abs(local.y) >= 1.0 { continue; }
            let uv = (local * 0.5 + 0.5) * f32(MIP) - 0.5;
            let base = vec2<i32>(floor(uv)); let f = uv - floor(uv);
            let texel = mix(mix(sprite_texel(base.x, base.y), sprite_texel(base.x + 1, base.y), f.x),
                            mix(sprite_texel(base.x, base.y + 1), sprite_texel(base.x + 1, base.y + 1), f.x), f.y);
            add_light(pixel, vec4<f32>(texel.rgb * tint.rgb, texel.a) * tint.a);
        }
    }
}
@compute @workgroup_size(64)
fn particle_splat(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let size = vec2<f32>(p.size_op.xy);
    let now = bitcast<f32>(p.size_op.z);
    let life = p.extra.w;
    let start = setting(START, now); let duration = setting(DURATION, now);
    // Each thread emits one particle after another, a life apart, from its own phase.
    let turn = (now - start) / life + random(thread, 0u, 0u);
    let generation = u32(i32(floor(turn)) + 1048576);
    let age = fract(turn) * life;
    let birth = now - age;
    if birth < start || (duration > 0.0 && birth >= start + duration) { return; }
    let variation = setting(VARIATION, birth);
    let own_life = life * (1.0 - 0.6 * variation * random(thread, generation, 1u));
    if age >= own_life { return; }
    var origin: vec3<f32>;
    var born_on = vec3<f32>(1.0);
    if p.extra.x > 0.5 {
        // One cell of the frame per group of threads; born where the emitter has alpha.
        let stride = u32(p.values.z); let slots = u32(p.values.w);
        let across = (p.size_op.x + stride - 1u) / stride;
        let cell = thread / slots;
        origin = vec3<f32>((vec2<f32>(f32(cell % across), f32(cell / across))
            + vec2<f32>(random(thread, generation, 2u), random(thread, generation, 3u))) * f32(stride), 0.0);
        if origin.x >= size.x || origin.y >= size.y { return; }
        let born = bitcast<vec4<f32>>(first[u32(origin.y) * p.size_op.x + u32(origin.x)]);
        let chance = setting(DENSITY, birth) * life / 10000.0 * f32(stride * stride) / f32(slots);
        if random(thread, generation, 4u) >= clamp(born.a, 0.0, 1.0) * chance { return; }
        born_on = unpremultiplied(born);
    } else {
        if random(thread, generation, 4u) >= setting(RATE, birth) * life / p.values.x { return; }
        origin = vec3<f32>(vec2<f32>(setting(BOX_X, birth), setting(BOX_Y, birth))
            + (vec2<f32>(random(thread, generation, 2u), random(thread, generation, 3u)) - 0.5)
            * vec2<f32>(setting(BOX_W, birth), setting(BOX_H, birth)), 0.0);
    }
    // Depth spreads births in front of and behind the frame.
    let depth = setting(DEPTH, birth);
    let deep = depth > 0.0;
    origin.z = (random(thread, generation, 13u) - 0.5) * depth;
    // A place on the target, drawn in proportion to its alpha: first the tile, by the
    // cumulative table, then the pixel in it. No particle goes without a place.
    var goal = origin; var lands_on = born_on; var landing = 0.0;
    let homing = setting(HOMING, birth);
    if p.extra.y > 0.5 {
        let count = tiles();
        let total = third[count - 1u].w;
        if total <= 0.0 { return; }
        let wanted = random(thread, generation, 8u) * total;
        var low = 0u; var high = count - 1u;
        for (var k = 0u; k < 16u; k++) {
            if low >= high { break; }
            let middle = (low + high) / 2u;
            if third[middle].w < wanted { low = middle + 1u; } else { high = middle; }
        }
        // Walk the tile's pixels to the one where the running alpha passes the draw.
        let t = tile();
        let across = (p.size_op.x + t - 1u) / t;
        let x0 = (low % across) * t; let y0 = (low / across) * t;
        var before = 0.0;
        if low > 0u { before = third[low - 1u].w; }
        let within = wanted - before;
        var run = 0.0;
        var found = false;
        for (var y = y0; y < min(y0 + t, p.size_op.y); y++) {
            for (var x = x0; x < min(x0 + t, p.size_op.x); x++) {
                let there = second[y * p.size_op.x + x];
                run += clamp(there.a, 0.0, 1.0);
                if run >= within && there.a > 0.0 {
                    goal = vec3<f32>(f32(x) + random(thread, generation, 16u), f32(y) + random(thread, generation, 17u), 0.0);
                    lands_on = unpremultiplied(there); found = true; break;
                }
            }
            if found { break; }
        }
        if !found { return; }
        if p.extra.x < 0.5 { born_on = lands_on; }
        if homing > 0.0 { landing = smoothstep(1.0 - homing, 1.0, age / own_life); }
    }
    let heading = radians(setting(DIRECTION, birth)) + (random(thread, generation, 5u) - 0.5) * radians(setting(SPREAD, birth));
    // In depth the spread is a cone: the launch also leans towards or away from the viewer.
    var lean = 0.0;
    if deep { lean = (random(thread, generation, 14u) - 0.5) * radians(setting(SPREAD, birth)); }
    let launch = vec3<f32>(cos(heading) * cos(lean), -sin(heading) * cos(lean), sin(lean))
        * setting(SPEED, birth) * (1.0 - 0.7 * variation * random(thread, generation, 6u));
    let where_now = place(origin, launch, birth, age, own_life, goal, homing, deep);
    let seen = filmed(where_now);
    if seen.z <= 0.0 { return; }
    let at = seen.xy;
    let share = age / own_life;
    let fade = setting(FADE, birth);
    var opacity = min(share / 0.04, 1.0);
    if fade > 0.0 { opacity *= clamp((1.0 - share) / fade, 0.0, 1.0); }
    let first_colour = vec4<f32>(setting(COLOUR, birth), setting(COLOUR + 1u, birth), setting(COLOUR + 2u, birth), setting(COLOUR_A, birth));
    let last_colour = vec4<f32>(setting(END_COLOUR, birth), setting(END_COLOUR + 1u, birth), setting(END_COLOUR + 2u, birth), setting(END_A, birth));
    var colour = mix(first_colour, last_colour, share);
    colour = vec4<f32>(colour.rgb * mix(vec3<f32>(1.0), mix(born_on, lands_on, landing), setting(INHERIT, birth)), colour.a);
    let hues = setting(HUES, birth);
    if hues > 0.0 {
        colour = vec4<f32>(turned(colour.rgb, (random(thread, generation, 12u) - 0.5) * 6.2831853 * hues), colour.a);
    }
    // Near particles are drawn larger, up to a limit that keeps one particle affordable.
    let radius = min(setting(SIZE, birth) * (1.0 - 0.6 * variation * random(thread, generation, 7u))
        * mix(1.0, setting(GROW, birth), share) * seen.z, 48.0);
    if radius <= 0.0 { return; }
    if (u32(p.extra.z) & 4u) != 0u {
        // Spin either way, from a random angle when spinning at all.
        let spin = radians(setting(SPIN, birth));
        var angle = 0.0;
        if spin != 0.0 {
            angle = random(thread, generation, 9u) * 6.2831853
                + spin * age * (1.0 - variation * random(thread, generation, 10u)) * sign(random(thread, generation, 11u) - 0.5);
        }
        stamp_sprite(at, radius, angle, vec4<f32>(colour.rgb, colour.a * opacity));
        // A streak leaves fading copies back along the path.
        let ghost_trail = min(setting(STREAK, birth), age);
        if ghost_trail > 0.0 {
            for (var g = 1u; g <= 5u; g++) {
                let back = f32(g) / 5.0;
                let was = filmed(place(origin, launch, birth, age - ghost_trail * back, own_life, goal, homing, deep));
                if was.z <= 0.0 { continue; }
                stamp_sprite(was.xy, radius * (1.0 - 0.3 * back), angle, vec4<f32>(colour.rgb, colour.a * opacity * 0.5 * (1.0 - back * 0.85)));
            }
        }
        return;
    }
    let light = vec4<f32>(colour.rgb * colour.a, colour.a) * opacity;
    // A streak is a row of dots back along the path, fading towards the tail.
    let trail = min(setting(STREAK, birth), age);
    if trail <= 0.0 { stamp(at, radius, light); return; }
    let tail = filmed(place(origin, launch, birth, age - trail, own_life, goal, homing, deep));
    if tail.z <= 0.0 { stamp(at, radius, light); return; }
    var back = tail.xy - at;
    let far = length(back);
    if far > 96.0 { back *= 96.0 / far; }
    let spacing = max(radius, 1.0) * 0.6;
    let dots = clamp(u32(ceil(min(far, 96.0) / spacing)) + 1u, 1u, 48u);
    // Overlapping dots add up; share the light so a streak is no brighter than a dot.
    let each = light * min(1.0, 1.1 * spacing / max(radius, 1.0));
    for (var k = 0u; k < dots; k++) {
        let along = f32(k) / f32(max(dots - 1u, 1u));
        var one = each * (1.0 - 0.85 * along);
        if dots == 1u { one = light; }
        stamp(at + back * along, radius * (1.0 - 0.5 * along), one);
    }
}
// ---- resolve: source = splat sums, second = fields ----
@compute @workgroup_size(8, 8)
fn particle_resolve(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    let sum = vec4<f32>(first[i]) / SCALE;
    // Glow adds the light; without it dots cover one another and alpha saturates softly.
    let glow = second[tiles() + (ROWS - 1u) * COUNT + GLOW].w;
    let covered = 1.0 - exp(-sum.a);
    var plain = vec4<f32>(0.0);
    if sum.a > 0.0 { plain = vec4<f32>(sum.rgb / sum.a * covered, covered); }
    store(i, mix(plain, vec4<f32>(sum.rgb, min(sum.a, 1.0)), glow));
}
