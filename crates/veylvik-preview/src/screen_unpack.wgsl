@group(0) @binding(1) var<storage, read> words: array<u32>;
@group(0) @binding(2) var<storage, read_write> restored: array<vec4<f32>>;
@compute @workgroup_size(8, 8)
fn unpack(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= settings.size.x || gid.y >= settings.size.y { return; }
    let i = gid.y * settings.size.x + gid.x;
    let word = words[i];
    let code = vec4<f32>(vec4<u32>(word & 255u, (word >> 8u) & 255u, (word >> 16u) & 255u, word >> 24u)) / 255.0;
    restored[i] = vec4<f32>(srgb_inverse(code.rgb), code.a);
}
