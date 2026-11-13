struct Settings { size: vec2<u32>, reference: vec2<i32>, predicted: vec2<i32>, radius: i32, search: i32 }
@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var<storage, read> reference: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> current: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> scores: array<f32>;
fn luminance(pixel: vec4<f32>) -> f32 { return dot(pixel.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
@compute @workgroup_size(64)
fn correlate(@builtin(global_invocation_id) id: vec3<u32>) {
    let side = u32(settings.search * 2 + 1);
    if id.x >= side * side { return; }
    let center = settings.predicted + vec2<i32>(i32(id.x % side), i32(id.x / side)) - vec2<i32>(settings.search);
    let r = settings.radius;
    if any(center - vec2<i32>(r) < vec2<i32>(0)) || any(center + vec2<i32>(r) >= vec2<i32>(settings.size)) { scores[id.x] = -2.0; return; }
    var sum_a = 0.0; var sum_b = 0.0; var aa = 0.0; var bb = 0.0; var ab = 0.0;
    for (var y = -r; y <= r; y++) {
        for (var x = -r; x <= r; x++) {
            let pa = settings.reference + vec2<i32>(x, y);
            let pb = center + vec2<i32>(x, y);
            let a = luminance(reference[u32(pa.y) * settings.size.x + u32(pa.x)]);
            let b = luminance(current[u32(pb.y) * settings.size.x + u32(pb.x)]);
            sum_a += a; sum_b += b; aa += a*a; bb += b*b; ab += a*b;
        }
    }
    let count = f32((2*r+1)*(2*r+1));
    let va = max(0.0, aa - sum_a*sum_a/count); let vb = max(0.0, bb - sum_b*sum_b/count);
    if min(va, vb) < count * 0.000001 { scores[id.x] = -2.0; return; }
    scores[id.x] = clamp((ab - sum_a*sum_b/count) / sqrt(va*vb), -1.0, 1.0);
}
