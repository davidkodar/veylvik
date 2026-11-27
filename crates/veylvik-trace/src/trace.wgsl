// Path tracing over the packed scene (see scene.rs): the camera's rays walk a top tree to
// placed meshes and each mesh's own tree; lights are sampled directly and area panels are
// also met by reflected rays, the two weighed against each other (multiple importance
// sampling). Scene frame: x right, y down, z towards the viewer; scene-linear Rec.709.
// The environment lookups (`environment_specular`) are appended from the renderer's
// shared WGSL and read `environment_map`.
struct Node { min: vec3<f32>, first: u32, max: vec3<f32>, count: u32 }
struct Instance { to_object: mat3x4<f32>, to_world: mat3x4<f32>, root: u32, triangle_base: u32, vertex_base: u32, surface: u32 }
// finish: brushing, the grain's direction (radians), imperfections, visible grain.
struct Surface { colour: vec3<f32>, roughness: f32, metallic: f32, emission: f32, opacity: f32, flags: u32, texture: u32, shadow: f32, ridges: f32, look: u32, finish: vec4<f32> }
struct Light { position: vec4<f32>, direction: vec4<f32>, colour: vec4<f32>, right: vec4<f32>, up: vec4<f32> }
struct Frame {
    // camera to world (rows), focal length in pixels, width, height
    camera: mat3x4<f32>,
    // focal px, width, height, lens radius (scene units; 0 = pinhole)
    lens: vec4<f32>,
    // focus distance, environment strength, environment turn (radians), backdrop (1)
    look: vec4<f32>,
    // lights, bounces, samples in this pass, first sample number of this pass
    counts: vec4<u32>,
    // seed, instances, the brightest a bounce may add (0: no limit), how much of the
    // picture this render is (below 1: mixed in, its samples in one pass)
    more: vec4<f32>,
    // where in `data` the placings (7 a placing), surfaces (2 each), top tree nodes
    // (2 each), its order (4 to a vec4) and the lights (5 each) start
    offsets: vec4<u32>,
    // where the lights start, environment sampling (1), where the textures' places
    // start, where the looks start (43 vec4s each: uv scale, parameters, studio)
    lights_at: vec4<u32>,
}
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var<storage, read> vertices: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> triangles: array<vec4<u32>>;
// The frame's small arrays in one buffer (devices allow few storage buffers a stage).
@group(0) @binding(4) var<storage, read> data: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> environment_map: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> picture: array<vec4<f32>>;
// The environment as a table to draw directions from (sky_table.wgsl).
@group(0) @binding(7) var<storage, read> sky_table: array<f32>;
const ACROSS: u32 = 256u;
const DOWN: u32 = 128u;
// Every picture surfaces read, one after another; their places are in `data`.
@group(0) @binding(8) var<storage, read> pictures: array<vec4<f32>>;
const UNLIT: u32 = 1u;
const CATCHER: u32 = 2u;
const CARD: u32 = 4u;
const CUT: u32 = 8u;
const STRAIGHT: u32 = 16u;
const GLASS: u32 = 32u;
fn bits(v: vec4<f32>) -> vec4<u32> { return bitcast<vec4<u32>>(v); }
fn instance_at(i: u32) -> Instance {
    let b = frame.offsets.x + 7u * i;
    let k = bits(data[b + 6u]);
    return Instance(mat3x4<f32>(data[b], data[b + 1u], data[b + 2u]), mat3x4<f32>(data[b + 3u], data[b + 4u], data[b + 5u]), k.x, k.y, k.z, k.w);
}
fn surface_at(i: u32) -> Surface {
    let b = frame.offsets.y + 4u * i;
    let a = data[b];
    let c = data[b + 1u];
    let e = data[b + 2u];
    return Surface(a.xyz, a.w, c.x, c.y, c.z, bitcast<u32>(c.w), bitcast<u32>(e.x), e.y, e.z, bitcast<u32>(e.w), data[b + 3u]);
}
// A picture's texel, repeating round its edges or clear beyond them.
fn picture_texel(place: vec4<u32>, x: i32, y: i32, repeat: bool) -> vec4<f32> {
    let w = i32(place.y);
    let h = i32(place.z);
    if repeat {
        return pictures[place.x + u32(((y % h + h) % h) * w + (x % w + w) % w)];
    }
    if x < 0 || y < 0 || x >= w || y >= h { return vec4<f32>(0.0); }
    return pictures[place.x + u32(y * w + x)];
}
// Picture `t` at `uv` (0 to 1 across it), bilinear at its pixel centres, as the
// interactive renderer reads it.
fn sample_picture(t: u32, uv: vec2<f32>, repeat: bool) -> vec4<f32> {
    let place = bits(data[frame.lights_at.z + t]);
    if place.y == 0u || place.z == 0u { return vec4<f32>(1.0); }
    var at = uv;
    if repeat { at = fract(uv); }
    let q = at * vec2<f32>(f32(place.y), f32(place.z)) - vec2<f32>(0.5);
    let i = vec2<i32>(floor(q));
    let f = fract(q);
    return mix(mix(picture_texel(place, i.x, i.y, repeat), picture_texel(place, i.x + 1, i.y, repeat), f.x),
        mix(picture_texel(place, i.x, i.y + 1, repeat), picture_texel(place, i.x + 1, i.y + 1, repeat), f.x), f.y);
}
// The texture coordinates where a ray met triangle `t` (in the packed array).
fn uv_at(inst: Instance, t: u32, u: f32, v: f32) -> vec2<f32> {
    let k = triangles[t];
    let base = inst.vertex_base;
    let a = vertices[2u * (base + k[0]) + 1u].zw;
    let b = vertices[2u * (base + k[1]) + 1u].zw;
    let c = vertices[2u * (base + k[2]) + 1u].zw;
    return a * (1.0 - u - v) + b * u + c * v;
}
fn top_node(k: u32) -> Node {
    let b = frame.offsets.z + 2u * k;
    let a = data[b];
    let c = data[b + 1u];
    return Node(a.xyz, bitcast<u32>(a.w), c.xyz, bitcast<u32>(c.w));
}
fn top_index(k: u32) -> u32 {
    return bits(data[frame.offsets.w + k / 4u])[k % 4u];
}
fn light_at(i: u32) -> Light {
    let b = frame.lights_at.x + 5u * i;
    return Light(data[b], data[b + 1u], data[b + 2u], data[b + 3u], data[b + 4u]);
}

const PI: f32 = 3.14159265;
const STACK: u32 = 48u;

// ---- random numbers: a hash per pixel, sample and draw ----
var<private> rng: u32;
fn hash(x: u32) -> u32 {
    var v = x * 747796405u + 2891336453u;
    let w = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (w >> 22u) ^ w;
}
fn random() -> f32 {
    rng = hash(rng);
    return f32(rng >> 8u) / 16777216.0;
}

// ---- walking the trees ----
struct Hit { t: f32, instance: u32, triangle: u32, u: f32, v: f32 }
fn box_near(n: Node, o: vec3<f32>, inv: vec3<f32>, far: f32) -> f32 {
    let a = (n.min - o) * inv;
    let b = (n.max - o) * inv;
    let lo = min(a, b);
    let hi = max(a, b);
    let near = max(max(lo.x, lo.y), max(lo.z, 0.0));
    let out = min(min(hi.x, hi.y), min(hi.z, far));
    return select(-1.0, near, near <= out);
}
fn triangle_hit(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, o: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = cross(d, e2);
    let det = dot(e1, p);
    if abs(det) < 1e-12 { return vec3<f32>(-1.0); }
    let s = o - a;
    let u = dot(s, p) / det;
    if u < 0.0 || u > 1.0 { return vec3<f32>(-1.0); }
    let q = cross(s, e1);
    let v = dot(d, q) / det;
    if v < 0.0 || u + v > 1.0 { return vec3<f32>(-1.0); }
    return vec3<f32>(dot(e2, q) / det, u, v);
}
fn corner(inst: Instance, t: u32, k: u32) -> vec3<f32> {
    let i = triangles[inst.triangle_base + t][k];
    return vertices[2u * (inst.vertex_base + i)].xyz;
}
// The nearest triangle of one placed mesh nearer than `far`, in its own space.
fn mesh_hit(inst: Instance, o: vec3<f32>, d: vec3<f32>, far: f32, stop_at_first: bool) -> Hit {
    var best = Hit(far, 0u, 0xffffffffu, 0.0, 0.0);
    // A shape whose picture cuts holes: rays pass where it is under half opaque.
    let surface = surface_at(inst.surface);
    let cut = (surface.flags & CUT) != 0u && surface.texture != 0xffffffffu;
    let inv = 1.0 / d;
    var stack: array<u32, STACK>;
    var size = 1u;
    stack[0] = inst.root;
    while size > 0u {
        size -= 1u;
        let n = nodes[stack[size]];
        if box_near(n, o, inv, best.t) < 0.0 { continue; }
        if n.count > 0u {
            for (var k = n.first; k < n.first + n.count; k++) {
                let h = triangle_hit(corner(inst, k, 0u), corner(inst, k, 1u), corner(inst, k, 2u), o, d);
                if h.x > 0.0 && h.x < best.t {
                    if cut && sample_picture(surface.texture, uv_at(inst, inst.triangle_base + k, h.y, h.z), true).a < 0.5 { continue; }
                    best = Hit(h.x, 0u, inst.triangle_base + k, h.y, h.z);
                    if stop_at_first { return best; }
                }
            }
        } else if n.first > 0u && size + 2u <= STACK {
            let a = inst.root + n.first;
            let da = box_near(nodes[a], o, inv, best.t);
            let db = box_near(nodes[a + 1u], o, inv, best.t);
            // The nearer child last, so it is walked first.
            if da >= 0.0 && db >= 0.0 {
                if da <= db { stack[size] = a + 1u; stack[size + 1u] = a; } else { stack[size] = a; stack[size + 1u] = a + 1u; }
                size += 2u;
            } else if da >= 0.0 { stack[size] = a; size += 1u; }
            else if db >= 0.0 { stack[size] = a + 1u; size += 1u; }
        }
    }
    return best;
}
// What a walk of the scene looks for: the nearest surface, the first found that blocks light
// (unlit surfaces, cards, shadow catchers and glass do not), or the nearest glass.
const NEAREST: u32 = 0u;
const BLOCKER: u32 = 1u;
const NEAREST_GLASS: u32 = 2u;
// What a walk finds along a world ray nearer than `far`.
fn scene_hit(o: vec3<f32>, d: vec3<f32>, far: f32, mode: u32) -> Hit {
    let stop_at_first = mode == BLOCKER;
    var best = Hit(far, 0u, 0xffffffffu, 0.0, 0.0);
    if u32(frame.more.y) == 0u { return best; }
    let inv = 1.0 / d;
    var stack: array<u32, STACK>;
    var size = 1u;
    stack[0] = 0u;
    while size > 0u {
        size -= 1u;
        let n = top_node(stack[size]);
        if box_near(n, o, inv, best.t) < 0.0 { continue; }
        if n.count > 0u {
            for (var k = n.first; k < n.first + n.count; k++) {
                let i = top_index(k);
                let inst = instance_at(i);
                let flags = surface_at(inst.surface).flags;
                if mode == BLOCKER && (flags & (UNLIT | CATCHER | CARD | GLASS)) != 0u { continue; }
                if mode == NEAREST_GLASS && (flags & GLASS) == 0u { continue; }
                // The instance's own space; the direction is turned, not normalised, so
                // distances along the ray stay in world lengths.
                let oo = vec4<f32>(o, 1.0) * inst.to_object;
                let od = vec4<f32>(d, 0.0) * inst.to_object;
                let h = mesh_hit(inst, oo, od, best.t, stop_at_first);
                if h.triangle != 0xffffffffu {
                    best = Hit(h.t, i, h.triangle, h.u, h.v);
                    if stop_at_first { return best; }
                }
            }
        } else if n.first > 0u && size + 2u <= STACK {
            stack[size] = n.first + 1u;
            stack[size + 1u] = n.first;
            size += 2u;
        }
    }
    return best;
}
fn blocked(o: vec3<f32>, d: vec3<f32>, far: f32) -> bool {
    return scene_hit(o, d, far, BLOCKER).triangle != 0xffffffffu;
}
fn has_glass() -> bool { return (frame.lights_at.y & 4u) != 0u; }
// How much light gets along a ray before `far`: none past anything solid, and through glass
// what each face lets through (Fresnel's share at that angle), tinted where it goes in, as
// a path through the glass is. The light goes straight on (no focusing: no caustics). `w`
// is 1 when it passed through glass, so a path that would have reached the light by
// bending through that glass must not count it again.
fn transmitted(o: vec3<f32>, d: vec3<f32>, far: f32) -> vec4<f32> {
    if blocked(o, d, far) { return vec4<f32>(0.0); }
    var light = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    if !has_glass() { return light; }
    var at = o;
    var left = far;
    for (var k = 0u; k < 16u; k++) {
        let h = scene_hit(at, d, left, NEAREST_GLASS);
        if h.triangle == 0xffffffffu { break; }
        let inst = instance_at(h.instance);
        let s = surface_at(inst.surface);
        let look = frame.lights_at.w + 43u * s.look + 4u;
        let f0 = clamp(data[look].g, 0.0, 0.9);
        let n = smooth_normal(inst, h);
        let fr = f0 + (1.0 - f0) * pow(1.0 - abs(dot(n, d)), 5.0);
        var share = vec3<f32>(1.0 - fr);
        // Going in (its normals point outwards): tinted as a path through it is.
        if dot(n, d) < 0.0 { share *= data[look + 34u].rgb; }
        light = vec4<f32>(light.rgb * share, 1.0);
        if max(max(light.r, light.g), light.b) < 1e-5 { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
        let p = at + d * h.t;
        let step = h.t + 1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z));
        at += d * step;
        left -= step;
        if left <= 0.0 { break; }
    }
    return light;
}
// A hit's interpolated normal in the world, as the mesh has it (not turned to face the ray).
fn smooth_normal(inst: Instance, h: Hit) -> vec3<f32> {
    let base = inst.vertex_base;
    var n = vec3<f32>(0.0);
    let w = vec3<f32>(1.0 - h.u - h.v, h.u, h.v);
    for (var k = 0u; k < 3u; k++) {
        let i = 2u * (base + triangles[h.triangle][k]);
        n += vec3<f32>(vertices[i].w, vertices[i + 1u].xy) * w[k];
    }
    let to_world_normal = mat3x3<f32>(inst.to_object[0].xyz, inst.to_object[1].xyz, inst.to_object[2].xyz);
    return normalize(to_world_normal * n);
}

// ---- surfaces ----
fn ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-12);
}
fn smith(nv: f32, nl: f32, a: f32) -> f32 {
    let k = a * 0.5;
    return (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
}
// How a point of a surface takes light: its colours, its roughness along (`ax`) and across
// (`ay`) its grain (equal without brushing; reflections are drawn from the even lobe `a`
// between them), the grain's directions, and how its finish brightens or dulls reflections.
struct Shade { base: vec3<f32>, f0: vec3<f32>, diffuse: vec3<f32>, a: f32, ax: f32, ay: f32, along: vec3<f32>, across: vec3<f32>, gain: f32 }
fn shade_of(s: Surface) -> Shade {
    let polish = clamp(s.roughness, 0.03, 1.0);
    let a = polish * polish;
    let metal = clamp(s.metallic, 0.0, 1.0);
    return Shade(s.colour, mix(vec3<f32>(0.04), s.colour, metal), s.colour * (1.0 - metal), a, a, a, vec3<f32>(0.0), vec3<f32>(0.0), 1.0);
}
// ---- finishes, as the interactive renderer makes them ----
fn finish_hash(q: vec2<f32>) -> f32 {
    var h = vec3<f32>(q.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973);
    h = fract(h);
    h += dot(h, h.yzx + 33.33);
    return fract((h.x + h.y) * h.z);
}
fn finish_noise(q: vec2<f32>) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let t = f * f * (3.0 - 2.0 * f);
    return mix(mix(finish_hash(i), finish_hash(i + vec2<f32>(1.0, 0.0)), t.x),
               mix(finish_hash(i + vec2<f32>(0.0, 1.0)), finish_hash(i + vec2<f32>(1.0, 1.0)), t.x), t.y);
}
fn smudges(q: vec2<f32>) -> f32 {
    return (finish_noise(q * 0.006 + vec2<f32>(3.0, 9.0)) - 0.5) * 1.0
        + (finish_noise(q * 0.019 + vec2<f32>(11.0, 2.0)) - 0.5) * 0.6
        + (finish_noise(q * 0.07 + vec2<f32>(7.0, 4.0)) - 0.5) * 0.3
        + (finish_noise(q * 0.9 + vec2<f32>(1.0, 6.0)) - 0.5) * 0.1;
}
// A surface's light with its finish at a point: `local` and `facing` are the point and
// its normal in the object's own space (so the finish stays on the surface as it moves),
// `normals` turns directions from that space to the world's; `n_in` is the shading normal,
// returned tilted by ridges and waves.
struct Finished { shade: Shade, n: vec3<f32> }
fn finished(s: Surface, local: vec3<f32>, facing: vec3<f32>, normals: mat3x3<f32>, n_in: vec3<f32>) -> Finished {
    var m = shade_of(s);
    let brushed = clamp(s.finish.x, 0.0, 1.0);
    let worn = clamp(s.finish.z, 0.0, 1.0);
    let grainy = clamp(s.finish.w, 0.0, 1.0);
    let ridged = clamp(s.ridges, 0.0, 1.0);
    var n = n_in;
    if brushed <= 0.0 && worn <= 0.0 && grainy <= 0.0 && ridged <= 0.0 {
        return Finished(m, n);
    }
    // The grain's frame on the surface, turned by the finish's direction.
    let fm = normalize(facing);
    var side = cross(fm, vec3<f32>(0.0, 1.0, 0.0));
    if dot(side, side) < 1e-4 { side = vec3<f32>(1.0, 0.0, 0.0); }
    side = normalize(side);
    let up = cross(side, fm);
    let c = cos(s.finish.y);
    let sn = sin(s.finish.y);
    let along_local = side * c + up * sn;
    let across_local = up * c - side * sn;
    // Patterns are sized for a 1920-wide frame and grow with the frame.
    let q = vec2<f32>(dot(local, along_local), dot(local, across_local)) / max(frame.lens.y / 1920.0, 1e-3);
    let blotch = smudges(q);
    var across = normalize(normals * across_local);
    across = normalize(across - n * dot(across, n));
    var along = normalize(cross(across, n));
    let ridge = (finish_noise(vec2<f32>(q.x * 0.0026, q.y * 0.0105)) - 0.5)
        + 0.4 * (finish_noise(vec2<f32>(q.x * 0.006 + 13.0, q.y * 0.028 + 5.0)) - 0.5);
    let wave = vec2<f32>(finish_noise(q * 0.012 + vec2<f32>(21.0, 5.0)) - 0.5,
                         finish_noise(q * 0.012 + vec2<f32>(4.0, 31.0)) - 0.5);
    let fine = finish_noise(vec2<f32>(q.x * 0.01, q.y * 1.9)) - 0.5;
    n = normalize(n + across * ridge * 0.22 * ridged
        + (along * wave.x + across * wave.y) * 0.025 * worn
        + across * fine * 0.01 * grainy);
    across = normalize(across - n * dot(across, n));
    along = normalize(cross(across, n));
    // Wear changes the polish from place to place; brushing scatters across the grain.
    let polish = clamp(s.roughness * (1.0 + blotch * 1.1 * worn), 0.03, 1.0);
    let a = polish * polish;
    m.ax = a;
    m.ay = clamp(a + brushed * 0.12, 0.0, 1.0);
    m.a = sqrt(m.ax * m.ay);
    m.along = along;
    m.across = across;
    var gain = 1.0;
    if grainy > 0.0 {
        let streaks = (finish_noise(vec2<f32>(q.x * 0.010, q.y * 2.6)) - 0.5) * 0.55
            + (finish_noise(vec2<f32>(q.x * 0.016 + 9.0, q.y * 1.1)) - 0.5) * 0.7
            + (finish_noise(vec2<f32>(q.x * 0.006 + 2.0, q.y * 0.4)) - 0.5) * 0.6
            + (finish_noise(vec2<f32>(q.x * 0.003 + 31.0, q.y * 0.13)) - 0.5) * 0.5;
        gain = max(1.0 + streaks * grainy, 0.0);
    }
    if worn > 0.0 {
        var bright = 0.0;
        var dull = 0.0;
        for (var k = 0; k < 3; k++) {
            let turn = (f32(k) - 1.0) * 0.22;
            let cc = cos(turn);
            let ss = sin(turn);
            let r = vec2<f32>(q.x * cc - q.y * ss, q.x * ss + q.y * cc);
            let o = f32(k) * 37.0;
            bright += smoothstep(0.62, 0.95, finish_noise(vec2<f32>(r.x * 0.014 + o, r.y * 0.7 + o)))
                * smoothstep(0.35, 0.6, finish_noise(r * 0.03 + vec2<f32>(o, 5.0)));
            dull += smoothstep(0.62, 0.95, finish_noise(vec2<f32>(r.x * 0.012 + o + 50.0, r.y * 0.55 + o)))
                * smoothstep(0.35, 0.6, finish_noise(r * 0.025 + vec2<f32>(9.0, o)));
        }
        gain *= max(1.0 + worn * (0.3 * bright - 0.16 * dull), 0.0);
    }
    m.gain = gain;
    return Finished(m, n);
}

// ---- library looks: the material's own blocks, read by the shared surface code ----
var<private> p: Params;
var<private> studio: StudioLighting;
var<private> look_scale: vec2<f32>;
fn load_look(k: u32) {
    let b = frame.lights_at.w + 43u * k;
    look_scale = data[b].xy;
    p = Params(bits(data[b + 1u]), data[b + 2u], data[b + 3u]);
    let q = b + 4u;
    studio.optics = data[q];
    studio.environment = data[q + 1u];
    for (var i = 0u; i < 8u; i++) {
        let c = q + 2u + 4u * i;
        studio.lights[i] = StudioLight(data[c], data[c + 1u], data[c + 2u], data[c + 3u]);
    }
    for (var i = 0u; i < 3u; i++) { studio.palette[i] = data[q + 34u + i]; }
    studio.maps = data[q + 37u];
    studio.finish = data[q + 38u];
    map_roughness = -1.0;
}
// The look's pattern coordinates from the mesh's uv, as the interactive renderer makes them.
fn look_uv(uv: vec2<f32>) -> vec2<f32> { return uv * look_scale / max(p.values.x, 1e-3); }
// A microfacet normal about `n` drawn by the GGX distribution of roughness `a`.
fn facet(n: vec3<f32>, a: f32) -> vec3<f32> {
    let u1 = random();
    let u2 = random();
    let a2 = a * a;
    let cos_t = sqrt((1.0 - u1) / (1.0 + (a2 - 1.0) * u1));
    let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
    let phi = 2.0 * PI * u2;
    return frame_of(n) * vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t);
}

// The specular part of the last `brdf`, for lights that show less in shiny surfaces.
var<private> shiny: vec3<f32>;
// Whether `draw` last drew a reflection (rather than a matte scatter).
var<private> drew_reflection: bool;
// What reaches the eye along `v` of light arriving along `l`, per unit of light and
// cosine, as the rasteriser's lights are (their radiance taken with a factor of pi).
fn brdf(m: Shade, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nl = dot(n, l);
    let nv = max(dot(n, v), 1e-4);
    if nl <= 0.0 { return vec3<f32>(0.0); }
    let h = normalize(l + v);
    let f = m.f0 + (vec3<f32>(1.0) - m.f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
    var lobe = ggx(max(dot(n, h), 0.0), m.a);
    if m.ay > m.ax {
        // Brushed: rougher across the grain than along it.
        let x = dot(h, m.along) / m.ax;
        let y = dot(h, m.across) / m.ay;
        let z = dot(h, n);
        let dd = x * x + y * y + z * z;
        lobe = 1.0 / (PI * m.ax * m.ay * dd * dd);
    }
    let specular = lobe * smith(nv, nl, m.a) * f * m.gain / max(4.0 * nv * nl, 1e-4);
    shiny = specular;
    return m.diffuse / PI * (vec3<f32>(1.0) - f) + specular;
}
fn specular_share(m: Shade, n: vec3<f32>, v: vec3<f32>) -> f32 {
    let f = m.f0 + (vec3<f32>(1.0) - m.f0) * pow(1.0 - max(dot(n, v), 0.0), 5.0);
    let spec = dot(f, vec3<f32>(0.2126, 0.7152, 0.0722));
    let diff = dot(m.diffuse * (vec3<f32>(1.0) - f), vec3<f32>(0.2126, 0.7152, 0.0722));
    return clamp(spec / max(spec + diff, 1e-4), 0.1, 0.95);
}
// The likelihood of drawing `l` by `draw`'s choice between a cosine-weighted diffuse
// direction and a GGX reflection.
fn draw_pdf(m: Shade, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> f32 {
    let nl = dot(n, l);
    if nl <= 0.0 { return 0.0; }
    let p = specular_share(m, n, v);
    let h = normalize(l + v);
    let nh = max(dot(n, h), 0.0);
    let spec = ggx(nh, m.a) * nh / max(4.0 * max(dot(v, h), 1e-4), 1e-6);
    return p * spec + (1.0 - p) * nl / PI;
}
fn frame_of(n: vec3<f32>) -> mat3x3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    return mat3x3<f32>(vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x), vec3<f32>(b, s + n.y * n.y * a, -n.y), n);
}
fn draw(m: Shade, n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let basis = frame_of(n);
    let u1 = random();
    let u2 = random();
    drew_reflection = random() < specular_share(m, n, v);
    if drew_reflection {
        // A microfacet normal drawn by the GGX distribution, and the mirror about it.
        let a2 = m.a * m.a;
        let cos_t = sqrt((1.0 - u1) / (1.0 + (a2 - 1.0) * u1));
        let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
        let phi = 2.0 * PI * u2;
        let h = basis * vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t);
        return reflect(-v, h);
    }
    let r = sqrt(u1);
    let phi = 2.0 * PI * u2;
    return basis * vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(1.0 - u1, 0.0)));
}
fn turned(d: vec3<f32>) -> vec3<f32> {
    let c = cos(frame.look.z);
    let s = sin(frame.look.z);
    // As the interactive renderer turns it.
    return vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
}
fn sky(d: vec3<f32>) -> vec3<f32> {
    return environment_specular(turned(d), 0.0) * frame.look.y;
}
fn sampling_sky() -> bool {
    return (frame.lights_at.y & 1u) != 0u && frame.look.y > 0.0;
}
// The first entry of `table[base .. base + n]` at or above `u` (a running share).
fn find(base: u32, n: u32, u: f32) -> u32 {
    var lo = 0u;
    var hi = n - 1u;
    while lo < hi {
        let mid = (lo + hi) / 2u;
        if sky_table[base + mid] < u { lo = mid + 1u; } else { hi = mid; }
    }
    return lo;
}
fn texel_share(x: u32, y: u32) -> f32 {
    let row = ACROSS * DOWN;
    let down = sky_table[row + y] - select(0.0, sky_table[row + y - 1u], y > 0u);
    let across = sky_table[y * ACROSS + x] - select(0.0, sky_table[y * ACROSS + x - 1u], x > 0u);
    return down * across;
}
// A direction drawn where the environment is bright, and its likelihood (per unit of
// solid angle).
fn sky_draw() -> vec4<f32> {
    let y = find(ACROSS * DOWN, DOWN, random());
    let x = find(y * ACROSS, ACROSS, random());
    let u = (f32(x) + random()) / f32(ACROSS);
    let v = (f32(y) + random()) / f32(DOWN);
    let phi = (u - 0.5) * 2.0 * PI;
    let theta = v * PI;
    let s = max(sin(theta), 1e-6);
    // The environment's own direction, then turned back to the world's.
    let e = vec3<f32>(s * cos(phi), -cos(theta), s * sin(phi));
    let c = cos(frame.look.z);
    let n = sin(frame.look.z);
    let d = vec3<f32>(c * e.x + n * e.z, e.y, -n * e.x + c * e.z);
    return vec4<f32>(d, texel_share(x, y) * f32(ACROSS * DOWN) / (2.0 * PI * PI * s));
}
// How likely `sky_draw` was to draw direction `d`.
fn sky_pdf(d: vec3<f32>) -> f32 {
    let e = turned(d);
    let f = normalize(vec3<f32>(e.x, -e.y, e.z));
    let u = atan2(f.z, f.x) / (2.0 * PI) + 0.5;
    let theta = acos(clamp(f.y, -1.0, 1.0));
    let x = min(u32(u * f32(ACROSS)), ACROSS - 1u);
    let y = min(u32(theta / PI * f32(DOWN)), DOWN - 1u);
    return texel_share(x, y) * f32(ACROSS * DOWN) / (2.0 * PI * PI * max(sin(theta), 1e-6));
}
// Whether a light casts shadows, and how strongly it shows in shiny surfaces.
fn casts(l: Light) -> bool { return l.up.w >= 1.5; }
fn reflects(l: Light) -> f32 { return l.up.w - select(0.0, 2.0, l.up.w >= 1.5); }
fn power(a: f32, b: f32) -> f32 {
    return a * a / max(a * a + b * b, 1e-12);
}
// An area light's panel met by a ray: the distance to it and its glow there (0: missed).
fn panel_hit(l: Light, o: vec3<f32>, d: vec3<f32>) -> vec4<f32> {
    let facing = l.direction.xyz;
    let closing = dot(d, facing);
    if closing >= -1e-6 { return vec4<f32>(-1.0); }
    let t = dot(l.position.xyz - o, facing) / closing;
    if t <= 0.0 { return vec4<f32>(-1.0); }
    let off = o + d * t - l.position.xyz;
    let hw = length(l.right.xyz);
    let hh = length(l.up.xyz);
    let x = dot(off, l.right.xyz / hw) / hw;
    let y = dot(off, l.up.xyz / hh) / hh;
    if abs(x) > 1.0 || abs(y) > 1.0 { return vec4<f32>(-1.0); }
    return vec4<f32>(l.colour.rgb * feathered(l, x, y), t);
}
fn feathered(l: Light, x: f32, y: f32) -> f32 {
    let feather = l.right.w;
    if feather <= 0.0 { return 1.0; }
    return (1.0 - smoothstep(1.0 - feather, 1.0, abs(x))) * (1.0 - smoothstep(1.0 - feather, 1.0, abs(y)));
}
// The light every lamp, and the environment where it is bright, send straight to a point,
// each checked for what blocks it. On the `last` surface of a path no reflected ray
// follows, so what is drawn here counts in full.
fn direct(m: Shade, p: vec3<f32>, n: vec3<f32>, v: vec3<f32>, last: bool) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let lift = n * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
    if sampling_sky() {
        let s = sky_draw();
        let l = s.xyz;
        let nl = dot(n, l);
        if nl > 0.0 && s.w > 0.0 {
            let t = transmitted(p + lift, l, 1e30);
            var w = 1.0;
            if !last && t.w < 0.5 { w = power(s.w, draw_pdf(m, n, v, l)); }
            sum += brdf(m, n, v, l) * sky(l) * t.rgb * nl / s.w * w;
        }
    }
    for (var i = 0u; i < frame.counts.x; i++) {
        let light = light_at(i);
        let kind = light.position.w;
        if kind > 2.5 {
            // A point drawn on the panel; weighed against reflected rays meeting it.
            let x = random() * 2.0 - 1.0;
            let y = random() * 2.0 - 1.0;
            let q = light.position.xyz + light.right.xyz * x + light.up.xyz * y;
            let to = q - p;
            let d2 = max(dot(to, to), 1e-6);
            let dist = sqrt(d2);
            let l = to / dist;
            let cos_l = dot(-l, light.direction.xyz);
            if cos_l <= 0.0 || dot(n, l) <= 0.0 { continue; }
            let area = 4.0 * length(light.right.xyz) * length(light.up.xyz);
            let pdf = d2 / (area * cos_l);
            var t = vec4<f32>(1.0, 1.0, 1.0, 0.0);
            if casts(light) { t = transmitted(p + lift, l, dist * 0.999); }
            if max(max(t.r, t.g), t.b) <= 0.0 { continue; }
            var w = 1.0;
            if !last && t.w < 0.5 { w = power(pdf, draw_pdf(m, n, v, l)); }
            let f = brdf(m, n, v, l) - shiny * (1.0 - reflects(light));
            sum += f * light.colour.rgb * t.rgb * feathered(light, x, y) * dot(n, l) / pdf * w;
            continue;
        }
        var l = -light.direction.xyz;
        var radiance = light.colour.rgb;
        var dist = 1e30;
        if kind < 1.5 {
            let to = light.position.xyz - p;
            let d2 = max(dot(to, to), 1.0);
            dist = sqrt(d2);
            l = to / dist;
            radiance *= 1.0e6 / d2;
            if kind > 0.5 {
                radiance *= smoothstep(light.direction.w, light.colour.w, dot(-l, light.direction.xyz));
            }
        }
        let nl = dot(n, l);
        if nl <= 0.0 || dot(radiance, radiance) <= 0.0 { continue; }
        if casts(light) {
            radiance *= transmitted(p + lift, l, dist * 0.999).rgb;
            if max(max(radiance.r, radiance.g), radiance.b) <= 0.0 { continue; }
        }
        let f = brdf(m, n, v, l) - shiny * (1.0 - reflects(light));
        sum += f * radiance * nl * PI;
    }
    return sum;
}
// The nearest area panel a ray meets before `far`: its glow and distance, and its likelihood
// of having been drawn as a light sample from `p`.
fn panels(o: vec3<f32>, d: vec3<f32>, far: f32, reflected: bool) -> vec4<f32> {
    var best = vec4<f32>(0.0, 0.0, 0.0, far);
    for (var i = 0u; i < frame.counts.x; i++) {
        let light = light_at(i);
        if light.position.w < 2.5 { continue; }
        var h = panel_hit(light, o, d);
        if h.w > 0.0 && h.w < best.w {
            // Met in a reflection, a panel shows as strongly as its light's Reflections.
            if reflected { h = vec4<f32>(h.rgb * reflects(light), h.w); }
            best = h;
        }
    }
    return best;
}
fn panel_pdf(o: vec3<f32>, d: vec3<f32>, t: f32) -> f32 {
    for (var i = 0u; i < frame.counts.x; i++) {
        let light = light_at(i);
        if light.position.w < 2.5 { continue; }
        let h = panel_hit(light, o, d);
        if abs(h.w - t) <= 1e-3 * max(t, 1.0) {
            let area = 4.0 * length(light.right.xyz) * length(light.up.xyz);
            return t * t / (area * max(-dot(d, light.direction.xyz), 1e-6));
        }
    }
    return 0.0;
}
struct Seen { colour: vec3<f32>, alpha: f32 }
// How much of the light reaching a shadow catcher at `p` is blocked, as the interactive
// renderer weighs it: each light by its brightness there, and the environment's light
// by one direction drawn as a matte surface takes it.
fn catcher_shade(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let lift = n * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
    let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
    var direct = 0.0;
    var blocked_light = 0.0;
    for (var i = 0u; i < frame.counts.x; i++) {
        let light = light_at(i);
        var l = -light.direction.xyz;
        var radiance = light.colour.rgb;
        var dist = 1e30;
        if light.position.w > 2.5 {
            let x = random() * 2.0 - 1.0;
            let y = random() * 2.0 - 1.0;
            let to = light.position.xyz + light.right.xyz * x + light.up.xyz * y - p;
            let d2 = max(dot(to, to), 1.0);
            dist = sqrt(d2);
            l = to / dist;
            let front = max(dot(-l, light.direction.xyz), 0.0);
            radiance *= 4.0 * length(light.right.xyz) * length(light.up.xyz) * front / d2 / PI;
        } else if light.position.w < 1.5 {
            let to = light.position.xyz - p;
            let d2 = max(dot(to, to), 1.0);
            dist = sqrt(d2);
            l = to / dist;
            radiance *= 1.0e6 / d2;
            if light.position.w > 0.5 {
                radiance *= smoothstep(light.direction.w, light.colour.w, dot(-l, light.direction.xyz));
            }
        }
        let nl = dot(n, l);
        if nl <= 0.0 { continue; }
        let amount = dot(radiance, luma) * nl;
        direct += amount;
        if casts(light) { blocked_light += amount * (1.0 - dot(transmitted(p + lift, l, dist * 0.999).rgb, luma)); }
    }
    let ambient = dot(environment_layer(turned(n), 9u), luma) * frame.look.y;
    let basis = frame_of(n);
    let u1 = random();
    let phi = 2.0 * PI * random();
    let toward = basis * vec3<f32>(sqrt(u1) * cos(phi), sqrt(u1) * sin(phi), sqrt(max(1.0 - u1, 0.0)));
    var enclosed: f32;
    enclosed = ambient * (1.0 - dot(transmitted(p + lift, toward, 1e30).rgb, luma));
    return clamp((blocked_light + enclosed) / max(direct + ambient, 1e-4), 0.0, 1.0);
}
// One path from the camera through pixel `px`.
// What the denoiser is guided by, of the first surface a path meets that is not passed
// through (glass): its colour, its facing and how far along the path it is.
var<private> guide_albedo: vec3<f32>;
var<private> guide_facing: vec3<f32>;
var<private> guide_distance: f32;
var<private> guided: bool;
fn guide(albedo: vec3<f32>, facing: vec3<f32>, distance: f32) {
    if guided { return; }
    guided = true;
    guide_albedo = clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0));
    guide_facing = facing;
    guide_distance = distance;
}
fn path(px: vec2<f32>) -> Seen {
    // The camera's ray, through a point of the lens when the lens has an aperture.
    let f = frame.lens.x;
    var dir = normalize(vec3<f32>((px.x - frame.lens.y * 0.5) / f, (px.y - frame.lens.z * 0.5) / f, -1.0));
    var start = vec3<f32>(0.0);
    if frame.lens.w > 0.0 {
        let focus = dir * (frame.look.x / -dir.z);
        let r = frame.lens.w * sqrt(random());
        let phi = 2.0 * PI * random();
        start = vec3<f32>(r * cos(phi), r * sin(phi), 0.0);
        dir = normalize(focus - start);
    }
    var o = vec4<f32>(start, 1.0) * frame.camera;
    var d = normalize(vec4<f32>(dir, 0.0) * frame.camera);
    guided = false;
    guide_albedo = vec3<f32>(1.0);
    guide_facing = -d;
    guide_distance = 1e6;
    var travelled = 0.0;
    var colour = vec3<f32>(0.0);
    var through = vec3<f32>(1.0);
    var alpha = 0.0;
    var last_pdf = 0.0;
    var last_reflection = false;
    // Light reached by bending through glass after a lit surface: that surface's own light
    // samples already passed straight through the glass for it (`transmitted`).
    var shaded = false;
    var through_glass = false;
    let limit = frame.more.z;
    var bounce = 0u;
    // Rays passing through cards and catchers go on without using up a bounce, up to a limit.
    var steps = 0u;
    loop {
        if bounce > frame.counts.y || steps >= 64u { break; }
        steps += 1u;
        let h = scene_hit(o, d, 1e30, NEAREST);
        let lit = panels(o, d, h.t, bounce > 0u && last_reflection);
        if lit.w < h.t {
            if through_glass { break; }
            guide(vec3<f32>(1.0), -d, travelled + lit.w);
            // A panel's glow, met directly or in a reflection.
            var w = 1.0;
            if bounce > 0u { w = power(last_pdf, panel_pdf(o, d, lit.w)); }
            colour += through * lit.rgb * w;
            if bounce == 0u { alpha = 1.0; }
            break;
        }
        if h.triangle == 0xffffffffu {
            if bounce == 0u {
                if frame.look.w > 0.5 { colour += sky(d); alpha = 1.0; }
            } else {
                // Weighed against having been drawn as the environment's light.
                if through_glass && sampling_sky() { break; }
                var w = 1.0;
                if sampling_sky() { w = power(last_pdf, sky_pdf(d)); }
                var add = through * sky(d) * w;
                if limit > 0.0 { add = min(add, vec3<f32>(limit)); }
                colour += add;
            }
            break;
        }
        travelled += h.t;
        let inst = instance_at(h.instance);
        let s = surface_at(inst.surface);
        let k0 = triangles[h.triangle][0];
        let k1 = triangles[h.triangle][1];
        let k2 = triangles[h.triangle][2];
        let base = inst.vertex_base;
        let w0 = 1.0 - h.u - h.v;
        let a = vertices[2u * (base + k0)];
        let b = vertices[2u * (base + k1)];
        let c = vertices[2u * (base + k2)];
        let na = vec3<f32>(a.w, vertices[2u * (base + k0) + 1u].xy);
        let nb = vec3<f32>(b.w, vertices[2u * (base + k1) + 1u].xy);
        let nc = vec3<f32>(c.w, vertices[2u * (base + k2) + 1u].xy);
        // Normals go to the world by the inverse transpose: the world-to-object rows as
        // columns, times the normal.
        let to_world_normal = mat3x3<f32>(inst.to_object[0].xyz, inst.to_object[1].xyz, inst.to_object[2].xyz);
        var n = normalize(to_world_normal * (na * w0 + nb * h.u + nc * h.v));
        var g = normalize(to_world_normal * cross(b.xyz - a.xyz, c.xyz - a.xyz));
        let v = -d;
        // Met from outside the object (its normals point outwards), for glass.
        let outside = dot(n, v) > 0.0;
        // Two-sided: a face seen from behind shades as its other side.
        if dot(g, v) < 0.0 { g = -g; }
        if dot(n, g) < 0.0 { n = -n; }
        let p = o + d * h.t;
        let uv = uv_at(inst, h.triangle, h.u, h.v);
        let past = p + d * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
        if (s.flags & CARD) != 0u {
            // A card's picture, tinted, at its opacity: met in proportion to how opaque it
            // is there, else the ray goes on through.
            var c = sample_picture(s.texture, uv, false);
            c = vec4<f32>(c.rgb * s.colour, c.a) * s.opacity;
            if random() < c.a {
                guide(vec3<f32>(1.0), n, travelled);
                colour += through * c.rgb / max(c.a, 1e-6);
                if bounce == 0u { alpha = 1.0; }
                break;
            }
            o = past;
            continue;
        }
        if (s.flags & CATCHER) != 0u {
            // Seen directly, a catcher shows only its shadow; to other rays it is not there.
            if bounce == 0u {
                guide(vec3<f32>(1.0), n, travelled);
                alpha = clamp(catcher_shade(p, n) * clamp(s.shadow, 0.0, 8.0), 0.0, 1.0);
                break;
            }
            o = past;
            continue;
        }
        if bounce == 0u { alpha = 1.0; }
        var sc = s;
        // The point and its normal in the object's own space, for the finish's patterns.
        let local = a.xyz * w0 + b.xyz * h.u + c.xyz * h.v;
        let facing = na * w0 + nb * h.u + nc * h.v;
        if s.texture != 0xffffffffu && (s.flags & CUT) != 0u {
            let t = sample_picture(s.texture, uv, true);
            sc.colour *= t.rgb / max(t.a, 1e-4);
        } else if s.texture != 0xffffffffu && (s.flags & STRAIGHT) != 0u {
            sc.colour *= sample_picture(s.texture, uv, true).rgb;
        }
        if (s.flags & UNLIT) != 0u {
            guide(vec3<f32>(1.0), n, travelled);
            colour += through * sc.colour;
            break;
        }
        var m: Shade;
        var glow = vec3<f32>(0.0);
        if s.look != 0xffffffffu {
            // A library look: what the shared surface code says the surface is here, in
            // the camera's frame (a face looking at the camera faces +z, as in the preview).
            load_look(s.look);
            let to_camera = mat3x3<f32>(frame.camera[0].xyz, frame.camera[1].xyz, frame.camera[2].xyz);
            let mp = material_point(look_uv(uv), normalize(to_camera * n));
            let bent = normalize(transpose(to_camera) * mp.normal);
            if dot(bent, g) > 0.0 { n = bent; }
            if mp.glass > 0.5 {
                // The denoiser follows the glass's own surface: what is seen in it changes
                // from path to path (reflected or let through), its surface does not.
                guide(vec3<f32>(1.0), n, travelled);
                // Glass: reflected or refracted by Fresnel's share, about a facet drawn by
                // its roughness (frosted glass), tinted as the light goes in.
                let f0g = clamp(mp.f0.g, 0.0, 0.9);
                let ior = (1.0 + sqrt(f0g)) / max(1.0 - sqrt(f0g), 1e-4);
                var h = n;
                if mp.rough > 0.03 { h = facet(n, mp.rough * mp.rough); }
                if dot(h, v) <= 0.0 { h = n; }
                let fr = f0g + (1.0 - f0g) * pow(1.0 - max(dot(v, h), 0.0), 5.0);
                let bent_through = refract(-v, h, select(ior, 1.0 / ior, outside));
                let lift = 1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z));
                if random() < fr || dot(bent_through, bent_through) < 1e-8 {
                    d = reflect(-v, h);
                    o = p + g * lift;
                } else {
                    d = bent_through;
                    o = p - g * lift;
                    if outside { through *= studio.palette[0].rgb; }
                }
                // A mirror-like turn: whatever light it meets next counts in full (a likelihood
                // the power heuristic can still square).
                last_pdf = 1e15;
                last_reflection = true;
                if shaded { through_glass = true; }
                continue;
            }
            let a = max(mp.rough * mp.rough, 1e-4);
            var ay = a;
            var along = vec3<f32>(0.0);
            var across = vec3<f32>(0.0);
            if mp.brushed > 0.0 {
                // Brushed: rougher across the grain than along it.
                ay = clamp(a + mp.brushed * 0.12, 0.0, 1.0);
                let grain = transpose(to_camera) * mp.across;
                across = normalize(grain - n * dot(grain, n) + vec3<f32>(1e-5, 0.0, 0.0));
                along = normalize(cross(across, n));
            }
            let metal = clamp(mp.metal, 0.0, 1.0);
            m = Shade(mp.albedo, mp.f0, mp.albedo * (1.0 - metal), sqrt(a * ay), a, ay, along, across, mp.gain);
            glow = mp.emission;
        } else {
            let fin = finished(sc, local, facing, to_world_normal, n);
            m = fin.shade;
            n = fin.n;
        }
        guide(through * (m.diffuse + m.f0), n, travelled);
        shaded = true;
        through_glass = false;
        var add = through * direct(m, p, n, v, bounce == frame.counts.y);
        if s.emission > 0.0 { add += through * s.colour * s.emission; }
        add += through * glow;
        if limit > 0.0 && bounce > 0u { add = min(add, vec3<f32>(limit)); }
        colour += add;
        if bounce == frame.counts.y { break; }
        // On to the next bounce, drawn by the surface's own scatter.
        let l = draw(m, n, v);
        let nl = dot(n, l);
        if nl <= 0.0 || dot(g, l) <= 0.0 { break; }
        let pdf = draw_pdf(m, n, v, l);
        if pdf <= 1e-8 { break; }
        through *= brdf(m, n, v, l) * nl / pdf;
        last_pdf = pdf;
        last_reflection = drew_reflection;
        // Russian roulette after the second bounce: a weak path ends, a kept one makes up for it.
        if bounce >= 2u {
            let keep = clamp(max(max(through.r, through.g), through.b), 0.05, 0.95);
            if random() > keep { break; }
            through /= keep;
        }
        o = p + g * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
        d = l;
        bounce += 1u;
    }
    return Seen(colour, alpha);
}
// A surface colour in one word: the square root of each channel in 10 bits.
fn pack_colour(c: vec3<f32>) -> u32 {
    let q = vec3<u32>(round(sqrt(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0))) * 1023.0));
    return (q.r << 20u) | (q.g << 10u) | q.b;
}
fn unpack_colour(w: u32) -> vec3<f32> {
    let e = vec3<f32>(vec3<u32>(w >> 20u, w >> 10u, w) & vec3<u32>(1023u)) / 1023.0;
    return e * e;
}
// The facing as two numbers (octahedral), for packing.
fn octahedral(n: vec3<f32>) -> vec2<f32> {
    let p = n.xy / (abs(n.x) + abs(n.y) + abs(n.z) + 1e-12);
    if n.z >= 0.0 { return p; }
    let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
    return (1.0 - abs(p.yx)) * s;
}
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
@compute @workgroup_size(8, 8)
fn trace(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(frame.lens.y);
    let h = u32(frame.lens.z);
    if gid.x >= w || gid.y >= h { return; }
    let i = gid.y * w + gid.x;
    var sum = vec4<f32>(0.0);
    var albedo = vec3<f32>(0.0);
    var facing = vec3<f32>(0.0);
    var distance = 0.0;
    var squares = 0.0;
    let first = frame.counts.w;
    for (var k = 0u; k < frame.counts.z; k++) {
        rng = hash(i ^ hash((first + k) * 9781u + bitcast<u32>(frame.more.x)));
        let seen = path(vec2<f32>(f32(gid.x) + random(), f32(gid.y) + random()));
        // Premultiplied: what was met, over what was not.
        sum += vec4<f32>(seen.colour, seen.alpha);
        albedo += guide_albedo;
        facing += guide_facing;
        distance += guide_distance;
        let l = dot(seen.colour, LUMA);
        squares += l * l;
    }
    // The running mean of every sample so far: this pass's samples join the earlier ones.
    let total = f32(first + frame.counts.z);
    let mine = sum / total;
    if (frame.lights_at.y & 2u) != 0u {
        // Gathered to be denoised: the mean colour, and the mean guides beside it.
        var kept = vec4<f32>(0.0);
        var old_albedo = vec3<f32>(0.0);
        var old_facing = vec3<f32>(0.0);
        var old = vec4<f32>(0.0);
        let keep = f32(first) / total;
        if first > 0u {
            kept = picture[2u * i];
            old = picture[2u * i + 1u];
            let a = unpack_colour(bitcast<u32>(old.x));
            old_albedo = a;
            let e = unpack2x16snorm(bitcast<u32>(old.y));
            var n = vec3<f32>(e, 1.0 - abs(e.x) - abs(e.y));
            if n.z < 0.0 {
                let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), n.xy >= vec2<f32>(0.0));
                n = vec3<f32>((1.0 - abs(n.yx)) * s, n.z);
            }
            old_facing = normalize(n);
        }
        let a = old_albedo * keep + albedo / total;
        var n = old_facing * keep + facing / total;
        if dot(n, n) < 1e-12 { n = vec3<f32>(0.0, 0.0, 1.0); }
        picture[2u * i] = kept * keep + mine;
        picture[2u * i + 1u] = vec4<f32>(
            bitcast<f32>(pack_colour(a)),
            bitcast<f32>(pack2x16snorm(octahedral(normalize(n)))),
            old.z * keep + distance / total,
            old.w * keep + squares / total,
        );
        return;
    }
    let blend = frame.more.w;
    if blend < 1.0 {
        picture[i] = mix(picture[i], mine, blend);
    } else if first == 0u {
        picture[i] = mine;
    } else {
        picture[i] = picture[i] * (f32(first) / total) + mine;
    }
}
