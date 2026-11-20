@group(0) @binding(1) var<storage, read> linear: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> packed: array<u32>;
@compute @workgroup_size(8, 8)
fn pack(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= settings.size.x || gid.y >= settings.size.y { return; }
    let i = gid.y * settings.size.x + gid.x;
    let pixel = linear[i];
    let alpha = clamp(pixel.a, 0.0, 1.0);
    var color = vec3<f32>(0.0);
    if alpha > 0.0 {
        if settings.look.y != 0.0 { color = filmic(pixel.rgb * settings.look.x / alpha) * alpha; }
        else { color = clamp(pixel.rgb * settings.look.x / alpha, vec3<f32>(0.0), vec3<f32>(1.0)) * alpha; }
    }
    let code = vec4<u32>(round(vec4<f32>(srgb(color), alpha) * 255.0));
    packed[i] = code.r | (code.g << 8u) | (code.b << 16u) | (code.a << 24u);
}
