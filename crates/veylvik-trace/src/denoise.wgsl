// Smoothing away a path traced picture's noise: an edge-avoiding à-trous filter (Dammertz
// et al. 2010) whose brightness test follows the noise measured at each pixel (as in
// SVGF). The light reaching a pixel (its colour over its surface colour, so textures are
// kept) is smoothed in five passes of widening reach, each neighbour weighed by how alike
// it is in facing, in distance (against how fast distance changes there) and in brightness
// (against the noise there); at the end it is multiplied by the surface colour again.
struct Stage {
    // width, height, reach of this pass in pixels, what the pass is (0 first, 1 smooth,
    // 2 smooth into the picture)
    size: vec4<u32>,
    // samples gathered, how far brightness may differ in noise's standard deviations,
    // how sharply facing must agree, how far distance may differ against its slope
    tune: vec4<f32>,
}
@group(0) @binding(0) var<uniform> stage: Stage;
// Two vec4s a pixel (trace.wgsl): the mean colour (premultiplied RGBA), and the guides:
// surface colour (packed), facing (packed), distance, mean squared brightness.
@group(0) @binding(1) var<storage, read> gathered: array<vec4<f32>>;
// The light reaching each pixel and its noise (variance of the mean brightness).
@group(0) @binding(2) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> into: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> picture: array<vec4<f32>>;
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// A surface colour in one word: the square root of each channel in 10 bits.
fn pack_colour(c: vec3<f32>) -> u32 {
    let q = vec3<u32>(round(sqrt(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0))) * 1023.0));
    return (q.r << 20u) | (q.g << 10u) | q.b;
}
fn unpack_colour(w: u32) -> vec3<f32> {
    let e = vec3<f32>(vec3<u32>(w >> 20u, w >> 10u, w) & vec3<u32>(1023u)) / 1023.0;
    return e * e;
}
fn albedo_at(i: u32) -> vec3<f32> {
    return max(unpack_colour(bitcast<u32>(gathered[2u * i + 1u].x)), vec3<f32>(0.02));
}
fn facing_at(i: u32) -> vec3<f32> {
    let e = unpack2x16snorm(bitcast<u32>(gathered[2u * i + 1u].y));
    var n = vec3<f32>(e, 1.0 - abs(e.x) - abs(e.y));
    if n.z < 0.0 {
        let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), n.xy >= vec2<f32>(0.0));
        n = vec3<f32>((1.0 - abs(n.yx)) * s, n.z);
    }
    return normalize(n);
}
fn distance_at(i: u32) -> f32 { return gathered[2u * i + 1u].z; }
fn index(x: i32, y: i32) -> u32 {
    let w = i32(stage.size.x);
    let h = i32(stage.size.y);
    return u32(clamp(y, 0, h - 1) * w + clamp(x, 0, w - 1));
}
@compute @workgroup_size(8, 8)
fn begin(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= stage.size.x || gid.y >= stage.size.y { return; }
    let i = gid.y * stage.size.x + gid.x;
    let c = gathered[2u * i];
    let a = albedo_at(i);
    let n = stage.tune.x;
    let mean = dot(c.rgb, LUMA);
    // The variance of the mean brightness: of one path, over the paths taken.
    var noise = max(gathered[2u * i + 1u].w - mean * mean, 0.0) / max(n - 1.0, 1.0);
    if n < 1.5 { noise = mean * mean + 1.0; }
    let al = max(dot(a, LUMA), 0.02);
    into[i] = vec4<f32>(c.rgb / a, noise / (al * al));
}
@compute @workgroup_size(8, 8)
fn widen(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= stage.size.x || gid.y >= stage.size.y { return; }
    let x = i32(gid.x);
    let y = i32(gid.y);
    let i = gid.y * stage.size.x + gid.x;
    let reach = i32(stage.size.z);
    let here = source[i];
    let n = facing_at(i);
    let z = distance_at(i);
    // How fast distance changes across this pixel.
    let slope = max(
        abs(distance_at(index(x + 1, y)) - distance_at(index(x - 1, y))),
        abs(distance_at(index(x, y + 1)) - distance_at(index(x, y - 1)))
    ) * 0.5;
    // The noise here, itself steadied over its neighbours.
    var noise = 0.0;
    let g3 = array<f32, 2>(0.5, 0.25);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            noise += source[index(x + dx, y + dy)].w * g3[abs(dx)] * g3[abs(dy)];
        }
    }
    let l = dot(here.rgb, LUMA);
    let spread = stage.tune.y * sqrt(max(noise, 0.0)) + 1e-6;
    let h = array<f32, 3>(0.375, 0.25, 0.0625);
    var sum = vec3<f32>(0.0);
    var noise_sum = 0.0;
    var weight = 0.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let qx = x + dx * reach;
            let qy = y + dy * reach;
            if qx < 0 || qy < 0 || qx >= i32(stage.size.x) || qy >= i32(stage.size.y) { continue; }
            let q = u32(qy) * stage.size.x + u32(qx);
            let there = source[q];
            var w = h[abs(dx)] * h[abs(dy)];
            if q != i {
                let apart = length(vec2<f32>(f32(dx * reach), f32(dy * reach)));
                let dz = abs(distance_at(q) - z);
                w *= exp(-dz / (stage.tune.w * slope * apart + 1e-3 * z + 1e-6));
                w *= pow(max(dot(facing_at(q), n), 0.0), stage.tune.z);
                w *= exp(-abs(dot(there.rgb, LUMA) - l) / spread);
            }
            sum += there.rgb * w;
            noise_sum += there.w * w * w;
            weight += w;
        }
    }
    let light = sum / weight;
    let left = noise_sum / (weight * weight);
    if stage.size.w == 2u {
        picture[i] = vec4<f32>(light * albedo_at(i), gathered[2u * i].a);
    } else {
        into[i] = vec4<f32>(light, left);
    }
}
