// Solids particles bounce off (see nodes/particle_solids.rs): after the surface data,
// how many, then each solid's kind, half sizes, scale and distance map, and its
// world-to-own rows at the ROWS moments of the last life; then the texts' maps.
const SOLID_HEAD = 12u;
fn solid_value(k: u32) -> f32 { return surface_at(u32(camera_row(114u)) + k); }
fn solid_count() -> u32 {
    if camera_row(115u) < 0.5 { return 0u; }
    return u32(solid_value(0u));
}
fn solid_base(i: u32) -> u32 { return 1u + i * (SOLID_HEAD + ROWS * 12u); }
// Solid `i`'s world-to-own rows at the moment `t` (seconds), between the two kept
// moments round it.
fn solid_rows(i: u32, t: f32) -> array<vec4<f32>, 3> {
    let life = p.extra.w;
    let row = clamp((t - (bitcast<f32>(p.size_op.z) - life)) / life, 0.0, 1.0) * f32(ROWS - 1u);
    let a = u32(floor(row)); let b = min(a + 1u, ROWS - 1u); let f = row - floor(row);
    let ma = solid_base(i) + SOLID_HEAD + a * 12u; let mb = solid_base(i) + SOLID_HEAD + b * 12u;
    var m: array<vec4<f32>, 3>;
    for (var r = 0u; r < 3u; r++) {
        let k = 4u * r;
        m[r] = mix(vec4<f32>(solid_value(ma + k), solid_value(ma + k + 1u), solid_value(ma + k + 2u), solid_value(ma + k + 3u)),
                   vec4<f32>(solid_value(mb + k), solid_value(mb + k + 1u), solid_value(mb + k + 2u), solid_value(mb + k + 3u)), f);
    }
    return m;
}
// Outside distance on two axes joined (as a box's corner).
fn joined(d: vec2<f32>) -> f32 { return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0); }
// A text's footprint distance at a point of its own plane: the map, and beyond it the
// way to the map's edge.
fn text_footprint(base: u32, q: vec2<f32>) -> f32 {
    let start = u32(camera_row(114u)) + u32(solid_value(base + 5u));
    let w = solid_value(base + 6u); let h = solid_value(base + 7u);
    let corner = vec2<f32>(solid_value(base + 8u), solid_value(base + 9u)); let cell = solid_value(base + 10u);
    let uv = (q - corner) / cell - 0.5;
    let inside = clamp(uv, vec2<f32>(0.0), vec2<f32>(w - 1.0, h - 1.0));
    let c = floor(min(inside, vec2<f32>(w - 2.0, h - 2.0))); let f = inside - c;
    let at = start + u32(c.y) * u32(w) + u32(c.x);
    let top = mix(surface_at(at), surface_at(at + 1u), f.x);
    let bottom = mix(surface_at(at + u32(w)), surface_at(at + u32(w) + 1u), f.x);
    return mix(top, bottom, f.y) + length(uv - inside) * cell;
}
// How far a point of solid `i`'s own space is from its surface, in its own units
// (negative inside; never more than the true distance).
fn solid_own(i: u32, q: vec3<f32>) -> f32 {
    let base = solid_base(i);
    let kind = u32(solid_value(base));
    let h = vec3<f32>(solid_value(base + 1u), solid_value(base + 2u), solid_value(base + 3u));
    switch kind {
        case 0u: {
            let d = abs(q) - h;
            return length(max(d, vec3<f32>(0.0))) + min(max(d.x, max(d.y, d.z)), 0.0);
        }
        case 1u: { return (length(q / max(h, vec3<f32>(1e-3))) - 1.0) * min(h.x, min(h.y, h.z)); }
        case 2u: {
            let across = (length(q.xz / max(h.xz, vec2<f32>(1e-3))) - 1.0) * min(h.x, h.z);
            return joined(vec2<f32>(across, abs(q.y) - h.y));
        }
        case 3u: {
            // Widest at y = +h.y, a point at -h.y.
            let m = min(h.x, h.z); let tall = 2.0 * max(h.y, 1e-3);
            let radial = length(q.xz / max(h.xz, vec2<f32>(1e-3))) * m;
            let side = (radial - m * (q.y + h.y) / tall) * tall / sqrt(tall * tall + m * m);
            return max(side, abs(q.y) - h.y);
        }
        case 4u: { return length(vec2<f32>(length(q.xy) - h.x, q.z)) - h.y; }
        case 5u: { return joined(vec2<f32>(text_footprint(base, q.xy), abs(q.z) - h.x)); }
        default: {
            let r = length(q.xy);
            return joined(vec2<f32>(max(r - h.x, h.y - r), abs(q.z) - 1.0));
        }
    }
}
// How far a point of the scene is from solid `i` at the moment `t`, in pixels.
fn solid_distance(i: u32, at: vec3<f32>, t: f32) -> f32 {
    let m = solid_rows(i, t);
    let q = vec3<f32>(dot(m[0], vec4<f32>(at, 1.0)), dot(m[1], vec4<f32>(at, 1.0)), dot(m[2], vec4<f32>(at, 1.0)));
    return solid_own(i, q) * solid_value(solid_base(i) + 4u);
}
// The nearest solid to a point at `t`: its distance, and which one.
struct Nearest { distance: f32, solid: u32 }
fn nearest_solid(at: vec3<f32>, t: f32) -> Nearest {
    var n = Nearest(1e30, 0u);
    for (var i = 0u; i < min(solid_count(), 16u); i++) {
        let d = solid_distance(i, at, t);
        if d < n.distance { n = Nearest(d, i); }
    }
    return n;
}
// The way out of solid `i` at a point, in the scene.
fn solid_out(i: u32, at: vec3<f32>, t: f32) -> vec3<f32> {
    let e = 0.75;
    let g = vec3<f32>(
        solid_distance(i, at + vec3<f32>(e, 0.0, 0.0), t) - solid_distance(i, at - vec3<f32>(e, 0.0, 0.0), t),
        solid_distance(i, at + vec3<f32>(0.0, e, 0.0), t) - solid_distance(i, at - vec3<f32>(0.0, e, 0.0), t),
        solid_distance(i, at + vec3<f32>(0.0, 0.0, e), t) - solid_distance(i, at - vec3<f32>(0.0, 0.0, e), t));
    if dot(g, g) < 1e-12 { return vec3<f32>(0.0, -1.0, 0.0); }
    return normalize(g);
}
// Where a move from `at` to `next` at the moment `t` first comes within a pixel of a
// solid: x < 0 when it does not; else how far along it (0 to 1) and which solid.
fn solid_hit(at: vec3<f32>, next: vec3<f32>, t: f32) -> vec2<f32> {
    let way = next - at; let far = length(way);
    var gone = 0.0;
    for (var k = 0u; k < 24u; k++) {
        let probe = at + way * (gone / max(far, 1e-6));
        let n = nearest_solid(probe, t);
        if n.distance < 1.0 { return vec2<f32>(gone / max(far, 1e-6), f32(n.solid)); }
        if gone >= far { break; }
        gone = min(gone + max(n.distance - 0.5, 0.5), far);
    }
    return vec2<f32>(-1.0, 0.0);
}
