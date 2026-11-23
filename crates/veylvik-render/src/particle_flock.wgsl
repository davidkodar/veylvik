// A flock, simulated (see simulation.rs): the state is two values a thread, where it is
// and its generation, then its velocity and whether it is alive. Each step first adds
// every particle into a hashed grid of cells the neighbour radius wide (count, sum of
// places and of velocities, in fixed point so the sums are the same in any order), then
// moves each particle by its neighbours' cells and the forces.
const FLOCK_RADIUS = 120u; const COHESION = 121u; const SEPARATION = 122u; const ALIGNMENT = 123u;
const TOP_SPEED = 124u;
const CELLS = 65536u;
// Fixed point: places in a cell in 4096ths of its width, velocities in 16ths of a pixel
// a second.
const PLACE_UNITS = 4096.0; const SPEED_UNITS = 16.0;
fn simulated() -> bool { return camera_row(118u) > 0.5; }
fn flock_cell(at: vec3<f32>, r: f32) -> vec3<i32> { return vec3<i32>(floor(at / r)); }
fn cell_slot(c: vec3<i32>) -> u32 {
    return hash(bitcast<u32>(c.x) * 73856093u ^ bitcast<u32>(c.y) * 19349663u ^ bitcast<u32>(c.z) * 83492791u) & (CELLS - 1u);
}
fn add_signed(k: u32, v: i32) { atomicAdd(&result[k], bitcast<u32>(v)); }
// What a particle adds to its cell, in fixed point.
fn place_units(at: vec3<f32>, c: vec3<i32>, r: f32) -> vec3<i32> {
    return vec3<i32>(round((at - vec3<f32>(c) * r) / r * PLACE_UNITS));
}
fn speed_units(v: vec3<f32>) -> vec3<i32> {
    return vec3<i32>(round(clamp(v, vec3<f32>(-100000.0), vec3<f32>(100000.0)) * SPEED_UNITS));
}
// ---- flock grid: second = the state; result = the cells, eight values each ----
@compute @workgroup_size(64)
fn particle_flock_grid(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let s0 = second[2u * thread]; let s1 = second[2u * thread + 1u];
    if s1.w < 0.5 { return; }
    let r = max(setting(FLOCK_RADIUS, bitcast<f32>(p.size_op.z)), 1.0);
    let c = flock_cell(s0.xyz, r); let k = cell_slot(c) * 8u;
    let at = place_units(s0.xyz, c, r); let v = speed_units(s1.xyz);
    atomicAdd(&result[k], 1u);
    add_signed(k + 1u, at.x); add_signed(k + 2u, at.y); add_signed(k + 3u, at.z);
    add_signed(k + 4u, v.x); add_signed(k + 5u, v.y); add_signed(k + 6u, v.z);
}
// How the neighbours steer a particle at `at` going at `v` (it is in the grid itself).
fn flocking(at: vec3<f32>, v: vec3<f32>, now: f32, deep: bool) -> vec3<f32> {
    let r = max(setting(FLOCK_RADIUS, now), 1.0);
    let own = flock_cell(at, r);
    var count = 0.0; var middle = vec3<f32>(0.0); var heading = vec3<f32>(0.0); var apart = vec3<f32>(0.0);
    var reach = 1; if !deep { reach = 0; }
    for (var dz = -reach; dz <= reach; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let c = own + vec3<i32>(dx, dy, dz);
                let k = cell_slot(c) * 2u;
                let a = first[k]; let b = first[k + 1u];
                var n = f32(a.x);
                var places = vec3<f32>(f32(bitcast<i32>(a.y)), f32(bitcast<i32>(a.z)), f32(bitcast<i32>(a.w)));
                var speeds = vec3<f32>(f32(bitcast<i32>(b.x)), f32(bitcast<i32>(b.y)), f32(bitcast<i32>(b.z)));
                if dx == 0 && dy == 0 && dz == 0 {
                    // Not counting itself.
                    n -= 1.0;
                    places -= vec3<f32>(place_units(at, c, r)); speeds -= vec3<f32>(speed_units(v));
                }
                if n < 0.5 { continue; }
                let centre = vec3<f32>(c) * r + places / n * (r / PLACE_UNITS);
                count += n; middle += centre * n; heading += speeds / SPEED_UNITS;
                // Keep clear of the cell's crowd, the harder the nearer.
                let away = at - centre; let far = length(away);
                if far < r && far > 0.001 { apart += away / far * (1.0 - far / r) * n; }
            }
        }
    }
    if count < 0.5 { return vec3<f32>(0.0); }
    var steer = (middle / count - at) / r * setting(COHESION, now)
        + (heading / count - v) * setting(ALIGNMENT, now)
        + apart * setting(SEPARATION, now);
    if !deep { steer.z = 0.0; }
    return steer;
}
// One particle moved on by `span` seconds from `since` at the place and speed given.
struct Moved { at: vec3<f32>, velocity: vec3<f32> }
fn advance(start: vec3<f32>, launch: vec3<f32>, since: f32, span: f32, deep: bool, steer: vec3<f32>) -> Moved {
    var at = start; var velocity = launch;
    let parts = clamp(u32(ceil(span * 240.0)), 1u, 16u);
    let h = span / f32(parts);
    let forced = force_count() > 0u; let solids = solid_count() > 0u;
    let top = setting(TOP_SPEED, since + span);
    for (var k = 0u; k < parts; k++) {
        let t = since + f32(k) * h;
        let f = forces_at(t);
        if forced { forces_now(t); }
        var push = f.pull;
        if f.attract != 0.0 || f.vortex != 0.0 {
            let to = f.centre - at.xy; let far = length(to);
            if far > 0.001 {
                let toward = to / far;
                push += (toward * f.attract + vec2<f32>(-toward.y, toward.x) * f.vortex) * (far / (far + 40.0));
            }
        }
        velocity += (vec3<f32>(push, 0.0) + steer) * h;
        var stir = vec3<f32>(0.0);
        if forced { let pushed = forces_push(at, velocity, t, h); velocity = pushed.velocity; stir = pushed.stir; }
        if top > 0.0 && length(velocity) > top { velocity *= top / length(velocity); }
        var drift = velocity + stir;
        if f.turbulence > 0.0 {
            var through = 0.0;
            if deep { through = (value_noise(vec3<f32>(at.xy / f.scale * 1.3 + vec2<f32>(91.0, 7.0), t * 0.35 + 13.0)) - 0.5) * 2.0 * f.turbulence; }
            drift += vec3<f32>(swirl(at.xy, t, f), through);
        }
        var next = at + drift * h;
        if solids {
            let met = solid_hit(at, next, t + h);
            if met.x >= 0.0 {
                let i = u32(met.y);
                var on = mix(at, next, met.x);
                let out = solid_out(i, on, t + h);
                on += out * max(1.0 - solid_distance(i, on, t + h), 0.0);
                velocity -= (1.0 + f.bounce) * min(dot(velocity, out), 0.0) * out;
                next = on + velocity * h * (1.0 - met.x);
            }
        }
        at = next;
    }
    return Moved(at, velocity);
}
// ---- step: first = the grid, second = the state before; result = the state after,
// at the time in size_op.z, `values.z` seconds on ----
@compute @workgroup_size(64)
fn particle_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let now = bitcast<f32>(p.size_op.z); let span = p.values.z;
    var out0 = vec4<f32>(0.0); var out1 = vec4<f32>(0.0);
    let b = born(thread, 0u, now);
    if b.ok && now - b.birth < b.own_life {
        let s0 = second[2u * thread]; let s1 = second[2u * thread + 1u];
        var moved: Moved;
        if s1.w > 0.5 && s0.w == f32(b.generation) && now - b.birth >= span {
            let steer = flocking(s0.xyz, s1.xyz, now - span, b.deep);
            moved = advance(s0.xyz, s1.xyz, now - span, span, b.deep, steer);
        } else {
            // Born during this step: from its birth on, steered by no one yet.
            moved = advance(b.origin, b.launch, b.birth, now - b.birth, b.deep, vec3<f32>(0.0));
        }
        out0 = vec4<f32>(moved.at, f32(b.generation)); out1 = vec4<f32>(moved.velocity, 1.0);
    }
    store(2u * thread, out0); store(2u * thread + 1u, out1);
}
// Where a flock's particle is at the frame, from the simulated state bound as `first`
// (false when it is not alive there).
fn simulated_at(thread: u32, generation: u32) -> bool {
    let s0 = bitcast<vec4<f32>>(first[2u * thread]); let s1 = bitcast<vec4<f32>>(first[2u * thread + 1u]);
    if s1.w < 0.5 || s0.w != f32(generation) { return false; }
    held = vec4<f32>(s0.xyz, 1.0);
    return true;
}
