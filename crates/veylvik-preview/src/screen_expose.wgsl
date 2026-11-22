// The frame with exposure only, still scene-linear and premultiplied: what a host's display is given.
@group(0) @binding(1) var<storage, read> linear: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> exposed: array<vec4<f32>>;
@compute @workgroup_size(8, 8)
fn expose(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= settings.size.x || gid.y >= settings.size.y { return; }
    let i = gid.y * settings.size.x + gid.x;
    let pixel = linear[i];
    exposed[i] = vec4<f32>(pixel.rgb * settings.look.x, pixel.a);
}
