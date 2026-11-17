// Particles, in a module of their own on the kernels' bind group layout: the splat pass
// adds fixed-point light into the output with atomics, the resolve pass turns the sums
// into an image. Each particle is a pure function of (thread, generation, time).
// p.size_op: width, height, time (f32 bits), seed. p.values: threads, threads per
// dispatch row. p.extra.x: 1 when an emitter image is connected, .y: 1 for a target.
// block (the node's data image, three values a texel): 0 life, speed, direction
// 1 spread (radians), gravity, wind   2 turbulence, turbulence scale, size
// 3 glow, variation, fade   4 start, duration, box chance   5 image chance, stride, slots
// 6 box x, y   7 box width, height   8 colour   9 end colour   10 their alphas
// 11 inherit, homing, streak
struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<u32>>;
@group(0) @binding(2) var<storage, read> block: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read> goal_image: array<vec4<f32>>;
const SCALE: f32 = 4096.0;
const STEPS: u32 = 32u;
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
// The curl of a noise potential: a swirling flow with no sources or sinks.
fn swirl(at: vec2<f32>, seconds: f32) -> vec2<f32> {
    let q = vec3<f32>(at / block[2].y, seconds * 0.35);
    let e = 0.02;
    let dx = potential(q + vec3<f32>(e, 0.0, 0.0)) - potential(q - vec3<f32>(e, 0.0, 0.0));
    let dy = potential(q + vec3<f32>(0.0, e, 0.0)) - potential(q - vec3<f32>(0.0, e, 0.0));
    return vec2<f32>(dy, -dx) / (2.0 * e) * block[2].x;
}
// Where a particle is `age` seconds after its birth at `origin`: launched at `launch`,
// pulled by gravity and wind, pushed by the swirl, in fixed steps of the nominal life so
// its path is the same at every frame; then steered onto `goal` as `landing` rises to 1.
fn place(origin: vec2<f32>, launch: vec2<f32>, birth: f32, age: f32, own_life: f32, goal: vec2<f32>) -> vec2<f32> {
    var at = origin; var velocity = launch;
    let step = block[0].x / f32(STEPS);
    let pull = vec2<f32>(block[1].z, block[1].y);
    var done = 0.0;
    for (var k = 0u; k < STEPS; k++) {
        let dt = min(step, age - done);
        if dt <= 0.0 { break; }
        velocity += pull * dt;
        var drift = velocity;
        if block[2].x > 0.0 { drift += swirl(at, birth + done); }
        at += drift * dt;
        done += dt;
    }
    if p.extra.y > 0.5 && block[11].y > 0.0 {
        at = mix(at, goal, smoothstep(1.0 - block[11].y, 1.0, age / own_life));
    }
    return at;
}
fn unpremultiplied(c: vec4<f32>) -> vec3<f32> {
    if c.a <= 0.0 { return vec3<f32>(1.0); }
    return c.rgb / c.a;
}
// A soft dot of `light` at `at`, added into the output.
fn stamp(at: vec2<f32>, radius: f32, light: vec4<f32>) {
    let sigma = max(radius * 0.5, 0.45);
    let reach = i32(ceil(max(radius, 1.0)));
    let centre = vec2<i32>(floor(at));
    for (var j = -reach; j <= reach; j++) {
        for (var i = -reach; i <= reach; i++) {
            let pixel = centre + vec2<i32>(i, j);
            if pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(p.size_op.x) || pixel.y >= i32(p.size_op.y) { continue; }
            let d = vec2<f32>(pixel) + vec2<f32>(0.5) - at;
            let amount = light * exp(-dot(d, d) / (2.0 * sigma * sigma));
            let base = (u32(pixel.y) * p.size_op.x + u32(pixel.x)) * 4u;
            let add = vec4<u32>(clamp(amount * SCALE + vec4<f32>(0.5), vec4<f32>(0.0), vec4<f32>(1048576.0)));
            if add.w == 0u && add.x + add.y + add.z == 0u { continue; }
            atomicAdd(&result[base], add.x); atomicAdd(&result[base + 1u], add.y);
            atomicAdd(&result[base + 2u], add.z); atomicAdd(&result[base + 3u], add.w);
        }
    }
}
@compute @workgroup_size(64)
fn particle_splat(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let size = vec2<f32>(p.size_op.xy);
    let now = bitcast<f32>(p.size_op.z);
    let life = block[0].x; let variation = block[3].y;
    // Each thread emits one particle after another, a life apart, from its own phase.
    let turn = (now - block[4].x) / life + random(thread, 0u, 0u);
    let generation = u32(i32(floor(turn)) + 1048576);
    let age = fract(turn) * life;
    let birth = now - age;
    if birth < block[4].x || (block[4].y > 0.0 && birth >= block[4].x + block[4].y) { return; }
    let own_life = life * (1.0 - 0.6 * variation * random(thread, generation, 1u));
    if age >= own_life { return; }
    var origin: vec2<f32>;
    var born_on = vec3<f32>(1.0);
    if p.extra.x > 0.5 {
        // One cell of the frame per group of threads; born where the emitter has alpha.
        let stride = u32(block[5].y); let slots = u32(block[5].z);
        let across = (p.size_op.x + stride - 1u) / stride;
        let cell = thread / slots;
        origin = (vec2<f32>(f32(cell % across), f32(cell / across))
            + vec2<f32>(random(thread, generation, 2u), random(thread, generation, 3u))) * f32(stride);
        if origin.x >= size.x || origin.y >= size.y { return; }
        let born = bitcast<vec4<f32>>(source[u32(origin.y) * p.size_op.x + u32(origin.x)]);
        if random(thread, generation, 4u) >= clamp(born.a, 0.0, 1.0) * block[5].x { return; }
        born_on = unpremultiplied(born);
    } else {
        if random(thread, generation, 4u) >= block[4].z { return; }
        origin = block[6].xy + (vec2<f32>(random(thread, generation, 2u), random(thread, generation, 3u)) - 0.5) * block[7].xy;
    }
    // A place on the target: the first of a fixed number of random spots that has alpha.
    var goal = origin; var lands_on = born_on; var landing = 0.0;
    if p.extra.y > 0.5 {
        var found = false;
        for (var k = 0u; k < 32u; k++) {
            let spot = vec2<f32>(random(thread, generation, 16u + 3u * k), random(thread, generation, 17u + 3u * k)) * size;
            let there = goal_image[u32(spot.y) * p.size_op.x + u32(spot.x)];
            if random(thread, generation, 18u + 3u * k) < there.a {
                goal = spot; lands_on = unpremultiplied(there); found = true; break;
            }
        }
        if !found { return; }
        if p.extra.x < 0.5 { born_on = lands_on; }
        if block[11].y > 0.0 { landing = smoothstep(1.0 - block[11].y, 1.0, age / own_life); }
    }
    let heading = block[0].z + (random(thread, generation, 5u) - 0.5) * block[1].x;
    let launch = vec2<f32>(cos(heading), -sin(heading))
        * block[0].y * (1.0 - 0.7 * variation * random(thread, generation, 6u));
    let at = place(origin, launch, birth, age, own_life, goal);
    let share = age / own_life;
    let fade = block[3].z;
    var opacity = min(share / 0.04, 1.0);
    if fade > 0.0 { opacity *= clamp((1.0 - share) / fade, 0.0, 1.0); }
    var colour = mix(vec4<f32>(block[8].rgb, block[10].x), vec4<f32>(block[9].rgb, block[10].y), share);
    colour = vec4<f32>(colour.rgb * mix(vec3<f32>(1.0), mix(born_on, lands_on, landing), block[11].x), colour.a);
    let light = vec4<f32>(colour.rgb * colour.a, colour.a) * opacity;
    let radius = block[2].z * (1.0 - 0.6 * variation * random(thread, generation, 7u));
    // A streak is a row of dots back along the path, fading towards the tail.
    let trail = min(block[11].z, age);
    if trail <= 0.0 { stamp(at, radius, light); return; }
    var back = place(origin, launch, birth, age - trail, own_life, goal) - at;
    let far = length(back);
    if far > 96.0 { back *= 96.0 / far; }
    let spacing = max(radius, 1.0) * 0.6;
    let dots = clamp(u32(ceil(min(far, 96.0) / spacing)) + 1u, 1u, 48u);
    // Overlapping dots add up; share the light so a streak is no brighter than a dot.
    let each = light * min(1.0, 2.2 * spacing / max(2.0 * max(radius, 1.0), 0.001)) ;
    for (var k = 0u; k < dots; k++) {
        let along = f32(k) / f32(max(dots - 1u, 1u));
        var one = each * (1.0 - 0.85 * along);
        if dots == 1u { one = light; }
        stamp(at + back * along, radius * (1.0 - 0.5 * along), one);
    }
}
@compute @workgroup_size(8, 8)
fn particle_resolve(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    let sum = vec4<f32>(source[i]) / SCALE;
    // Glow adds the light; without it dots cover one another and alpha saturates softly.
    let covered = 1.0 - exp(-sum.a);
    var plain = vec4<f32>(0.0);
    if sum.a > 0.0 { plain = vec4<f32>(sum.rgb / sum.a * covered, covered); }
    let out = mix(plain, vec4<f32>(sum.rgb, min(sum.a, 1.0)), block[3].x);
    atomicStore(&result[i * 4u], bitcast<u32>(out.r)); atomicStore(&result[i * 4u + 1u], bitcast<u32>(out.g));
    atomicStore(&result[i * 4u + 2u], bitcast<u32>(out.b)); atomicStore(&result[i * 4u + 3u], bitcast<u32>(out.a));
}
