// A flock, simulated (see simulation.rs): the state is two values a thread, where it is
// and its generation, then its velocity and whether it is alive. Each step first adds
// every particle into a hashed grid of cells the neighbour radius wide (count, sum of
// places and of velocities, in fixed point so the sums are the same in any order), and
// into a fine grid of cells a third as wide, which is then turned into a list of the
// particles in each fine cell (counts, a running total for where each cell's list
// starts, then the particles). Each particle then steers towards its neighbours' cells
// and into their heading, keeps clear of the particles themselves near it (their
// pushes summed in fixed point, so in any order the same), and is moved by the forces.
const FLOCK_RADIUS = 120u; const COHESION = 121u; const SEPARATION = 122u; const ALIGNMENT = 123u;
const TOP_SPEED = 124u;
const CELLS = 65536u;
// The fine grid, in the grid buffer after the cells (eight values each): counts, where
// each list starts, how many are listed so far, sums of places (four values each), then
// the lists. In a fine cell with more particles than are walked, an even share of them
// is walked, picked by each one's own number (not by where it happens to be in the
// list) and counted for the rest; one beyond LIST_MOST is kept clear of as a whole.
const FINE = 65536u; const FINE_COUNT = 524288u; const FINE_START = 589824u;
const FINE_CURSOR = 655360u; const FINE_SUM = 720896u; const FINE_LIST = 983040u;
const FINE_MOST = 96u; const LIST_MOST = 1024u;
// How near another particle must come before it is kept clear of.
fn personal(r: f32) -> f32 { return max(r / 3.0, 1.0); }
fn grid_u32(k: u32) -> u32 { let v = first[k / 4u]; return v[k % 4u]; }
// Fixed point: places in a cell in 4096ths of its width, velocities in 16ths of a pixel
// a second.
const PLACE_UNITS = 4096.0; const SPEED_UNITS = 16.0;
fn simulated() -> bool { return camera_row(118u) > 0.5; }
// The grid is shifted by a different amount at every step (from the step's time, so
// the same at every run): cell borders never stay put, so particles kept clear of the
// cells' crowds do not settle into bands along them.
fn grid_shift(r: f32) -> vec3<f32> {
    let step = p.size_op.z;
    return vec3<f32>(f32(hash(step) >> 8u), f32(hash(step ^ 0x5bd1e995u) >> 8u), f32(hash(step ^ 0x27d4eb2du) >> 8u))
        / 16777216.0 * r;
}
fn flock_cell(at: vec3<f32>, r: f32) -> vec3<i32> { return vec3<i32>(floor((at + grid_shift(r)) / r)); }
fn cell_slot(c: vec3<i32>) -> u32 {
    return hash(bitcast<u32>(c.x) * 73856093u ^ bitcast<u32>(c.y) * 19349663u ^ bitcast<u32>(c.z) * 83492791u) & (CELLS - 1u);
}
fn add_signed(k: u32, v: i32) { atomicAdd(&result[k], bitcast<u32>(v)); }
// What a particle adds to its cell, in fixed point.
fn place_units(at: vec3<f32>, c: vec3<i32>, r: f32) -> vec3<i32> {
    return vec3<i32>(round((at + grid_shift(r) - vec3<f32>(c) * r) / r * PLACE_UNITS));
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
    let near = personal(r); let fc = flock_cell(s0.xyz, near); let f = cell_slot(fc);
    let fat = place_units(s0.xyz, fc, near);
    atomicAdd(&result[FINE_COUNT + f], 1u);
    add_signed(FINE_SUM + 4u * f, fat.x); add_signed(FINE_SUM + 4u * f + 1u, fat.y); add_signed(FINE_SUM + 4u * f + 2u, fat.z);
}
// ---- where each fine cell's list starts: one group, each of its threads totalling
// a share of the cells, then a running total over the shares ----
var<workgroup> shares: array<u32, 256>;
@compute @workgroup_size(256)
fn particle_flock_scan(@builtin(local_invocation_index) i: u32) {
    let per = FINE / 256u; let base = i * per;
    var sum = 0u;
    for (var k = 0u; k < per; k++) { sum += atomicLoad(&result[FINE_COUNT + base + k]); }
    shares[i] = sum;
    workgroupBarrier();
    if i == 0u {
        var run = 0u;
        for (var k = 0u; k < 256u; k++) { let share = shares[k]; shares[k] = run; run += share; }
    }
    workgroupBarrier();
    var at = shares[i];
    for (var k = 0u; k < per; k++) {
        atomicStore(&result[FINE_START + base + k], at);
        at += atomicLoad(&result[FINE_COUNT + base + k]);
    }
}
// ---- each particle into its fine cell's list (in no particular order: whatever reads
// a list adds in fixed point) ----
@compute @workgroup_size(64)
fn particle_flock_list(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let s0 = second[2u * thread]; let s1 = second[2u * thread + 1u];
    if s1.w < 0.5 { return; }
    let r = max(setting(FLOCK_RADIUS, bitcast<f32>(p.size_op.z)), 1.0);
    let f = cell_slot(flock_cell(s0.xyz, personal(r)));
    let at = atomicLoad(&result[FINE_START + f]) + atomicAdd(&result[FINE_CURSOR + f], 1u);
    atomicStore(&result[FINE_LIST + at], thread);
}
// How the particles near one at `at` (thread `me`) push it clear of them: each one
// nearer than `personal` away pushes along the line between them, the harder the
// nearer, summed in 256ths so the order of the list does not matter. Two in the very
// same place part along a direction of their own pair, opposite for each.
fn crowding(at: vec3<f32>, me: u32, now: f32, deep: bool) -> vec3<f32> {
    let near = personal(max(setting(FLOCK_RADIUS, now), 1.0));
    let own = flock_cell(at, near);
    var push = vec3<i32>(0);
    var reach = 1; if !deep { reach = 0; }
    for (var dz = -reach; dz <= reach; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let c = own + vec3<i32>(dx, dy, dz);
                let f = cell_slot(c);
                let n = grid_u32(FINE_COUNT + f);
                if n == 0u { continue; }
                if n > LIST_MOST {
                    let sums = vec3<f32>(f32(bitcast<i32>(grid_u32(FINE_SUM + 4u * f))), f32(bitcast<i32>(grid_u32(FINE_SUM + 4u * f + 1u))), f32(bitcast<i32>(grid_u32(FINE_SUM + 4u * f + 2u))));
                    let middle = vec3<f32>(c) * near + sums / f32(n) * (near / PLACE_UNITS) - grid_shift(near);
                    var away = at - middle; if !deep { away.z = 0.0; }
                    let d = length(away);
                    if d < near && d > 0.001 { push += vec3<i32>(round(away / d * (1.0 - d / near) * f32(n) * 256.0)); }
                    continue;
                }
                let start = grid_u32(FINE_START + f);
                let every = (n + FINE_MOST - 1u) / FINE_MOST;
                for (var m = 0u; m < LIST_MOST; m++) {
                    if m >= n { break; }
                    let other = grid_u32(FINE_LIST + start + m);
                    if other == me || (every > 1u && hash(other ^ 0x68e31da4u) % every != 0u) { continue; }
                    var away = at - second[2u * other].xyz; if !deep { away.z = 0.0; }
                    let d = length(away);
                    if d >= near { continue; }
                    var along: vec3<f32>;
                    if d > 0.0001 {
                        along = away / d;
                    } else {
                        let a = f32(hash(min(me, other) * 2654435761u ^ max(me, other)) >> 8u) / 16777216.0 * 6.2831853;
                        along = vec3<f32>(cos(a), sin(a), 0.0) * select(-1.0, 1.0, me < other);
                    }
                    push += vec3<i32>(round(along * (1.0 - d / near) * 256.0 * f32(every)));
                }
            }
        }
    }
    return vec3<f32>(push) / 256.0;
}
// How the neighbours steer a particle at `at` going at `v` (it is in the grid itself).
fn flocking(at: vec3<f32>, v: vec3<f32>, now: f32, deep: bool, me: u32) -> vec3<f32> {
    let r = max(setting(FLOCK_RADIUS, now), 1.0);
    let own = flock_cell(at, r);
    var count = 0.0; var middle = vec3<f32>(0.0); var heading = vec3<f32>(0.0);
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
                let centre = vec3<f32>(c) * r + places / n * (r / PLACE_UNITS) - grid_shift(r);
                count += n; middle += centre * n; heading += speeds / SPEED_UNITS;
            }
        }
    }
    if count < 0.5 { return vec3<f32>(0.0); }
    var steer = (middle / count - at) / r * setting(COHESION, now)
        + (heading / count - v) * setting(ALIGNMENT, now)
        + crowding(at, me, now, deep) * setting(SEPARATION, now);
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
// A bird born on its emitter keeps the emitter's colour where it was born in its state's
// alive value: 1 is alive with no colour of its own, 2 + the colour packed 7 bits a
// channel (exact in a float) is alive in that colour.
fn alive_value(b: Born) -> f32 {
    if p.extra.x > -0.5 { return 1.0; }
    let c = vec3<u32>(round(clamp(b.born_on, vec3<f32>(0.0), vec3<f32>(1.0)) * 127.0));
    return 2.0 + f32(c.x | (c.y << 7u) | (c.z << 14u));
}
// The colour a simulated bird was born in, read with its place (w 1 when it has one).
var<private> held_tint: vec4<f32>;
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
            let steer = flocking(s0.xyz, s1.xyz, now - span, b.deep, thread);
            moved = advance(s0.xyz, s1.xyz, now - span, span, b.deep, steer);
        } else {
            // Born during this step: from its birth on, steered by no one yet.
            moved = advance(b.origin, b.launch, b.birth, now - b.birth, b.deep, vec3<f32>(0.0));
        }
        out0 = vec4<f32>(moved.at, f32(b.generation)); out1 = vec4<f32>(moved.velocity, alive_value(b));
    }
    store(2u * thread, out0); store(2u * thread + 1u, out1);
}
// Where a flock's particle is at the frame, from the simulated state bound as `first`
// (false when it is not alive there).
fn simulated_at(thread: u32, generation: u32) -> bool {
    let s0 = bitcast<vec4<f32>>(first[2u * thread]); let s1 = bitcast<vec4<f32>>(first[2u * thread + 1u]);
    if s1.w < 0.5 || s0.w != f32(generation) { return false; }
    held = vec4<f32>(s0.xyz, 1.0);
    held_tint = vec4<f32>(0.0);
    if s1.w >= 1.5 {
        let code = u32(s1.w - 2.0);
        held_tint = vec4<f32>(vec3<f32>(f32(code & 127u), f32((code >> 7u) & 127u), f32((code >> 14u) & 127u)) / 127.0, 1.0);
    }
    return true;
}
