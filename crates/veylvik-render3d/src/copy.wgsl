// Resolved colour to the frame's premultiplied rows, through the camera's depth of
// field, blended in for motion blur samples.
struct Lens {
    // f^2 / N * pixels per millimetre / 2 (0: no depth of field), focal length,
    // focus distance (0: the frame centre), nearest scene distance
    optics: vec4<f32>,
    // largest blur radius (px), blend into the frame, depth constant, unused
    limits: vec4<f32>,
}
@group(0) @binding(0) var resolved: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> frame: array<vec4<f32>>;
@group(0) @binding(2) var depth: texture_depth_multisampled_2d;
@group(0) @binding(3) var<uniform> lens: Lens;

// Distance from the camera (reversed infinite depth: d = near / distance; 0 = sky).
fn distance_at(p: vec2<i32>) -> f32 {
    let d = textureLoad(depth, p, 0);
    if d <= 0.0 { return 1.0e9; }
    return lens.limits.z / d;
}
// Blur radius in pixels of a point at `distance` for a lens focused at `focus`.
fn blur(distance: f32, focus: f32, k: f32) -> f32 {
    return min(k * abs(1.0 - focus / distance), lens.limits.x);
}
@compute @workgroup_size(8, 8)
fn copy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(resolved);
    if gid.x >= size.x || gid.y >= size.y { return; }
    let p = vec2<i32>(gid.xy);
    var colour = textureLoad(resolved, p, 0);
    if lens.optics.x > 0.0 {
        var focus = lens.optics.z;
        if focus <= 0.0 { focus = distance_at(vec2<i32>(size / 2u)); }
        focus = min(focus, 1.0e8);
        let k = lens.optics.x / max(focus - lens.optics.y, 1.0);
        let reach = min(lens.limits.x, max(k, k * abs(1.0 - focus / lens.optics.w)));
        if reach >= 0.5 {
            // Scatter-as-gather along a golden-angle spiral: a neighbour adds its
            // colour where its own blur reaches this pixel; neighbours behind this
            // one cannot blur over it by more than twice its own blur.
            let here = distance_at(p);
            let own = blur(here, focus, k);
            let step = max(0.5, reach / 20.0);
            let last = vec2<i32>(size) - 1;
            var sum = colour;
            var count = 1.0;
            var r = step;
            var angle = 0.0;
            for (var i = 0; i < 2048 && r < reach; i++) {
                let q = clamp(p + vec2<i32>(round(vec2<f32>(cos(angle), sin(angle)) * r)), vec2<i32>(0), last);
                let there = distance_at(q);
                var size_there = blur(there, focus, k);
                if there > here { size_there = min(size_there, own * 2.0); }
                let m = smoothstep(r - 0.5, r + 0.5, size_there);
                sum += mix(sum / count, textureLoad(resolved, q, 0), m);
                count += 1.0;
                r += step / r;
                angle += 2.39996323;
            }
            colour = sum / count;
        }
    }
    let i = gid.y * size.x + gid.x;
    if lens.limits.y >= 1.0 {
        frame[i] = colour;
    } else {
        frame[i] = mix(frame[i], colour, lens.limits.y);
    }
}
