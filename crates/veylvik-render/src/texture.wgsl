struct Params { width: u32, height: u32, encoding: u32, padding: u32 }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(0) var<storage, read_write> output: array<vec4<f32>>;
@group(1) @binding(1) var<storage, read_write> invalid: atomic<u32>;
@group(1) @binding(2) var<uniform> params: Params;
fn finite(v: vec4<f32>) -> bool {
    return all((bitcast<vec4<u32>>(v) & vec4<u32>(0x7f800000u)) != vec4<u32>(0x7f800000u));
}
fn linear(v: vec3<f32>) -> vec3<f32> {
    return select(pow(max((v + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}
@compute @workgroup_size(8, 8)
fn import_texture(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.width || id.y >= params.height { return; }
    var p = textureLoad(source, vec2<i32>(id.xy), 0);
    if !finite(p) || p.a < 0.0 || p.a > 1.0 { atomicOr(&invalid, 1u); return; }
    if p.a == 0.0 { p = vec4<f32>(0.0); }
    else {
        if params.encoding == 3u { p = vec4<f32>(p.rgb / p.a, p.a); }
        if params.encoding >= 2u { p = vec4<f32>(linear(p.rgb), p.a); }
        if params.encoding != 0u { p = vec4<f32>(p.rgb * p.a, p.a); }
    }
    if !finite(p) { atomicOr(&invalid, 1u); }
    output[id.y * params.width + id.x] = p;
}
