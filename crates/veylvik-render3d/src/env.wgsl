// A panorama turned into the studio environment's layout: level 0 is 1024x512, each
// further level halves it (a 2x2 average, so rougher reflections see a softer sky),
// and layer 9 (64x32, after level 8) repeats level 4 for diffuse light.
struct EnvParams {
    // source width, source height, level being written, 1 when the source is a
    // frame of the shot instead of a panorama
    source: vec4<u32>,
}
@group(0) @binding(0) var<uniform> env_params: EnvParams;
@group(0) @binding(1) var<storage, read> env_source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> env_target: array<vec4<f32>>;
fn env_size(level: u32) -> vec2<u32> {
    return vec2<u32>(1024u, 512u) >> vec2<u32>(level);
}
fn env_offset(level: u32) -> u32 {
    var offset = 0u;
    for (var i = 0u; i < level; i++) {
        let e = env_size(i);
        offset += e.x * e.y;
    }
    return offset;
}
fn env_texel(x: i32, y: i32) -> vec3<f32> {
    let w = i32(env_params.source.x);
    let h = i32(env_params.source.y);
    // Wrap around horizontally, as a panorama does (a frame stops at its edges);
    // clamp at the poles.
    var xx = ((x % w) + w) % w;
    if env_params.source.w == 1u { xx = clamp(x, 0, w - 1); }
    let yy = clamp(y, 0, h - 1);
    let c = env_source[u32(yy * w + xx)];
    return c.rgb / max(c.a, 1e-4) * step(1e-4, c.a);
}
@compute @workgroup_size(8, 8)
fn resample(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= 1024u || gid.y >= 512u { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(1024.0, 512.0);
    var q = uv * vec2<f32>(env_params.source.xy) - 0.5;
    if env_params.source.w == 1u {
        // A frame of the shot: this texel's direction (x right, y down, as
        // environment_layer reads it) looks at the picture as the camera did, its
        // middle straight ahead and its edges a right angle off, the same behind.
        let phi = (uv.x - 0.5) * 6.2831853;
        let theta = uv.y * 3.14159265;
        let d = vec2<f32>(sin(theta) * cos(phi), -cos(theta));
        q = (vec2<f32>(0.5) + 0.5 * d) * vec2<f32>(env_params.source.xy) - 0.5;
    }
    let i = vec2<i32>(floor(q));
    let f = fract(q);
    let c = mix(mix(env_texel(i.x, i.y), env_texel(i.x + 1, i.y), f.x),
                mix(env_texel(i.x, i.y + 1), env_texel(i.x + 1, i.y + 1), f.x), f.y);
    env_target[gid.y * 1024u + gid.x] = vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) gid: vec3<u32>) {
    let level = env_params.source.z;
    let size = env_size(level);
    if gid.x >= size.x || gid.y >= size.y { return; }
    let above = env_size(level - 1u);
    let start = env_offset(level - 1u);
    let p = gid.xy * 2u;
    let sum = env_target[start + p.y * above.x + p.x] + env_target[start + p.y * above.x + p.x + 1u]
        + env_target[start + (p.y + 1u) * above.x + p.x] + env_target[start + (p.y + 1u) * above.x + p.x + 1u];
    env_target[env_offset(level) + gid.y * size.x + gid.x] = sum * 0.25;
}
@compute @workgroup_size(8, 8)
fn irradiance(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= 64u || gid.y >= 32u { return; }
    env_target[env_offset(9u) + gid.y * 64u + gid.x] = env_target[env_offset(4u) + gid.y * 64u + gid.x];
}
// A shape's picture made ready for an Image texture look, which reads the frame-sized
// straight colour the 2D material reads: the picture (source.xy, premultiplied)
// stretched over the frame (source.zw), so the look's texture coordinates cover it once.
@compute @workgroup_size(8, 8)
fn straight(@builtin(global_invocation_id) gid: vec3<u32>) {
    let frame = env_params.source.zw;
    if gid.x >= frame.x || gid.y >= frame.y { return; }
    let size = vec2<i32>(env_params.source.xy);
    let q = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(frame) * vec2<f32>(size) - 0.5;
    let base = vec2<i32>(floor(q));
    let f = fract(q);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1u);
        let at = clamp(base + o, vec2<i32>(0), size - 1);
        let w = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
        sum += env_source[u32(at.y * size.x + at.x)] * w;
    }
    env_target[gid.y * frame.x + gid.x] = vec4<f32>(sum.rgb / max(sum.a, 1e-4) * step(1e-4, sum.a), 1.0);
}
