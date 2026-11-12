@group(0) @binding(0) var<storage, read> pixels: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> invalid: atomic<u32>;

@compute @workgroup_size(8, 8)
fn validate(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = id.x + id.y * groups.x * 8u;
    if index >= arrayLength(&pixels) { return; }
    // IEEE exponent bits identify both infinity and NaN without floating-point
    // comparison optimizations hiding non-finite values.
    let exponent = bitcast<vec4<u32>>(pixels[index]) & vec4<u32>(0x7f800000u);
    if any(exponent == vec4<u32>(0x7f800000u)) { atomicOr(&invalid, 1u); }
}
