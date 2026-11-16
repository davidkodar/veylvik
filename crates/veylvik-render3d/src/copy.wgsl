// Resolved colour to the frame's premultiplied rows.
@group(0) @binding(0) var resolved: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> frame: array<vec4<f32>>;
@compute @workgroup_size(8, 8)
fn copy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(resolved);
    if gid.x >= size.x || gid.y >= size.y { return; }
    frame[gid.y * size.x + gid.x] = textureLoad(resolved, vec2<i32>(gid.xy), 0);
}
