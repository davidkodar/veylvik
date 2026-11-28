// Render passes for compositing, appended to trace.wgsl: 16 rays through each pixel on a
// 4 x 4 grid, without lighting or a lens, each meeting the nearest solid surface (a card
// only where its picture is at least half opaque, splats and volumes not at all).
const PASS_NORMALS: u32 = 1u;
const PASS_POSITION: u32 = 2u;
const PASS_MOTION: u32 = 3u;
const PASS_OBJECTS: u32 = 4u;
const PASS_OBJECTS_MORE: u32 = 5u;
struct PassHit { found: bool, p: vec3<f32>, n: vec3<f32>, instance: u32 }
fn pass_hit(start: vec3<f32>, d: vec3<f32>) -> PassHit {
    var o = start;
    for (var step = 0u; step < 64u; step++) {
        let h = scene_hit(o, d, 1e30, NEAREST);
        if h.triangle == 0xffffffffu { break; }
        let inst = instance_at(h.instance);
        let s = surface_at(inst.surface);
        let p = o + d * h.t;
        if (s.flags & CARD) != 0u
            && sample_picture(s.texture, uv_at(inst, h.triangle, h.u, h.v), false).a * s.opacity < 0.5 {
            o = p + d * (1e-3 + 1e-5 * max(max(abs(p.x), abs(p.y)), abs(p.z)));
            continue;
        }
        var n = smooth_normal(inst, h);
        if dot(n, d) > 0.0 { n = -n; }
        return PassHit(true, p, n, h.instance);
    }
    return PassHit(false, vec3<f32>(0.0), vec3<f32>(0.0), 0u);
}
fn rows_at(at: u32) -> mat3x4<f32> {
    return mat3x4<f32>(data[at], data[at + 1u], data[at + 2u]);
}
// Where a world point lands in the frame through camera `k` (0 a frame before, 1 now,
// 2 a frame after), in pixels, y down.
fn pass_pixel(k: u32, w: vec3<f32>) -> vec2<f32> {
    let q = vec4<f32>(w, 1.0) * rows_at(frame.pass_info.w + 3u * k);
    let f = frame.lens.x;
    let ahead = max(-q.z, 1e-6);
    return vec2<f32>(f * q.x / ahead + frame.lens.y * 0.5, f * q.y / ahead + frame.lens.z * 0.5);
}
fn pass_id(instance: u32) -> u32 {
    let at = frame.pass_info.y + instance / 4u;
    return bitcast<u32>(data[at][instance % 4u]);
}
@compute @workgroup_size(8, 8)
fn passes(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = u32(frame.lens.y);
    let height = u32(frame.lens.z);
    if gid.x >= width || gid.y >= height { return; }
    let kind = frame.pass_info.x;
    let f = frame.lens.x;
    let o = (vec4<f32>(0.0, 0.0, 0.0, 1.0) * frame.camera);
    // The middle four first: position and motion are those of the hit nearest the middle.
    let order = array<u32, 16>(5u, 10u, 6u, 9u, 0u, 1u, 2u, 3u, 4u, 7u, 8u, 11u, 12u, 13u, 14u, 15u);
    var ids: array<u32, 16>;
    var counts: array<u32, 16>;
    var kinds = 0u;
    var normal = vec3<f32>(0.0);
    var middle = PassHit(false, vec3<f32>(0.0), vec3<f32>(0.0), 0u);
    for (var j = 0u; j < 16u; j++) {
        let k = order[j];
        let px = vec2<f32>(f32(gid.x) + (f32(k % 4u) + 0.5) * 0.25, f32(gid.y) + (f32(k / 4u) + 0.5) * 0.25);
        let dir = normalize(vec3<f32>((px.x - frame.lens.y * 0.5) / f, (px.y - frame.lens.z * 0.5) / f, -1.0));
        let d = normalize(vec4<f32>(dir, 0.0) * frame.camera);
        let hit = pass_hit(o, d);
        if !hit.found { continue; }
        if !middle.found { middle = hit; }
        normal += hit.n;
        if kind >= PASS_OBJECTS {
            let id = pass_id(hit.instance);
            var slot = kinds;
            for (var m = 0u; m < kinds; m++) {
                if ids[m] == id { slot = m; break; }
            }
            if slot == kinds { ids[slot] = id; counts[slot] = 0u; kinds += 1u; }
            counts[slot] += 1u;
        }
    }
    var out = vec4<f32>(0.0);
    if middle.found {
        if kind == PASS_NORMALS {
            out = vec4<f32>(normalize(normal), 1.0);
        } else if kind == PASS_POSITION {
            out = vec4<f32>(middle.p, 1.0);
        } else if kind == PASS_MOTION {
            // The point where it is on its object, a frame before and after.
            let inst = instance_at(middle.instance);
            let local = vec4<f32>(middle.p, 1.0) * inst.to_object;
            let moved = frame.pass_info.z + 6u * middle.instance;
            let before = vec4<f32>(local, 1.0) * rows_at(moved);
            let after = vec4<f32>(local, 1.0) * rows_at(moved + 3u);
            let now = pass_pixel(1u, middle.p);
            let forward = pass_pixel(2u, after) - now;
            let backward = pass_pixel(0u, before) - now;
            // x right and y up, as compositing programs read motion.
            out = vec4<f32>(forward.x, -forward.y, backward.x, -backward.y);
        } else {
            // The objects covering most, each with its share: two ranks a picture.
            let skip = select(0u, 2u, kind == PASS_OBJECTS_MORE);
            var ranked = array<vec2<f32>, 2>(vec2<f32>(0.0), vec2<f32>(0.0));
            for (var r = 0u; r < skip + 2u; r++) {
                var best = 16u;
                for (var m = 0u; m < kinds; m++) {
                    if counts[m] == 0u { continue; }
                    if best == 16u || counts[m] > counts[best] || (counts[m] == counts[best] && ids[m] < ids[best]) {
                        best = m;
                    }
                }
                if best == 16u { break; }
                if r >= skip {
                    ranked[r - skip] = vec2<f32>(bitcast<f32>(ids[best]), f32(counts[best]) / 16.0);
                }
                counts[best] = 0u;
            }
            out = vec4<f32>(ranked[0].x, ranked[0].y, ranked[1].x, ranked[1].y);
        }
    }
    picture[gid.y * width + gid.x] = out;
}
