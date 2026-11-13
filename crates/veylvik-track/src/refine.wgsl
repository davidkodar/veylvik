// Direct alignment of the tracking area: Gauss-Newton normal equations for plane motion
// plus photometric gain and bias, Huber-weighted. Each thread accumulates its share of
// the area's pixels; the host sums threads in double precision and solves.
struct Params {
    size: vec2<u32>, threads: u32, stride: u32,
    center: vec2<f32>, scale: f32, cols: u32,
    // Motion h0..h7 in centred coordinates scaled by `scale` (h8 = 1).
    h0: vec4<f32>, h1: vec4<f32>,
    // Gain, bias, Huber threshold, pixel count.
    photometric: vec4<f32>,
    quad0: vec4<f32>, quad1: vec4<f32>,
    // Sampled bounding box in reference pixels: min x, min y, max x, max y.
    bbox: vec4<f32>,
}
@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> reference: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> current: array<vec4<f32>>;
// One-element buffer means no matte; alpha above one half excludes a current pixel.
@group(0) @binding(3) var<storage, read> occlusion: array<vec4<f32>>;
// threads × 67: 55 upper-triangle normal-matrix terms, 10 gradient terms, error, count.
@group(0) @binding(4) var<storage, read_write> sums: array<f32>;

fn luma(p: vec4<f32>) -> f32 { return dot(p.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
fn cur(p: vec2<f32>) -> f32 {
    let a = vec2<u32>(floor(p)); let t = fract(p); let w = P.size.x;
    return mix(mix(luma(current[a.y*w+a.x]), luma(current[a.y*w+a.x+1u]), t.x),
               mix(luma(current[(a.y+1u)*w+a.x]), luma(current[(a.y+1u)*w+a.x+1u]), t.x), t.y);
}
fn corner(i: u32) -> vec2<f32> {
    switch i { case 0u: { return P.quad0.xy; } case 1u: { return P.quad0.zw; }
    case 2u: { return P.quad1.xy; } default: { return P.quad1.zw; } }
}
fn inside_quad(p: vec2<f32>) -> bool {
    var positive = false; var negative = false;
    for (var i = 0u; i < 4u; i++) {
        let a = corner(i); let b = corner((i+1u)%4u);
        let d = (b.x-a.x)*(p.y-a.y)-(b.y-a.y)*(p.x-a.x);
        positive = positive || d > 0.0; negative = negative || d < 0.0;
    }
    return !(positive && negative);
}
fn occluded(q: vec2<f32>) -> bool {
    if arrayLength(&occlusion) < P.size.x*P.size.y { return false; }
    let c = vec2<u32>(round(q));
    return occlusion[c.y*P.size.x+c.x].a > 0.5;
}

@compute @workgroup_size(64)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= P.threads { return; }
    var acc: array<f32, 67>;
    for (var i = 0u; i < 67u; i++) { acc[i] = 0.0; }
    let total = u32(P.photometric.w);
    let size = vec2<f32>(P.size);
    for (var k = id.x; k < total; k += P.threads) {
        let p = P.bbox.xy + vec2<f32>(f32(k % P.cols), f32(k / P.cols)) * f32(P.stride);
        if any(p > P.bbox.zw) || !inside_quad(p) { continue; }
        let n = (p - P.center) / P.scale;
        let w = P.h1.z*n.x + P.h1.w*n.y + 1.0;
        if w <= 1e-6 { continue; }
        let qn = vec2<f32>(P.h0.x*n.x + P.h0.y*n.y + P.h0.z, P.h0.w*n.x + P.h1.x*n.y + P.h1.y) / w;
        let q = qn * P.scale + P.center;
        if any(q < vec2<f32>(1.0)) || any(q > size - vec2<f32>(3.0)) || occluded(q) { continue; }
        let reference_luma = luma(reference[u32(p.y)*P.size.x + u32(p.x)]);
        let current_luma = cur(q);
        let gain = P.photometric.x;
        let e = gain*current_luma + P.photometric.y - reference_luma;
        // Image gradient with respect to the centred, scaled coordinates.
        let g = vec2<f32>(cur(q + vec2<f32>(1.0, 0.0)) - cur(q - vec2<f32>(1.0, 0.0)),
                          cur(q + vec2<f32>(0.0, 1.0)) - cur(q - vec2<f32>(0.0, 1.0))) * 0.5 * P.scale * gain;
        let r = -(g.x*qn.x + g.y*qn.y);
        var j: array<f32, 10>;
        j[0] = g.x*n.x/w; j[1] = g.x*n.y/w; j[2] = g.x/w;
        j[3] = g.y*n.x/w; j[4] = g.y*n.y/w; j[5] = g.y/w;
        j[6] = r*n.x/w; j[7] = r*n.y/w;
        j[8] = current_luma; j[9] = 1.0;
        let huber = P.photometric.z;
        let weight = select(huber / abs(e), 1.0, abs(e) <= huber);
        var t = 0u;
        for (var a = 0u; a < 10u; a++) {
            for (var b = a; b < 10u; b++) { acc[t] += weight*j[a]*j[b]; t++; }
        }
        for (var a = 0u; a < 10u; a++) { acc[55u+a] += weight*j[a]*e; }
        acc[65] += weight*e*e;
        acc[66] += 1.0;
    }
    for (var i = 0u; i < 67u; i++) { sums[id.x*67u + i] = acc[i]; }
}
