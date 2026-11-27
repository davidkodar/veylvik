// Particles, in a module of their own on the kernels' bind group layout. Four passes:
// prepare (tile sums of the target's alpha, and a small copy of the sprite), fields
// (force field, obstacle, the target's cumulative tile table and the settings, packed
// into one image), splat (one thread per particle adds fixed-point light into the
// output with atomics) and resolve (sums to an image). A particle is a pure function
// of (thread, generation, time).
// p.size_op: width, height, time (f32 bits), seed. p.values: threads, threads per
// dispatch row, emitter cell stride, threads per cell. p.extra: emitter (0 none, 1 as it
// is, 2 to 4 an atlas of its past with that many tiles a side),
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
// The glints in the light lighting() last gave (see lit_colour), and whether the grains
// are being counted into the self-shadowing grid rather than drawn.
var<private> glint: vec3<f32>;
var<private> occluding: bool = false;
// Features compiled in only for systems that use them (set when the program is made):
// bouncing off a scene's solids, and Force nodes. Without one its code is left out.
override SOLIDS: bool = true;
override FORCES: bool = true;
// The target is divided into tiles of 16 pixels, or 32 in frames too large for that.
fn tile() -> u32 {
    if p.size_op.x * p.size_op.y > 2500000u { return 32u; }
    return 16u;
}
const MIP: u32 = 64u;
// The sprite sheet: cells across and down (flags bits 8 to 15) and the copy's size.
fn sheet_columns() -> u32 { return ((u32(p.extra.z) >> 8u) & 15u) + 1u; }
fn sheet_rows() -> u32 { return ((u32(p.extra.z) >> 12u) & 15u) + 1u; }
fn sheet_width() -> u32 { return MIP * sheet_columns(); }
fn sheet_texels() -> u32 { return MIP * MIP * sheet_columns() * sheet_rows(); }
const SPRITE_RATE = 125u;
const COUNT: u32 = 138u;
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
const BOX_Z = 42u; const REACH = 43u; const TOWARDS = 44u;
const BURST = 45u; const BURST_SPEED = 46u; const BURST_SPREAD = 47u; const BURST_LIFE = 48u;
const BURST_SIZE = 49u; const BURST_COLOUR = 50u; const BURST_A = 53u;
const TRAIL = 54u; const TRAIL_WIDTH = 55u; const LIGHT_LIMIT = 56u;
const LINK_DISTANCE = 57u; const LINK_OPACITY = 58u; const LINK_WIDTH = 59u;
// Curves over the life: which are set (1 size, 2 opacity, 4 speed, 8 colour), then eight
// values each, and eight RGBA colours.
const CURVES = 60u; const SIZE_CURVE = 61u; const OPACITY_CURVE = 69u; const SPEED_CURVE = 77u;
const COLOUR_CURVE = 85u;
const LIT = 117u; const AMBIENT = 118u; const SCATTER = 119u; const HOLD = 126u;
const SPARKLE = 127u; const SPARKLE_TURN = 128u; const SHADING = 129u; const BOKEH = 130u; const CLUMP = 131u; const CLUMP_RADIUS = 132u;
const SELF_SHADOW = 133u; const GLINT_WHITE = 134u; const SPARKLE_SPREAD = 135u;
// The camera row's lights: how many at 36, then twelve values each from 40.
const LIGHT_COUNT = 36u; const LIGHT_BASE = 40u;
// Where the camera is in the scene: the world-to-camera rows' rotation turned back on
// their offset.
fn eye() -> vec3<f32> {
    let r0 = vec3<f32>(camera_row(0u), camera_row(1u), camera_row(2u));
    let r1 = vec3<f32>(camera_row(4u), camera_row(5u), camera_row(6u));
    let r2 = vec3<f32>(camera_row(8u), camera_row(9u), camera_row(10u));
    return -(r0 * camera_row(3u) + r1 * camera_row(7u) + r2 * camera_row(11u));
}
// How much of light `k`'s light reaches `at` past the scene's solids, from its shadow
// map as 3D surfaces read theirs (1: nothing in the way, or no map).
fn shadow_seen(k: u32, at: vec3<f32>) -> f32 {
    if camera_row(39u) < 0.5 { return 1.0; }
    let base = u32(camera_row(38u)); let side = u32(camera_row(37u));
    let h = base + 20u * k;
    if fourth[h + 16u].z < 0.5 { return 1.0; }
    let a = vec4<f32>(at, 1.0);
    let m0 = vec4<f32>(fourth[h].z, fourth[h + 1u].z, fourth[h + 2u].z, fourth[h + 3u].z);
    let m1 = vec4<f32>(fourth[h + 4u].z, fourth[h + 5u].z, fourth[h + 6u].z, fourth[h + 7u].z);
    let m2 = vec4<f32>(fourth[h + 8u].z, fourth[h + 9u].z, fourth[h + 10u].z, fourth[h + 11u].z);
    let m3 = vec4<f32>(fourth[h + 12u].z, fourth[h + 13u].z, fourth[h + 14u].z, fourth[h + 15u].z);
    let q = vec4<f32>(dot(m0, a), dot(m1, a), dot(m2, a), dot(m3, a));
    if q.w <= 0.0 { return 1.0; }
    let ndc = q.xyz / q.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // Outside the map nothing casts; beyond its far end a point is behind every caster.
    if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 { return 1.0; }
    let depth = min(ndc.z, 1.0);
    let map = base + 128u + k * side * side;
    let centre = vec2<i32>(uv * f32(side));
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let t = clamp(centre + vec2<i32>(x, y), vec2<i32>(0), vec2<i32>(i32(side) - 1));
            let stored = fourth[map + u32(t.y) * side + u32(t.x)].z;
            if depth <= stored + 0.0015 { lit += 1.0; }
        }
    }
    return lit / 9.0;
}
// What the scene's lights do to a particle's colour at `at`: 1 when it is not lit;
// otherwise the light reaching it (as a lit 3D surface there would get it, scattered
// towards the camera), mixed in by how lit it is.
// A particle's glitter facet `age` seconds into its life: a direction of its own,
// turning about an axis of its own at the Sparkle turn rate.
fn facet(thread: u32, generation: u32, age: f32, seconds: f32) -> vec3<f32> {
    let n = normalize(vec3<f32>(random(thread, generation, 51u), random(thread, generation, 52u), random(thread, generation, 53u)) * 2.0 - 1.0 + vec3<f32>(1e-4));
    let k = normalize(vec3<f32>(random(thread, generation, 54u), random(thread, generation, 55u), random(thread, generation, 56u)) * 2.0 - 1.0 + vec3<f32>(1e-4));
    let angle = radians(setting(SPARKLE_TURN, seconds)) * age + random(thread, generation, 57u) * 6.2831853;
    let c = cos(angle);
    return n * c + cross(k, n) * sin(angle) + k * dot(k, n) * (1.0 - c);
}
// How much a grain is lit as the surface it was born on: Shading while it is on it,
// nothing once it has drifted a few dozen pixels (at 1920 wide) from where it was born.
fn attached(surface: vec3<f32>, born_at: vec3<f32>, at: vec3<f32>, seconds: f32) -> f32 {
    if dot(surface, surface) < 0.25 { return 0.0; }
    let reach = 60.0 * f32(p.size_op.x) / 1920.0;
    return setting(SHADING, seconds) * clamp(1.0 - length(at - born_at) / reach, 0.0, 1.0);
}
// The surface a grain is lit as, and how much: the one it was born on near its birth
// place, the one it gathers onto near its place there.
fn surface_light(surface: vec3<f32>, born_at: vec3<f32>, landing: vec3<f32>, goal: vec3<f32>, at: vec3<f32>, seconds: f32) -> vec4<f32> {
    let near_birth = attached(surface, born_at, at, seconds);
    let near_goal = attached(landing, goal, at, seconds);
    if near_goal > near_birth { return vec4<f32>(landing, near_goal); }
    return vec4<f32>(surface, near_birth);
}
fn lighting(at: vec3<f32>, seconds: f32, facet_now: vec3<f32>, surface: vec3<f32>, attach: f32) -> vec3<f32> {
    glint = vec3<f32>(0.0);
    let lit = setting(LIT, seconds);
    if lit <= 0.0 { return vec3<f32>(1.0); }
    // On its surface a grain takes the light as the surface would: lit facing the light,
    // dark facing away, and the ambient light dimmer where the surface faces down.
    var light = vec3<f32>(setting(AMBIENT, seconds)) * mix(1.0, 0.6 - 0.4 * surface.y, attach);
    let g = setting(SCATTER, seconds);
    let sparkle = setting(SPARKLE, seconds);
    // The flash's sharpness: a facet within `spread` degrees of mirroring a light gives at
    // least half its flash. Wider flashes are dimmer by the normalised lobe's peak, so the
    // grains' light overall stays about the same as more of them shimmer. 0: the narrowest.
    let spread = setting(SPARKLE_SPREAD, seconds);
    var shine = 800.0;
    var flash_gain = 1.0;
    if spread > 0.0 {
        shine = log(0.5) / log(cos(radians(min(spread, 45.0))));
        flash_gain = (shine + 2.0) / 802.0;
    }
    let towards_eye = normalize(eye() - at);
    let count = min(u32(camera_row(LIGHT_COUNT)), 6u);
    for (var i = 0u; i < count; i++) {
        let base = LIGHT_BASE + 12u * i;
        let kind = camera_row(base + 3u);
        let source_at = vec3<f32>(camera_row(base), camera_row(base + 1u), camera_row(base + 2u));
        let aim = vec3<f32>(camera_row(base + 4u), camera_row(base + 5u), camera_row(base + 6u));
        var radiance = vec3<f32>(camera_row(base + 8u), camera_row(base + 9u), camera_row(base + 10u));
        // Which way the light travels when it reaches the particle.
        var travel = aim;
        if kind < 1.5 || kind > 2.5 {
            let to = at - source_at;
            let d2 = max(dot(to, to), 1.0);
            travel = to / sqrt(d2);
            // Inverse square, normalised to 1000 pixels, as for 3D surfaces.
            radiance *= 1.0e6 / d2;
            if kind > 0.5 && kind < 1.5 {
                radiance *= smoothstep(camera_row(base + 7u), camera_row(base + 11u), dot(travel, aim));
            }
            // A panel lights only what is in front of it.
            if kind > 2.5 { radiance *= max(dot(travel, aim), 0.0); }
        }
        // What the scene's solids keep of it, and the grains between it and the light.
        radiance *= shadow_seen(i, at);
        var reach = 1.0e9;
        if kind < 1.5 || kind > 2.5 { reach = length(source_at - at); }
        radiance *= transmitted(at, -travel, reach, seconds);
        // Henyey-Greenstein scattering towards the camera, 1 for every direction at g 0.
        let c = dot(travel, towards_eye);
        let phase = (1.0 - g * g) / pow(max(1.0 + g * g - 2.0 * g * c, 1e-4), 1.5);
        light += radiance * mix(phase, 2.0 * max(dot(surface, -travel), 0.0), attach);
        // Glitter: the facet (either side) mirrors this light into the camera.
        if sparkle > 0.0 {
            let half_way = normalize(towards_eye - travel);
            let flash = radiance * sparkle * flash_gain * pow(abs(dot(facet_now, half_way)), shine);
            light += flash;
            glint += flash * lit;
        }
    }
    return mix(vec3<f32>(1.0), light, lit);
}
// A grain's colour lit by `light` from lighting(): its glints take the light's colour
// by Glint white, the rest the grain's own.
fn lit_colour(base: vec3<f32>, light: vec3<f32>, seconds: f32) -> vec3<f32> {
    let white = setting(GLINT_WHITE, seconds);
    if white <= 0.0 { return base * light; }
    return base * (light - glint) + glint * mix(base, vec3<f32>(1.0), white);
}
// ---- shading one another: how many grains are where, counted in a grid over the
// camera's view (cells across and down the frame, and in depth spaced evenly in the log
// of the distance, from a quarter to four times the distance the camera is focused at,
// or of the frame when it has no focus, so a slice there is about as deep as a cell is
// wide), in w of the prepare pass's buffer after the sprite ----
const GRID_RANGE: f32 = 16.0;
fn grid_near() -> f32 {
    let base = tiles() + ROWS * COUNT;
    var centre = third[base + 14u].w;
    if centre <= 0.0 { centre = third[base + 12u].w; }
    return max(centre, 1.0) / 4.0;
}
fn self_shadowing() -> bool { return (u32(p.extra.z) & 64u) != 0u; }
fn grid_origin() -> u32 { return tiles() + sheet_texels(); }
// The grid's cells across, down and in depth: halved until it fits (none in a frame
// too small for it).
fn grid_dims() -> vec3<u32> {
    let pixels = p.size_op.x * p.size_op.y;
    let room = pixels - min(grid_origin(), pixels);
    var d = vec3<u32>(96u, 54u, 192u);
    for (var k = 0u; k < 5u; k++) {
        if d.x * d.y * d.z <= room { return d; }
        d = d / 2u;
    }
    return vec3<u32>(0u);
}
// The cell holding `at`, or -1 outside the grid.
fn grid_cell(at: vec3<f32>, d: vec3<u32>) -> i32 {
    let seen = filmed(at);
    if seen.z <= 0.0 { return -1; }
    let w = f32(p.size_op.x);
    let x = i32(floor(seen.x / w * f32(d.x)));
    let y = i32(floor(seen.y / f32(p.size_op.y) * f32(d.y)));
    let z = i32(floor(log(distance_to(at) / grid_near()) / log(GRID_RANGE) * f32(d.z)));
    if x < 0 || y < 0 || z < 0 || x >= i32(d.x) || y >= i32(d.y) || z >= i32(d.z) { return -1; }
    return (z * i32(d.y) + y) * i32(d.x) + x;
}
// Where `at` lies in the grid, in cells (a cell's middle at its index plus a half), or
// a negative depth when the camera does not see it.
fn grid_place(at: vec3<f32>, d: vec3<u32>) -> vec3<f32> {
    let seen = filmed(at);
    if seen.z <= 0.0 { return vec3<f32>(-1.0e6); }
    return vec3<f32>(seen.x / f32(p.size_op.x) * f32(d.x), seen.y / f32(p.size_op.y) * f32(d.y),
        log(distance_to(at) / grid_near()) / log(GRID_RANGE) * f32(d.z));
}
fn grid_count(x: i32, y: i32, z: i32, d: vec3<u32>) -> f32 {
    if x < 0 || y < 0 || z < 0 || x >= i32(d.x) || y >= i32(d.y) || z >= i32(d.z) { return 0.0; }
    return fourth[grid_origin() + u32((z * i32(d.y) + y) * i32(d.x) + x)].w;
}
// Grains a cubic pixel at `at`: the counts of the eight cells round it, blended by how
// near it is to each, over a cell's size in the world there, so the shade changes
// smoothly from cell to cell.
fn grains_at(at: vec3<f32>, d: vec3<u32>) -> f32 {
    let g = grid_place(at, d) - vec3<f32>(0.5);
    if g.z < -1.0 { return 0.0; }
    let c0 = vec3<i32>(floor(g));
    let f = g - floor(g);
    var count = 0.0;
    for (var k = 0u; k < 8u; k++) {
        let o = vec3<i32>(i32(k & 1u), i32((k >> 1u) & 1u), i32((k >> 2u) & 1u));
        let w = mix(1.0 - f, f, vec3<f32>(o));
        count += w.x * w.y * w.z * grid_count(c0.x + o.x, c0.y + o.y, c0.z + o.z, d);
    }
    if count <= 0.0 { return 0.0; }
    let away = distance_to(at);
    let focal = third[tiles() + ROWS * COUNT + 12u].w;
    let across = f32(p.size_op.x) / f32(d.x) * away / focal;
    let down = f32(p.size_op.y) / f32(d.y) * away / focal;
    let deep = away * (pow(GRID_RANGE, 1.0 / f32(d.z)) - 1.0);
    return count / max(across * down * deep, 1e-6);
}
// How much of a light reaches `at` from `toward` (at most `reach` away) past the grains
// in between, each blocking its own size times Self shadow.
fn transmitted(at: vec3<f32>, toward: vec3<f32>, reach: f32, seconds: f32) -> f32 {
    let strength = setting(SELF_SHADOW, seconds);
    if strength <= 0.0 || !self_shadowing() { return 1.0; }
    let d = grid_dims();
    if d.x == 0u { return 1.0; }
    let away = distance_to(at);
    let focal = third[tiles() + ROWS * COUNT + 12u].w;
    if away <= 0.0 || focal <= 0.0 { return 1.0; }
    // Steps of a cell and a half, 24 of them, from a step and a half out: the grain's own
    // neighbourhood does not count, or a grain on a surface facing the light would be
    // shaded by its neighbours on that surface.
    let step = 1.5 * f32(p.size_op.x) / f32(d.x) * away / focal;
    let size = setting(SIZE, seconds);
    let blocking = 3.14159265 * size * size * strength;
    // Each grain starts its march at its own place within the first step, so the grid's
    // cells show as fine noise among the grains rather than as bands.
    let jitter = f32(hash(bitcast<u32>(at.x) ^ hash(bitcast<u32>(at.y) ^ hash(bitcast<u32>(at.z)))) >> 8u) / 16777216.0;
    var depth = 0.0;
    for (var k = 1u; k <= 24u; k++) {
        let s = (f32(k) + 0.5 - jitter) * step;
        if s > reach { break; }
        depth += grains_at(at + toward * s, d) * step;
    }
    return exp(-blocking * depth);
}
fn has_curve(bit: u32, seconds: f32) -> bool { return (u32(setting(CURVES, seconds)) & bit) != 0u; }
// A curve's value at `share` of the life: its eight values joined by straight lines.
fn life_curve(base: u32, seconds: f32, share: f32) -> f32 {
    let x = clamp(share, 0.0, 1.0) * 7.0;
    let a = u32(floor(x)); let b = min(a + 1u, 7u);
    return mix(setting(base + a, seconds), setting(base + b, seconds), x - floor(x));
}
fn life_colour(seconds: f32, share: f32) -> vec4<f32> {
    let x = clamp(share, 0.0, 1.0) * 7.0;
    let a = u32(floor(x)); let b = min(a + 1u, 7u);
    let ca = vec4<f32>(setting(COLOUR_CURVE + 4u * a, seconds), setting(COLOUR_CURVE + 4u * a + 1u, seconds), setting(COLOUR_CURVE + 4u * a + 2u, seconds), setting(COLOUR_CURVE + 4u * a + 3u, seconds));
    let cb = vec4<f32>(setting(COLOUR_CURVE + 4u * b, seconds), setting(COLOUR_CURVE + 4u * b + 1u, seconds), setting(COLOUR_CURVE + 4u * b + 2u, seconds), setting(COLOUR_CURVE + 4u * b + 3u, seconds));
    return mix(ca, cb, x - floor(x));
}
// The camera row's values beyond the camera: particles (threads before the bursts'),
// burst particles each, and whether bursts are set off by the obstacle (else by death).
const PARENTS = 32u; const EACH = 33u; const ON_IMPACT = 34u;
// How many particles are joined by lines (0: no lines).
const LINKED = 35u;
fn camera_row(k: u32) -> f32 { return third[tiles() + ROWS * COUNT + k].w; }
// The surface particles are born on (see `surface` in nodes/particles.rs), after the
// camera row.
fn surface_at(k: u32) -> f32 { return third[tiles() + COUNT * (ROWS + 1u) + k].w; }
// Whether particles gather onto a 3D object, and where its surface's table starts.
fn gathering_3d() -> bool { return camera_row(121u) > 0.5; }
// Whether particles are steered to a place: on a target picture or a 3D object.
fn gathering() -> bool { return p.extra.y > 0.5 || gathering_3d(); }
// A place on that surface, picked by area, and its outward direction, where the
// surface was at `birth`: xyz and the direction in the second value.
struct OnSurface { at: vec3<f32>, out: vec3<f32> }
fn on_surface(thread: u32, generation: u32, birth: f32) -> OnSurface {
    return on_surface_in(0u, thread, generation, birth, vec3<u32>(2u, 3u, 18u));
}
// A place on the surface whose table starts at `base`, picked by area with the draws
// `salts`, where the surface was at `birth`.
fn on_surface_in(base: u32, thread: u32, generation: u32, birth: f32, salts: vec3<u32>) -> OnSurface {
    let triangles = u32(surface_at(base)); let objects = u32(surface_at(base + 1u));
    let first = base + 2u + objects * ROWS * 12u;
    let total = surface_at(first + (triangles - 1u) * 11u + 10u);
    let wanted = random(thread, generation, salts.x) * total;
    var low = 0u; var high = triangles - 1u;
    for (var k = 0u; k < 18u; k++) {
        if low >= high { break; }
        let middle = (low + high) / 2u;
        if surface_at(first + middle * 11u + 10u) < wanted { low = middle + 1u; } else { high = middle; }
    }
    let t = first + low * 11u;
    let object = u32(surface_at(t));
    let corner = vec3<f32>(surface_at(t + 1u), surface_at(t + 2u), surface_at(t + 3u));
    let e1 = vec3<f32>(surface_at(t + 4u), surface_at(t + 5u), surface_at(t + 6u));
    let e2 = vec3<f32>(surface_at(t + 7u), surface_at(t + 8u), surface_at(t + 9u));
    var r1 = random(thread, generation, salts.y); var r2 = random(thread, generation, salts.z);
    if r1 + r2 > 1.0 { r1 = 1.0 - r1; r2 = 1.0 - r2; }
    let local = corner + e1 * r1 + e2 * r2;
    let normal = normalize(cross(e1, e2));
    // The object's place at the birth, between the two kept moments around it.
    let life = p.extra.w;
    let row = clamp((birth - (bitcast<f32>(p.size_op.z) - life)) / life, 0.0, 1.0) * f32(ROWS - 1u);
    let a = u32(floor(row)); let b = min(a + 1u, ROWS - 1u); let f = row - floor(row);
    let ma = base + 2u + (object * ROWS + a) * 12u; let mb = base + 2u + (object * ROWS + b) * 12u;
    var m: array<vec4<f32>, 3>;
    for (var r = 0u; r < 3u; r++) {
        m[r] = mix(vec4<f32>(surface_at(ma + 4u * r), surface_at(ma + 4u * r + 1u), surface_at(ma + 4u * r + 2u), surface_at(ma + 4u * r + 3u)),
                   vec4<f32>(surface_at(mb + 4u * r), surface_at(mb + 4u * r + 1u), surface_at(mb + 4u * r + 2u), surface_at(mb + 4u * r + 3u)), f);
    }
    var s: OnSurface;
    s.at = vec3<f32>(dot(m[0], vec4<f32>(local, 1.0)), dot(m[1], vec4<f32>(local, 1.0)), dot(m[2], vec4<f32>(local, 1.0)));
    s.out = normalize(vec3<f32>(dot(m[0].xyz, normal), dot(m[1].xyz, normal), dot(m[2].xyz, normal)));
    return s;
}
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

// ---- prepare: source = target, second = sprite, third = the scene's depth ----
// y holds what hides particles at each pixel: the distance of the 3D object there
// (0: nothing). The sprite's small copy needs all four values of its texels, so
// those pixels (about two rows at the top of the frame) carry no depth.
@compute @workgroup_size(8, 8)
fn particle_prepare(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    let count = tiles();
    // Without a depth picture to copy (flag 8: depth beside a target; without a target
    // the splat reads the depth itself, flag 16) or shadow maps to take in (flag 32) only
    // the tile table and the sprite at the start are ever read: the frame beyond them is
    // left alone.
    let shadowed = (u32(p.extra.z) & 32u) != 0u;
    if (u32(p.extra.z) & 8u) == 0u && !shadowed && i >= count + sheet_texels() { return; }
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
    } else if i < count + sheet_texels() && (u32(p.extra.z) & 4u) != 0u {
        // The sprite's frame averaged down to MIP x MIP a cell of its sheet.
        let cell = i - count;
        let sw = sheet_width(); let sh = MIP * sheet_rows();
        let x0 = (cell % sw) * p.size_op.x / sw; let x1 = max(((cell % sw) + 1u) * p.size_op.x / sw, x0 + 1u);
        let y0 = (cell / sw) * p.size_op.y / sh; let y1 = max(((cell / sw) + 1u) * p.size_op.y / sh, y0 + 1u);
        var sum = vec4<f32>(0.0);
        for (var y = y0; y < min(y1, p.size_op.y); y++) {
            for (var x = x0; x < min(x1, p.size_op.x); x++) { sum += second[y * p.size_op.x + x]; }
        }
        out = sum / f32((x1 - x0) * (y1 - y0));
    }
    let holds_sprite = i >= count && i < count + sheet_texels() && (u32(p.extra.z) & 4u) != 0u;
    if (u32(p.extra.z) & 8u) != 0u && !holds_sprite {
        let there = third[i];
        if there.a > 0.5 { out.y = max(there.r, 0.0); }
    }
    // The scene's shadow maps (fourth: the 3D renderer's views, then its maps from byte
    // 1024), one value a pixel in z from just after the sprite: the views' 128 values,
    // then the maps.
    let at_shadows = count + sheet_texels();
    if shadowed && i >= at_shadows {
        let k = i - at_shadows;
        var f = k;
        if k >= 128u { f = 256u + (k - 128u); }
        if f / 4u < p.size_op.x * p.size_op.y { out.z = fourth[f / 4u][f % 4u]; }
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
    let count = tiles();
    // The settings rows, the camera row and the surface particles are born on, which the
    // camera row says the length of.
    let at_length = ROWS * COUNT + 112u;
    let carried = COUNT * (ROWS + 1u) + u32(fourth[at_length / 3u][at_length % 3u]);
    // Without a field or an obstacle only the table at the start is ever read (splat
    // and resolve read the rest only for those): the frame beyond it is left alone.
    if (flags & 3u) == 0u && i >= count + carried { return; }
    var out = vec4<f32>(0.0);
    if (flags & 1u) != 0u {
        // The slope of the field's brightness, over a few pixels so it is smooth.
        let x = i32(gid.x); let y = i32(gid.y);
        out.x = (field_value(x + 3, y) - field_value(x - 3, y)) / 6.0;
        out.y = (field_value(x, y + 3) - field_value(x, y - 3)) / 6.0;
    }
    if (flags & 2u) != 0u { out.z = clamp(second[i].a, 0.0, 1.0); }
    if i < count {
        // Cumulative alpha up to and including tile i. Without a target every tile's
        // alpha is nought, so the sum is too: no need to add up thousands of them.
        var sum = 0.0;
        if p.extra.y > 0.5 {
            for (var k = 0u; k <= i; k++) { sum += third[k].x; }
        }
        out.w = sum;
    } else if i < count + carried {
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
// Keyed forces sampled finely over the last life (see force_table in nodes/particles.rs):
// where the table starts after the camera row (camera row 119) and its rows (120; 0 none).
fn force_rows() -> u32 { return u32(camera_row(120u)); }
fn forces_at(seconds: f32) -> Forces {
    var f: Forces;
    let rows = force_rows();
    if rows > 1u {
        let life = p.extra.w;
        let x = clamp((seconds - (bitcast<f32>(p.size_op.z) - life)) / life, 0.0, 1.0) * f32(rows - 1u);
        let a = u32(floor(x)); let b = min(a + 1u, rows - 1u); let w = x - floor(x);
        let base = tiles() + COUNT * (ROWS + 1u) + u32(camera_row(119u));
        var v: array<f32, 11>;
        for (var j = 0u; j < 11u; j++) {
            v[j] = mix(third[base + a * 11u + j].w, third[base + b * 11u + j].w, w);
        }
        f.pull = vec2<f32>(v[1], v[0]);
        f.turbulence = v[2]; f.scale = v[3]; f.push = v[4]; f.flow = v[5]; f.bounce = v[6];
        f.centre = vec2<f32>(v[7], v[8]); f.attract = v[9]; f.vortex = v[10];
        return f;
    }
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
// The age at which the last place() first touched the obstacle (-1: it did not).
var<private> impact: f32;
// With `recording` set, place() keeps the path it walks: where the particle was after
// each step (xyz) and at what age (w), from its birth on.
var<private> recording: bool;
var<private> path: array<vec4<f32>, 100>;
var<private> path_count: u32;
// A flock's simulated place at the frame (w 1), which place() gives in place of a path.
var<private> held: vec4<f32>;
// The part of a particle's life before it reaches its place on the target: its life
// less the hold it then spends there.
fn flight(own_life: f32, birth: f32) -> f32 {
    return max(own_life - setting(HOLD, birth), own_life * 0.05);
}
fn place(origin: vec3<f32>, launch: vec3<f32>, birth: f32, age: f32, own_life: f32, goal: vec3<f32>, homing: f32, deep: bool) -> vec3<f32> {
    if held.w > 0.5 { impact = -1.0; path_count = 0u; return held.xyz; }
    var at = origin; var velocity = launch;
    impact = -1.0;
    path_count = 0u;
    if recording { path[0] = vec4<f32>(origin, 0.0); path_count = 1u; }
    let flags = u32(p.extra.z);
    var steps = 32u;
    // Keyed forces are followed closely: more steps, and the forces read at each.
    let keyed_forces = force_rows() > 1u;
    if keyed_forces { steps = 128u; }
    let solids = solid_count() > 0u;
    let forced = force_count() > 0u;
    if (flags & 2u) != 0u || solids { steps = max(steps, 96u); }
    let step = p.extra.w / f32(steps);
    var done = 0.0;
    var f: Forces;
    let paced = has_curve(4u, birth);
    for (var k = 0u; k < steps; k++) {
        let dt = min(step, age - done);
        if dt <= 0.0 { break; }
        // Forces follow their keys, re-read a few times over the life.
        if k % 8u == 0u || keyed_forces {
            f = forces_at(birth + done);
            if forced { forces_now(birth + done); }
        }
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
        var stir = vec3<f32>(0.0);
        if forced {
            let pushed = forces_push(at, velocity, birth + done, dt);
            velocity = pushed.velocity; stir = pushed.stir;
        }
        var drift = velocity + stir;
        if f.turbulence > 0.0 {
            // In depth the swirl also carries particles towards and away from the viewer.
            var through = 0.0;
            if deep { through = (value_noise(vec3<f32>(at.xy / f.scale * 1.3 + vec2<f32>(91.0, 7.0), (birth + done) * 0.35 + 13.0)) - 0.5) * 2.0 * f.turbulence; }
            drift += vec3<f32>(swirl(at.xy, birth + done, f), through);
        }
        var next = at + drift * dt;
        // Speed over the life: how far it travels in this step, at the middle of the step.
        if paced { next = at + drift * dt * life_curve(SPEED_CURVE, birth, (done + dt * 0.5) / own_life); }
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
        if hit && impact < 0.0 { impact = done; }
        if hit {
            // Bounce off the obstacle's edge: reflect about the way out of it.
            var out = vec2<f32>(solid(next.xy - vec2<f32>(2.0, 0.0)) - solid(next.xy + vec2<f32>(2.0, 0.0)),
                                solid(next.xy - vec2<f32>(0.0, 2.0)) - solid(next.xy + vec2<f32>(0.0, 2.0)));
            if dot(out, out) < 1e-6 { out = -drift.xy; }
            out = normalize(out);
            velocity = vec3<f32>(velocity.xy - 2.0 * min(dot(velocity.xy, out), 0.0) * out, velocity.z) * f.bounce;
            next = at;
        }
        // Bounce off the scene's solids: met within a pixel along the move, or already
        // inside one that moved onto it, it is set on the surface and turned back.
        if solids && !hit {
            let t = birth + done + dt;
            let met = solid_hit(at, next, t);
            if met.x >= 0.0 {
                if impact < 0.0 { impact = done; }
                let i = u32(met.y);
                var on = mix(at, next, met.x);
                let out = solid_out(i, on, t);
                on += out * max(1.0 - solid_distance(i, on, t), 0.0);
                // Back off it with bounce of the speed it hit it at; along it, it slides on,
                // for the rest of the step.
                velocity -= (1.0 + f.bounce) * min(dot(velocity, out), 0.0) * out;
                at = on;
                next = on + velocity * dt * (1.0 - met.x);
            }
        }
        at = next;
        done += dt;
        if recording && path_count < 100u { path[path_count] = vec4<f32>(at, done); path_count += 1u; }
    }
    if gathering() && homing > 0.0 {
        at = mix(at, goal, smoothstep(1.0 - homing, 1.0, age / flight(own_life, birth)));
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
// The same through the camera as it was when the exposure began.
fn filmed_before(at: vec3<f32>) -> vec3<f32> {
    let base = tiles() + ROWS * COUNT + 18u;
    let v = vec3<f32>(
        dot(vec4<f32>(third[base].w, third[base + 1u].w, third[base + 2u].w, third[base + 3u].w), vec4<f32>(at, 1.0)),
        dot(vec4<f32>(third[base + 4u].w, third[base + 5u].w, third[base + 6u].w, third[base + 7u].w), vec4<f32>(at, 1.0)),
        dot(vec4<f32>(third[base + 8u].w, third[base + 9u].w, third[base + 10u].w, third[base + 11u].w), vec4<f32>(at, 1.0)));
    let focal = third[base + 12u].w;
    let away = -v.z;
    if away <= 1.0 { return vec3<f32>(0.0); }
    return vec3<f32>(vec2<f32>(p.size_op.xy) * 0.5 + focal * v.xy / away, focal / away);
}
// How far in front of the camera a point of the scene is.
fn distance_to(at: vec3<f32>) -> f32 {
    let base = tiles() + ROWS * COUNT;
    return -dot(vec4<f32>(third[base + 8u].w, third[base + 9u].w, third[base + 10u].w, third[base + 11u].w), vec4<f32>(at, 1.0));
}
// The distance of the 3D object at a pixel (0: none, or no depth connected).
fn occluder(at: vec2<f32>) -> f32 {
    let flags = u32(p.extra.z);
    if (flags & 24u) == 0u { return 0.0; }
    let q = vec2<i32>(floor(at));
    if q.x < 0 || q.y < 0 || q.x >= i32(p.size_op.x) || q.y >= i32(p.size_op.y) { return 0.0; }
    let i = u32(q.y) * p.size_op.x + u32(q.x);
    // Without a target the depth picture is bound where the target would be, and read
    // as the prepare pass copies it: the distance where an object is, else nought.
    if (flags & 16u) != 0u {
        let there = second[i];
        return select(0.0, max(there.r, 0.0), there.a > 0.5);
    }
    return fourth[i].y;
}
// The lens blur in pixels of something `away` from the scene's camera.
fn lens_blur(away: f32) -> f32 {
    let base = tiles() + ROWS * COUNT;
    let optics = third[base + 13u].w;
    if optics <= 0.0 { return 0.0; }
    var focus = third[base + 14u].w;
    // Autofocus: whatever the depth picture holds at the frame centre.
    if focus <= 0.0 { focus = occluder(vec2<f32>(p.size_op.xy) * 0.5); }
    if focus <= 0.0 { return 0.0; }
    let k = optics / max(focus - third[base + 15u].w, 1.0);
    return min(k * abs(1.0 - focus / max(away, 1.0)), third[base + 16u].w);
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
// A soft dot of `light` at `at`, added into the output. A dot smaller than a pixel is
// drawn a pixel wide, with its light cut to its own size, so a crowd of tiny particles
// is as bright in a small frame as it is in a large one.
fn stamp(at: vec2<f32>, radius: f32, given: vec4<f32>) {
    let sigma = max(radius * 0.5, 0.45);
    let light = given * min(radius * radius * 0.25 / (sigma * sigma), 1.0);
    let reach = i32(ceil(max(radius, 1.0)));
    let centre = vec2<i32>(floor(at));
    for (var j = -reach; j <= reach; j++) {
        for (var i = -reach; i <= reach; i++) {
            let pixel = centre + vec2<i32>(i, j);
            let d = vec2<f32>(pixel) + vec2<f32>(0.5) - at;
            var soft = exp(-dot(d, d) / (2.0 * sigma * sigma));
            // A large dot reaches zero at its rim, all the way round: cut off at the
            // square it is drawn in, it showed that square's corners.
            let rim = exp(-f32(reach * reach) / (2.0 * sigma * sigma));
            soft = mix(soft, max(soft - rim, 0.0) / (1.0 - rim), smoothstep(2.0, 6.0, radius));
            add_light(pixel, light * soft);
        }
    }
}
// A grain drawn as a soft dot, or out of focus with Bokeh as a lens's disc.
fn draw_dot(at: vec2<f32>, radius: f32, sharp: f32, light: vec4<f32>, birth: f32) {
    let disc = setting(BOKEH, birth) * smoothstep(1.3, 2.5, radius / max(sharp, 0.25));
    if disc > 0.0 && radius > 1.5 {
        stamp_disc(at, radius, light * disc);
        if disc < 1.0 { stamp(at, radius, light * (1.0 - disc)); }
    } else {
        stamp(at, radius, light);
    }
}
// A lens's image of an out-of-focus point: a flat disc of `radius` with a crisp edge and
// a slightly brighter rim, carrying the same light as a dot of that radius.
fn stamp_disc(at: vec2<f32>, radius: f32, light: vec4<f32>) {
    let reach = i32(ceil(radius)) + 1;
    let centre = vec2<i32>(floor(at));
    // The light a dot of this radius carries, as a share of pi r^2: a half, falling to a
    // third as the dot is made to reach zero at its rim (see stamp); the disc with its
    // rim carries about 1.15 pi r^2.
    let each = light * (mix(0.5, 0.343, smoothstep(2.0, 6.0, radius)) / 1.15);
    for (var j = -reach; j <= reach; j++) {
        for (var i = -reach; i <= reach; i++) {
            let pixel = centre + vec2<i32>(i, j);
            let d = length(vec2<f32>(pixel) + vec2<f32>(0.5) - at);
            let inside = clamp(radius + 0.5 - d, 0.0, 1.0);
            if inside <= 0.0 { continue; }
            let rim = 1.0 + 0.45 * smoothstep(radius * 0.6, radius, d);
            add_light(pixel, each * inside * rim);
        }
    }
}
// A texel of cell `cell` of the sprite sheet (clear beyond the cell).
fn sprite_texel(cell: u32, x: i32, y: i32) -> vec4<f32> {
    if x < 0 || y < 0 || x >= i32(MIP) || y >= i32(MIP) { return vec4<f32>(0.0); }
    let cx = cell % sheet_columns(); let cy = cell / sheet_columns();
    return fourth[tiles() + (cy * MIP + u32(y)) * sheet_width() + cx * MIP + u32(x)];
}
// Which cell of the sprite sheet a particle shows `age` seconds into its life.
fn sprite_cell(thread: u32, generation: u32, age: f32, own_life: f32, birth: f32) -> u32 {
    let cells = sheet_columns() * sheet_rows();
    if cells == 1u { return 0u; }
    let first = min(u32(random(thread, generation, 25u) * f32(cells)), cells - 1u);
    switch (u32(p.extra.z) >> 16u) & 3u {
        case 1u: { return min(u32(age / own_life * f32(cells)), cells - 1u); }
        case 2u: { return (first + u32(max(floor(age * setting(SPRITE_RATE, birth)), 0.0))) % cells; }
        default: { return first; }
    }
}
// The sprite image (its cell `cell`), `radius` wide either side of `at` and turned by
// `angle`, tinted.
fn stamp_sprite(at: vec2<f32>, radius: f32, angle: f32, tint: vec4<f32>, cell: u32) {
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
            let texel = mix(mix(sprite_texel(cell, base.x, base.y), sprite_texel(cell, base.x + 1, base.y), f.x),
                            mix(sprite_texel(cell, base.x, base.y + 1), sprite_texel(cell, base.x + 1, base.y + 1), f.x), f.y);
            add_light(pixel, vec4<f32>(texel.rgb * tint.rgb, texel.a) * tint.a);
        }
    }
}
// A particle as born: where and how it was launched, from its thread and a turn
// `back` lives before the one at `now`. `ok` is false when it was never born.
struct Born {
    ok: bool, generation: u32, birth: f32, own_life: f32, origin: vec3<f32>, launch: vec3<f32>,
    goal: vec3<f32>, homing: f32, deep: bool, born_on: vec3<f32>, lands_on: vec3<f32>,
    // The outward direction of the surface it was born on (zero when not born on one).
    surface: vec3<f32>,
    // The outward direction of the 3D object's surface where it lands (zero when none).
    landing: vec3<f32>,
}
// The share of the life a thread's birth may fall in: the burst's, for a flock born in
// a burst shorter than its life (see `threads` in nodes/particles.rs), else all of it.
fn burst_window(life: f32, duration: f32) -> f32 {
    if camera_row(118u) > 0.5 && duration > 0.0 && duration < life { return duration / life; }
    return 1.0;
}
fn born(thread: u32, back: u32, now: f32) -> Born {
    var b: Born; b.ok = false;
    let size = vec2<f32>(p.size_op.xy);
    let life = p.extra.w;
    let start = setting(START, now); let duration = setting(DURATION, now);
    // Each thread emits one particle after another, a life apart, from its own phase.
    // (Worked out as it always was for the life at `now`, so frames stay the same.)
    // A flock born in a burst shorter than its life holds one bird a thread, so each
    // thread's one birth falls inside the burst rather than anywhere in the life, or
    // most of the threads (and the flock's room) would go unused.
    let window = burst_window(life, duration);
    var phase = random(thread, 0u, 0u);
    if window < 1.0 { phase = 1.0 - phase * window; }
    let turn = (now - start) / life + phase;
    b.generation = u32(i32(floor(turn)) + 1048576 - i32(back));
    let generation = b.generation;
    let age = fract(turn) * life + f32(back) * life;
    b.birth = now - age;
    let birth = b.birth;
    if birth < start || (duration > 0.0 && birth >= start + duration) { return b; }
    let variation = setting(VARIATION, birth);
    b.own_life = life * (1.0 - 0.6 * variation * random(thread, generation, 1u));
    var origin: vec3<f32>;
    var born_on = vec3<f32>(1.0);
    var surface_out = vec3<f32>(0.0);
    if p.extra.x > 0.5 {
        // One cell of the frame per group of threads; born where the emitter has alpha.
        let stride = u32(p.values.z); let slots = u32(p.values.w);
        let across = (p.size_op.x + stride - 1u) / stride;
        let cell = thread / slots;
        origin = vec3<f32>((vec2<f32>(f32(cell % across), f32(cell / across))
            + vec2<f32>(random(thread, generation, 2u), random(thread, generation, 3u))) * f32(stride), 0.0);
        if origin.x >= size.x || origin.y >= size.y { return b; }
        // The emitter as it is, or as it was nearest the birth: a tile of its history.
        var texel = vec2<u32>(u32(origin.x), u32(origin.y));
        let grid = u32(p.extra.x);
        if grid > 1u {
            let tile = min(u32(round(min(age / life, 1.0) * f32(grid * grid - 1u))), grid * grid - 1u);
            let across = p.size_op.x / grid; let down = p.size_op.y / grid;
            texel = vec2<u32>((tile % grid) * across + min(texel.x / grid, across - 1u),
                              (tile / grid) * down + min(texel.y / grid, down - 1u));
        }
        let at_birth = bitcast<vec4<f32>>(first[texel.y * p.size_op.x + texel.x]);
        let chance = setting(DENSITY, birth) * life / 10000.0 * f32(stride * stride) / f32(slots);
        if random(thread, generation, 4u) >= clamp(at_birth.a, 0.0, 1.0) * chance { return b; }
        born_on = unpremultiplied(at_birth);
    } else if p.extra.x < -0.5 {
        // A flock born on its emitter (bound fourth in its simulation, as the emitter was
        // at the flock's start): each bird tries places in the box (the whole frame when
        // the box is empty) and is born at the first its alpha takes, in its colour.
        if random(thread, generation, 4u) >= setting(RATE, birth) * life * window / camera_row(PARENTS) { return b; }
        var box_size = vec2<f32>(setting(BOX_W, birth), setting(BOX_H, birth));
        var box_middle = vec2<f32>(setting(BOX_X, birth), setting(BOX_Y, birth));
        if box_size.x <= 0.0 || box_size.y <= 0.0 { box_size = size; box_middle = size * 0.5; }
        var found = false;
        for (var t = 0u; t < 64u; t++) {
            let q = box_middle + (vec2<f32>(random(thread, generation, 200u + 3u * t), random(thread, generation, 201u + 3u * t)) - 0.5) * box_size;
            if q.x < 0.0 || q.y < 0.0 || q.x >= size.x || q.y >= size.y { continue; }
            let there = fourth[u32(q.y) * p.size_op.x + u32(q.x)];
            if random(thread, generation, 202u + 3u * t) < clamp(there.a, 0.0, 1.0) {
                origin = vec3<f32>(q, 0.0);
                born_on = unpremultiplied(there);
                found = true;
                break;
            }
        }
        if !found { return b; }
    } else if camera_row(113u) > 0.5 {
        // On the surface of a 3D object, as it was at the birth.
        if random(thread, generation, 4u) >= setting(RATE, birth) * life * window / camera_row(PARENTS) { return b; }
        let spot = on_surface(thread, generation, birth);
        origin = spot.at;
        surface_out = spot.out;
    } else {
        if random(thread, generation, 4u) >= setting(RATE, birth) * life * window / camera_row(PARENTS) { return b; }
        origin = vec3<f32>(vec2<f32>(setting(BOX_X, birth), setting(BOX_Y, birth))
            + (vec2<f32>(random(thread, generation, 2u), random(thread, generation, 3u)) - 0.5)
            * vec2<f32>(setting(BOX_W, birth), setting(BOX_H, birth)), 0.0);
    }
    // Depth spreads births in front of and behind the frame.
    let depth = setting(DEPTH, birth);
    let towards = setting(TOWARDS, birth);
    let from_surface = length(surface_out) > 0.5;
    b.deep = depth > 0.0 || setting(BOX_Z, birth) != 0.0 || towards != 0.0 || from_surface;
    if !from_surface { origin.z = setting(BOX_Z, birth) + (random(thread, generation, 13u) - 0.5) * depth; }
    // A place on the target, drawn in proportion to its alpha: first the tile, by the
    // cumulative table, then the pixel in it. No particle goes without a place.
    var goal = origin; var lands_on = born_on;
    b.homing = setting(HOMING, birth);
    if p.extra.y > 0.5 {
        let count = tiles();
        let total = third[count - 1u].w;
        if total <= 0.0 { return b; }
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
        if !found { return b; }
        if p.extra.x < 0.5 { born_on = lands_on; }
    }
    if gathering_3d() {
        // A place on the 3D object, where the object is now.
        let place = on_surface_in(u32(camera_row(122u)), thread, generation, now, vec3<u32>(60u, 61u, 62u));
        goal = place.at;
        b.landing = place.out;
        b.deep = true;
    }
    let heading = radians(setting(DIRECTION, birth)) + (random(thread, generation, 5u) - 0.5) * radians(setting(SPREAD, birth));
    // In depth the spread is a cone: the launch also leans towards or away from the viewer.
    var lean = 0.0;
    if b.deep { lean = (random(thread, generation, 14u) - 0.5) * radians(setting(SPREAD, birth)); }
    b.launch = vec3<f32>(cos(heading) * cos(lean), -sin(heading) * cos(lean), sin(lean))
        * setting(SPEED, birth) * (1.0 - 0.7 * variation * random(thread, generation, 6u))
        + vec3<f32>(0.0, 0.0, towards);
    if from_surface {
        // Straight out of the surface, within a cone of the spread round its direction.
        let n = surface_out;
        var side = cross(n, vec3<f32>(0.0, 0.0, 1.0));
        if dot(side, side) < 1e-6 { side = cross(n, vec3<f32>(0.0, 1.0, 0.0)); }
        side = normalize(side);
        let up = cross(n, side);
        let half = radians(setting(SPREAD, birth)) * 0.5;
        let c = 1.0 - random(thread, generation, 5u) * (1.0 - cos(half));
        let turn = random(thread, generation, 14u) * 6.2831853;
        let dir = n * c + (side * cos(turn) + up * sin(turn)) * sqrt(max(1.0 - c * c, 0.0));
        b.launch = dir * setting(SPEED, birth) * (1.0 - 0.7 * variation * random(thread, generation, 6u));
    }
    b.origin = origin; b.goal = goal; b.born_on = born_on; b.lands_on = lands_on;
    b.surface = surface_out;
    b.ok = true;
    return b;
}
// How one particle looks: its colours over its life (before the emitter's tint and the
// hue turn), its size at birth, and the random draws its own.
// `ends` says it is not drawn once it has touched the obstacle (it burst there).
struct Look { first: vec4<f32>, last: vec4<f32>, tint: vec3<f32>, size: f32, thread: u32, generation: u32, hue: f32, ramp: bool, ends: bool, surface: vec3<f32>, landing: vec3<f32> }
// Draw a particle `age` seconds into its life on its path, as a dot, a sprite or a streak.
fn draw(origin: vec3<f32>, launch: vec3<f32>, birth: f32, age: f32, own_life: f32, goal: vec3<f32>, homing: f32, deep: bool, look: Look) {
    let thread = look.thread; let generation = look.generation;
    let variation = setting(VARIATION, birth);
    let ribbon = setting(TRAIL, birth) > 0.0 && (u32(p.extra.z) & 4u) == 0u && !simulated();
    recording = ribbon;
    let where_now = place(origin, launch, birth, age, own_life, goal, homing, deep);
    recording = false;
    // Set off by the obstacle, a particle ends where it first touches it.
    if look.ends && impact >= 0.0 { return; }
    let seen = filmed(where_now);
    if seen.z <= 0.0 { return; }
    if occluding {
        // Counted into the grid, with its clump's grains, instead of drawn.
        let d = grid_dims();
        if d.x == 0u { return; }
        let cell = grid_cell(where_now, d);
        if cell >= 0 {
            atomicAdd(&result[(grid_origin() + u32(cell)) * 4u + 3u], u32(clamp(round(setting(CLUMP, birth)), 1.0, 16.0)));
        }
        return;
    }
    let at = seen.xy;
    // A 3D object in front of the particle hides it.
    let away = distance_to(where_now);
    let wall = occluder(at);
    if wall > 0.0 && away > wall * 1.002 + 1.0 { return; }
    // Beyond its reach a particle is not drawn; it fades out over the last quarter.
    let reach = setting(REACH, birth);
    if reach > 0.0 && away >= reach { return; }
    let share = age / own_life;
    let fade = setting(FADE, birth);
    var opacity = min(share / 0.04, 1.0);
    if reach > 0.0 { opacity *= clamp((reach - away) / (0.25 * reach), 0.0, 1.0); }
    if has_curve(2u, birth) {
        // Opacity over the life in place of the fade.
        opacity = life_curve(OPACITY_CURVE, birth, share);
        if reach > 0.0 { opacity *= clamp((reach - away) / (0.25 * reach), 0.0, 1.0); }
    } else if fade > 0.0 { opacity *= clamp((1.0 - share) / fade, 0.0, 1.0); }
    var colour = mix(look.first, look.last, share);
    if look.ramp && has_curve(8u, birth) { colour = life_colour(birth, share); }
    let unlit = colour;
    let shaded = surface_light(look.surface, origin, look.landing, goal, where_now, birth);
    colour = vec4<f32>(lit_colour(colour.rgb, lighting(where_now, birth, facet(look.thread, look.generation, age, birth), shaded.xyz, shaded.w), birth), colour.a);
    colour = vec4<f32>(colour.rgb * look.tint, colour.a);
    let hues = setting(HUES, birth);
    if hues > 0.0 {
        colour = vec4<f32>(turned(colour.rgb, (look.hue - 0.5) * 6.2831853 * hues), colour.a);
    }
    // Near particles are drawn larger, up to a limit that keeps one particle affordable
    // (48 pixels at 1920 wide, more in a larger frame so a film looks the same at any
    // size); points (the camera row's 31st value) are the same size at any distance.
    let largest = 48.0 * max(f32(p.size_op.x) / 1920.0, 1.0);
    let nearness = mix(seen.z, 1.0, camera_row(31u));
    var growth = mix(1.0, setting(GROW, birth), share);
    if has_curve(1u, birth) { growth = life_curve(SIZE_CURVE, birth, share); }
    let sharp = min(look.size * (1.0 - 0.6 * variation * random(thread, generation, 7u))
        * growth * nearness, largest);
    if sharp <= 0.0 { return; }
    // Out of focus it spreads over a larger disc and dims by as much.
    let radius = min(sharp + lens_blur(away), largest);
    colour = vec4<f32>(colour.rgb, colour.a * sharp * sharp / (radius * radius));
    if (u32(p.extra.z) & 4u) != 0u {
        // Spin either way, from a random angle when spinning at all.
        let spin = radians(setting(SPIN, birth));
        var angle = 0.0;
        if spin != 0.0 {
            angle = random(thread, generation, 9u) * 6.2831853
                + spin * age * (1.0 - variation * random(thread, generation, 10u)) * sign(random(thread, generation, 11u) - 0.5);
        }
        let cell = sprite_cell(thread, generation, age, own_life, birth);
        stamp_sprite(at, radius, angle, vec4<f32>(colour.rgb, colour.a * opacity), cell);
        // A streak leaves fading copies back along the path.
        let ghost_trail = min(setting(STREAK, birth), age);
        if ghost_trail > 0.0 {
            for (var g = 1u; g <= 5u; g++) {
                let back = f32(g) / 5.0;
                let was = filmed(place(origin, launch, birth, age - ghost_trail * back, own_life, goal, homing, deep));
                if was.z <= 0.0 { continue; }
                stamp_sprite(was.xy, radius * (1.0 - 0.3 * back), angle, vec4<f32>(colour.rgb, colour.a * opacity * 0.5 * (1.0 - back * 0.85)), cell);
            }
        }
        return;
    }
    let light = vec4<f32>(colour.rgb * colour.a, colour.a) * opacity;
    if ribbon {
        trail_ribbon(at, radius * setting(TRAIL_WIDTH, birth), light, age, min(setting(TRAIL, birth), age), own_life, goal, homing, birth);
        return;
    }
    // A streak is a row of dots back along the path, fading towards the tail.
    var trail = min(setting(STREAK, birth), age);
    // An exposure: the scene camera's own move during it streaks the particle too.
    let exposure = min(camera_row(17u), age);
    trail = max(trail, exposure);
    if trail <= 0.0 {
        draw_dot(at, radius, sharp, light, birth);
        // A clump: more grains round this one, each its own small way off (in depth too),
        // filmed and lit for itself and glinting with a facet of its own, for the cost of
        // drawing them only.
        let clump = u32(clamp(round(setting(CLUMP, birth)), 1.0, 16.0));
        let spread = setting(CLUMP_RADIUS, birth);
        for (var k = 1u; k < 16u; k++) {
            if k >= clump { break; }
            let u = vec3<f32>(random(thread, generation, 70u + 3u * k), random(thread, generation, 71u + 3u * k), random(thread, generation, 72u + 3u * k)) * 2.0 - 1.0;
            let grain = where_now + normalize(u + vec3<f32>(1e-4)) * spread * pow(random(thread, generation, 120u + k), 0.33);
            let seen_k = filmed(grain);
            if seen_k.z <= 0.0 { continue; }
            let wall_k = occluder(seen_k.xy);
            let away_k = distance_to(grain);
            if wall_k > 0.0 && away_k > wall_k * 1.002 + 1.0 { continue; }
            let shaded_k = surface_light(look.surface, origin, look.landing, goal, grain, birth);
            let lit_k = lighting(grain, birth, facet(thread + k * 0x01000193u, generation, age, birth), shaded_k.xyz, shaded_k.w);
            var c = vec4<f32>(lit_colour(unlit.rgb, lit_k, birth) * look.tint, unlit.a);
            if hues > 0.0 { c = vec4<f32>(turned(c.rgb, (look.hue - 0.5) * 6.2831853 * hues), c.a); }
            let sharp_k = min(sharp * mix(1.0, seen_k.z, 1.0 - camera_row(31u)) / max(nearness, 1e-4), largest);
            let radius_k = min(sharp_k + lens_blur(away_k), largest);
            c = vec4<f32>(c.rgb, c.a * sharp_k * sharp_k / max(radius_k * radius_k, 1e-6));
            draw_dot(seen_k.xy, radius_k, sharp_k, vec4<f32>(c.rgb * c.a, c.a) * opacity, birth);
        }
        return;
    }
    let earlier = place(origin, launch, birth, age - trail, own_life, goal, homing, deep);
    var tail = filmed(earlier);
    if exposure > 0.0 { tail = filmed_before(earlier); }
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
// A trail along the path the particle took (kept by place() as it walked), from its head
// at `at` back `trail` seconds: unbroken, narrowing and fading towards the tail.
fn trail_ribbon(at: vec2<f32>, width: f32, light: vec4<f32>, age: f32, trail: f32, own_life: f32, goal: vec3<f32>, homing: f32, birth: f32) {
    if trail <= 0.0 || width <= 0.0 { stamp(at, width, light); return; }
    let spacing = max(width, 1.0) * 0.5;
    // Overlapping dots add up: share the light so the ribbon is as bright as the head.
    let each = light * min(1.0, 1.1 * spacing / max(width, 1.0));
    var head = at;
    var head_age = age;
    var drawn = 0u;
    stamp(at, width, each);
    // From the newest point of the path back, while it is within the trail.
    var k = i32(path_count) - 1;
    loop {
        if k < 0 || drawn >= 640u { break; }
        var point = path[k];
        // The last step may end past the present: it is where the particle is now.
        if point.w >= age { k -= 1; continue; }
        var tail_age = point.w;
        var where_then = point.xyz;
        if gathering() && homing > 0.0 {
            where_then = mix(where_then, goal, smoothstep(1.0 - homing, 1.0, tail_age / flight(own_life, birth)));
        }
        // The trail ends part way along this step.
        if age - tail_age > trail {
            let keep = (trail - (age - head_age)) / max(head_age - tail_age, 1e-6);
            let seen_then = filmed(where_then);
            if seen_then.z <= 0.0 { break; }
            let end = mix(head, seen_then.xy, clamp(keep, 0.0, 1.0));
            drawn += ribbon_segment(head, end, head_age, age - trail, age, trail, width, each);
            break;
        }
        let seen_then = filmed(where_then);
        if seen_then.z <= 0.0 { break; }
        drawn += ribbon_segment(head, seen_then.xy, head_age, tail_age, age, trail, width, each);
        head = seen_then.xy; head_age = tail_age;
        k -= 1;
    }
}
// Dots from `start` to `finish` (ages `start_age` to `finish_age`), narrowing and fading with how far
// back along the trail they are; how many were drawn.
fn ribbon_segment(start: vec2<f32>, finish: vec2<f32>, start_age: f32, finish_age: f32, age: f32, trail: f32, width: f32, each: vec4<f32>) -> u32 {
    let spacing = max(width, 1.0) * 0.5;
    let dots = clamp(u32(ceil(length(finish - start) / spacing)), 1u, 160u);
    for (var d = 1u; d <= dots; d++) {
        let f = f32(d) / f32(dots);
        let back = clamp((age - mix(start_age, finish_age, f)) / trail, 0.0, 1.0);
        stamp(mix(start, finish, f), width * (1.0 - 0.8 * back), each * (1.0 - back));
    }
    return dots;
}
// What a thread draws: a particle on its path, as draw() takes it.
struct Drawn {
    origin: vec3<f32>, launch: vec3<f32>, birth: f32, age: f32, own_life: f32,
    goal: vec3<f32>, homing: f32, deep: bool, look: Look,
}
@compute @workgroup_size(64)
fn particle_splat(@builtin(global_invocation_id) gid: vec3<u32>) {
    splat(gid);
}
// ---- occlusion: as splat, but each particle is counted into the self-shadowing grid ----
@compute @workgroup_size(64)
fn particle_occlusion(@builtin(global_invocation_id) gid: vec3<u32>) {
    occluding = true;
    splat(gid);
}
// ---- carry: first = prepare, second = occlusion; the prepare pass's buffer with the
// grid's counts in w ----
@compute @workgroup_size(8, 8)
fn particle_carry(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    var out = bitcast<vec4<f32>>(first[i]);
    let d = grid_dims();
    let o = grid_origin();
    if i >= o && i < o + d.x * d.y * d.z { out.w = f32(bitcast<u32>(second[i].w)); }
    store(i, out);
}
fn splat(gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let now = bitcast<f32>(p.size_op.z);
    let parents = u32(camera_row(PARENTS));
    let on_impact = camera_row(ON_IMPACT) > 0.5;
    // Up to two particles to draw (a burst particle may follow either of two lives),
    // drawn at one place below: the driver copies what draw() calls into each place
    // it is called from, so fewer places make a far quicker first compile.
    var drawn: array<Drawn, 2>;
    var count = 0u;
    if thread < parents {
        let b = born(thread, 0u, now);
        if !b.ok { return; }
        let age = now - b.birth;
        if age >= b.own_life { return; }
        if simulated() && !simulated_at(thread, b.generation) { return; }
        let first_colour = vec4<f32>(setting(COLOUR, b.birth), setting(COLOUR + 1u, b.birth), setting(COLOUR + 2u, b.birth), setting(COLOUR_A, b.birth));
        let last_colour = vec4<f32>(setting(END_COLOUR, b.birth), setting(END_COLOUR + 1u, b.birth), setting(END_COLOUR + 2u, b.birth), setting(END_A, b.birth));
        var landing = 0.0;
        if gathering() && b.homing > 0.0 { landing = smoothstep(1.0 - b.homing, 1.0, age / flight(b.own_life, b.birth)); }
        // A bird of a flock born on its emitter keeps the colour it was born in.
        var born_on = b.born_on;
        if simulated() && held_tint.w > 0.5 { born_on = held_tint.rgb; }
        let tint = mix(vec3<f32>(1.0), mix(born_on, b.lands_on, landing), setting(INHERIT, b.birth));
        drawn[0] = Drawn(b.origin, b.launch, b.birth, age, b.own_life, b.goal, b.homing, b.deep,
            Look(first_colour, last_colour, tint, setting(SIZE, b.birth), thread, b.generation, random(thread, b.generation, 12u), true, on_impact, b.surface, b.landing));
        count = 1u;
    } else {
        // A burst particle: the parent's thread and which of its burst this is. It looks
        // back over the parent's last two lives for the one whose end it follows.
        let each = max(u32(camera_row(EACH)), 1u);
        let parent = (thread - parents) / each;
        let index = (thread - parents) % each;
        for (var back = 0u; back < 2u; back++) {
            let b = born(parent, back, now);
            if !b.ok { continue; }
            if f32(index) >= round(setting(BURST, b.birth)) { continue; }
            // Where the parent ended (its life's end, or where it first touched the
            // obstacle) and a moment before: three places on its path, from one call.
            var end = b.own_life;
            var path_end = vec3<f32>(0.0);
            var at = vec3<f32>(0.0);
            var before = vec3<f32>(0.0);
            var h = 0.0;
            var missed = false;
            var death = 0.0;
            var child_life = 0.0;
            var age = 0.0;
            for (var q = 0u; q < 3u; q++) {
                var asked = b.own_life;
                if q == 1u {
                    if !(on_impact || end < b.own_life) { at = path_end; continue; }
                    asked = end;
                } else if q == 2u {
                    asked = end - h;
                }
                let there = place(b.origin, b.launch, b.birth, asked, b.own_life, b.goal, b.homing, b.deep);
                if q == 0u {
                    path_end = there;
                    if on_impact {
                        if impact < 0.0 { missed = true; break; }
                        end = impact;
                    }
                    // A burst particle not alive now needs no more of the path.
                    death = b.birth + end;
                    child_life = setting(BURST_LIFE, b.birth) * (1.0 - 0.6 * setting(VARIATION, b.birth) * random(thread, b.generation, 21u));
                    age = now - death;
                    if age < 0.0 || age >= child_life { missed = true; break; }
                    h = min(1.0 / 120.0, end);
                } else if q == 1u {
                    at = there;
                } else {
                    before = there;
                }
            }
            if missed { continue; }
            // How fast the parent was going as it ended.
            var moving = vec3<f32>(0.0);
            if h > 0.0 { moving = (at - before) / h; }
            // Thrown within the spread round the way the parent was going (back the way it
            // came off an obstacle), the parent's own speed carried on at its end of life.
            var ahead = atan2(-moving.y, moving.x);
            if on_impact { ahead += 3.14159265; } else if length(moving.xy) < 1.0 { ahead = 1.5707963; }
            let heading = ahead + (random(thread, b.generation, 22u) - 0.5) * radians(setting(BURST_SPREAD, b.birth));
            var lean = 0.0;
            if b.deep { lean = (random(thread, b.generation, 23u) - 0.5) * radians(setting(BURST_SPREAD, b.birth)); }
            var launch = vec3<f32>(cos(heading) * cos(lean), -sin(heading) * cos(lean), sin(lean))
                * setting(BURST_SPEED, b.birth) * (1.0 - 0.7 * setting(VARIATION, b.birth) * random(thread, b.generation, 24u));
            if !on_impact { launch += moving; }
            // The burst's colour from its start, fading out by the fade setting.
            let burst_colour = vec4<f32>(setting(BURST_COLOUR, b.birth), setting(BURST_COLOUR + 1u, b.birth), setting(BURST_COLOUR + 2u, b.birth), setting(BURST_A, b.birth));
            let tint = mix(vec3<f32>(1.0), b.born_on, setting(INHERIT, b.birth));
            // Clear of the obstacle it ended on, so the burst does not start inside it.
            drawn[count] = Drawn(at, launch, death, age, child_life, at, 0.0, b.deep,
                Look(burst_colour, burst_colour, tint, setting(SIZE, b.birth) * setting(BURST_SIZE, b.birth), thread, b.generation,
                    // A burst turns its hue with the particle it came from.
                    random(parent, b.generation, 12u), false, false, vec3<f32>(0.0), vec3<f32>(0.0)));
            count += 1u;
        }
    }
    for (var k = 0u; k < count; k++) {
        let d = drawn[k];
        draw(d.origin, d.launch, d.birth, d.age, d.own_life, d.goal, d.homing, d.deep, d.look);
    }
}
// ---- positions: as splat; result = two values for each linked particle: where it is
// on the frame (xy), how visible (z, 0 when not drawn) and its size; then its colour ----
@compute @workgroup_size(64)
fn particle_positions(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= u32(camera_row(LINKED)) { return; }
    let now = bitcast<f32>(p.size_op.z);
    var spot = vec4<f32>(0.0); var colour = vec4<f32>(0.0);
    let b = born(i, 0u, now);
    let age = now - b.birth;
    if b.ok && age < b.own_life && (!simulated() || simulated_at(i, b.generation)) {
        let where_now = place(b.origin, b.launch, b.birth, age, b.own_life, b.goal, b.homing, b.deep);
        let ended = camera_row(ON_IMPACT) > 0.5 && impact >= 0.0;
        let seen = filmed(where_now);
        let away = distance_to(where_now);
        let wall = occluder(seen.xy);
        let hidden = wall > 0.0 && away > wall * 1.002 + 1.0;
        if !ended && !hidden && seen.z > 0.0 {
            let share = age / b.own_life;
            let fade = setting(FADE, b.birth);
            var opacity = min(share / 0.04, 1.0);
            if has_curve(2u, b.birth) { opacity = life_curve(OPACITY_CURVE, b.birth, share); }
            else if fade > 0.0 { opacity *= clamp((1.0 - share) / fade, 0.0, 1.0); }
            let first_colour = vec4<f32>(setting(COLOUR, b.birth), setting(COLOUR + 1u, b.birth), setting(COLOUR + 2u, b.birth), setting(COLOUR_A, b.birth));
            let last_colour = vec4<f32>(setting(END_COLOUR, b.birth), setting(END_COLOUR + 1u, b.birth), setting(END_COLOUR + 2u, b.birth), setting(END_A, b.birth));
            colour = mix(first_colour, last_colour, share);
            if has_curve(8u, b.birth) { colour = life_colour(b.birth, share); }
            let shaded = surface_light(b.surface, b.origin, b.landing, b.goal, where_now, b.birth);
            colour = vec4<f32>(lit_colour(colour.rgb, lighting(where_now, b.birth, facet(i, b.generation, age, b.birth), shaded.xyz, shaded.w), b.birth), colour.a);
            let hues = setting(HUES, b.birth);
            if hues > 0.0 { colour = vec4<f32>(turned(colour.rgb, (random(i, b.generation, 12u) - 0.5) * 6.2831853 * hues), colour.a); }
            spot = vec4<f32>(seen.xy, opacity * colour.a, setting(SIZE, b.birth));
        }
    }
    store(2u * i, spot);
    store(2u * i + 1u, colour);
}
// ---- links: second = positions, third = fields; result = light summed as the splat's ----
// A line from each linked particle to every later one nearer than the link distance,
// stronger the nearer they are, in the two particles' colours.
@compute @workgroup_size(64)
fn particle_links(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(camera_row(LINKED));
    if i >= n { return; }
    let a = second[2u * i];
    if a.z <= 0.0 { return; }
    let now = bitcast<f32>(p.size_op.z);
    let reach = setting(LINK_DISTANCE, now);
    if reach <= 0.0 { return; }
    let strength = setting(LINK_OPACITY, now);
    let radius = max(setting(LINK_WIDTH, now) * 0.5, 0.35);
    let ca = second[2u * i + 1u];
    var made = 0u;
    for (var j = i + 1u; j < n; j++) {
        let b = second[2u * j];
        if b.z <= 0.0 { continue; }
        let apart = length(b.xy - a.xy);
        if apart >= reach || apart < 0.5 { continue; }
        let near = 1.0 - apart / reach;
        let cb = second[2u * j + 1u];
        let colour = (ca.rgb + cb.rgb) * 0.5;
        let alpha = near * near * strength * min(a.z, b.z);
        let spacing = max(radius, 0.5) * 1.2;
        let dots = clamp(u32(ceil(apart / spacing)), 1u, 512u);
        // Dots along the line overlap: each gives its share so the line is as bright as set.
        let each = vec4<f32>(colour * alpha, alpha) * min(1.0, 1.1 * spacing / max(radius * 2.0, 1.0));
        for (var d = 0u; d <= dots; d++) {
            stamp(mix(a.xy, b.xy, f32(d) / f32(dots)), radius, each);
        }
        made += 1u;
        if made >= 48u { break; }
    }
}
// ---- resolve: source = splat sums, second = fields, third = lines' sums (when linked) ----
@compute @workgroup_size(8, 8)
fn particle_resolve(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let i = gid.y * p.size_op.x + gid.x;
    var sum = vec4<f32>(first[i]) / SCALE;
    // Lines between particles add their light (their sums come in as the third input).
    if second[tiles() + ROWS * COUNT + LINKED].w > 0.0 {
        sum += vec4<f32>(bitcast<vec4<u32>>(third[i])) / SCALE;
    }
    // Glow adds the light; without it dots cover one another and alpha saturates softly.
    let glow = second[tiles() + (ROWS - 1u) * COUNT + GLOW].w;
    let covered = 1.0 - exp(-sum.a);
    var plain = vec4<f32>(0.0);
    if sum.a > 0.0 { plain = vec4<f32>(sum.rgb / sum.a * covered, covered); }
    // A light limit bends added light above half of it smoothly towards it (slope 1 at the
    // bend, so nothing jumps), the same factor for every channel so the colour stays.
    var light = sum.rgb;
    let limit = second[tiles() + (ROWS - 1u) * COUNT + LIGHT_LIMIT].w;
    let peak = max(light.r, max(light.g, light.b));
    let bend = limit * 0.5;
    if limit > 0.0 && peak > bend {
        light *= (bend + bend * (1.0 - exp(-(peak - bend) / bend))) / peak;
    }
    store(i, mix(plain, vec4<f32>(light, min(sum.a, 1.0)), glow));
}
