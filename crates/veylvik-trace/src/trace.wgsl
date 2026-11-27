// Path tracing over the packed scene (see scene.rs): the camera's rays walk a top tree to
// placed meshes and each mesh's own tree; lights are sampled directly and area panels are
// also met by reflected rays, the two weighed against each other (multiple importance
// sampling). Scene frame: x right, y down, z towards the viewer; scene-linear Rec.709.
// The environment lookups (`environment_specular`) are appended from the renderer's
// shared WGSL and read `environment_map`.
struct Node { min: vec3<f32>, first: u32, max: vec3<f32>, count: u32 }
struct Instance { to_object: mat3x4<f32>, to_world: mat3x4<f32>, root: u32, triangle_base: u32, vertex_base: u32, surface: u32 }
struct Surface { colour: vec3<f32>, roughness: f32, metallic: f32, emission: f32, opacity: f32, flags: u32 }
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
fn bits(v: vec4<f32>) -> vec4<u32> { return bitcast<vec4<u32>>(v); }
fn instance_at(i: u32) -> Instance {
    let b = frame.offsets.x + 7u * i;
    let k = bits(data[b + 6u]);
    return Instance(mat3x4<f32>(data[b], data[b + 1u], data[b + 2u]), mat3x4<f32>(data[b + 3u], data[b + 4u], data[b + 5u]), k.x, k.y, k.z, k.w);
}
fn surface_at(i: u32) -> Surface {
    let b = frame.offsets.y + 2u * i;
    let a = data[b];
    let c = data[b + 1u];
    return Surface(a.xyz, a.w, c.x, c.y, c.z, bitcast<u32>(c.w));
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
// The nearest surface along a world ray nearer than `far`; with `stop_at_first`, the first found
// that blocks light (unlit cards and shadow catchers do not).
fn scene_hit(o: vec3<f32>, d: vec3<f32>, far: f32, stop_at_first: bool) -> Hit {
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
                if stop_at_first && (surface_at(inst.surface).flags & 3u) != 0u { continue; }
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
    return scene_hit(o, d, far, true).triangle != 0xffffffffu;
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
struct Shade { base: vec3<f32>, f0: vec3<f32>, diffuse: vec3<f32>, a: f32 }
fn shade_of(s: Surface) -> Shade {
    let polish = clamp(s.roughness, 0.03, 1.0);
    let a = polish * polish;
    let metal = clamp(s.metallic, 0.0, 1.0);
    return Shade(s.colour, mix(vec3<f32>(0.04), s.colour, metal), s.colour * (1.0 - metal), a);
}
// What reaches the eye along `v` of light arriving along `l`, per unit of light and
// cosine, as the rasteriser's lights are (their radiance taken with a factor of pi).
fn brdf(m: Shade, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nl = dot(n, l);
    let nv = max(dot(n, v), 1e-4);
    if nl <= 0.0 { return vec3<f32>(0.0); }
    let h = normalize(l + v);
    let f = m.f0 + (vec3<f32>(1.0) - m.f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
    let specular = ggx(max(dot(n, h), 0.0), m.a) * smith(nv, nl, m.a) * f / max(4.0 * nv * nl, 1e-4);
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
    if random() < specular_share(m, n, v) {
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
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}
fn sky(d: vec3<f32>) -> vec3<f32> {
    return environment_specular(turned(d), 0.0) * frame.look.y;
}
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
// The light every lamp sends straight to a point, each checked for what blocks it.
fn direct(m: Shade, p: vec3<f32>, n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let lift = n * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
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
            if blocked(p + lift, l, dist * 0.999) { continue; }
            let w = power(pdf, draw_pdf(m, n, v, l));
            sum += brdf(m, n, v, l) * light.colour.rgb * feathered(light, x, y) * dot(n, l) / pdf * w;
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
        if blocked(p + lift, l, dist * 0.999) { continue; }
        sum += brdf(m, n, v, l) * radiance * nl * PI;
    }
    return sum;
}
// The nearest area panel a ray meets before `far`: its glow and distance, and its likelihood
// of having been drawn as a light sample from `p`.
fn panels(o: vec3<f32>, d: vec3<f32>, far: f32) -> vec4<f32> {
    var best = vec4<f32>(0.0, 0.0, 0.0, far);
    for (var i = 0u; i < frame.counts.x; i++) {
        let light = light_at(i);
        if light.position.w < 2.5 { continue; }
        let h = panel_hit(light, o, d);
        if h.w > 0.0 && h.w < best.w {
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
// One path from the camera through pixel `px`.
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
    var colour = vec3<f32>(0.0);
    var through = vec3<f32>(1.0);
    var alpha = 0.0;
    var last_pdf = 0.0;
    let limit = frame.more.z;
    for (var bounce = 0u; bounce <= frame.counts.y; bounce++) {
        let h = scene_hit(o, d, 1e30, false);
        let lit = panels(o, d, h.t);
        if lit.w < h.t {
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
                var add = through * sky(d);
                if limit > 0.0 { add = min(add, vec3<f32>(limit)); }
                colour += add;
            }
            break;
        }
        if bounce == 0u { alpha = 1.0; }
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
        // Two-sided: a face seen from behind shades as its other side.
        if dot(g, v) < 0.0 { g = -g; }
        if dot(n, g) < 0.0 { n = -n; }
        let p = o + d * h.t;
        if (s.flags & 1u) != 0u {
            colour += through * s.colour;
            break;
        }
        let m = shade_of(s);
        var add = through * direct(m, p, n, v);
        if s.emission > 0.0 { add += through * s.colour * s.emission; }
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
        // Russian roulette after the second bounce: a weak path ends, a kept one makes up for it.
        if bounce >= 2u {
            let keep = clamp(max(max(through.r, through.g), through.b), 0.05, 0.95);
            if random() > keep { break; }
            through /= keep;
        }
        o = p + g * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
        d = l;
    }
    return Seen(colour, alpha);
}
@compute @workgroup_size(8, 8)
fn trace(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(frame.lens.y);
    let h = u32(frame.lens.z);
    if gid.x >= w || gid.y >= h { return; }
    let i = gid.y * w + gid.x;
    var sum = vec4<f32>(0.0);
    let first = frame.counts.w;
    for (var k = 0u; k < frame.counts.z; k++) {
        rng = hash(i ^ hash((first + k) * 9781u + bitcast<u32>(frame.more.x)));
        let seen = path(vec2<f32>(f32(gid.x) + random(), f32(gid.y) + random()));
        // Premultiplied: what was met, over what was not.
        sum += vec4<f32>(seen.colour, seen.alpha);
    }
    // The running mean of every sample so far: this pass's samples join the earlier ones.
    let total = f32(first + frame.counts.z);
    let mine = sum / total;
    let blend = frame.more.w;
    if blend < 1.0 {
        picture[i] = mix(picture[i], mine, blend);
    } else if first == 0u {
        picture[i] = mine;
    } else {
        picture[i] = picture[i] * (f32(first) / total) + mine;
    }
}
