// The environment behind the objects, seen through the camera.
struct Sky {
    // Camera to world, column-major.
    camera: mat4x4<f32>,
    // width, height, focal length in pixels, strength
    view: vec4<f32>,
    // rotation (radians about the vertical), roughness level (0..8), and the
    // panorama's own width and height when it is shown as it is (0: not)
    more: vec4<f32>,
}
@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var<storage, read> environment_map: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> sky_picture: array<vec4<f32>>;
// The panorama's own pixel, wrapped round and clamped at the poles; straight colour.
fn sky_texel(x: i32, y: i32) -> vec3<f32> {
    let w = i32(sky.more.z);
    let h = i32(sky.more.w);
    let c = sky_picture[u32(clamp(y, 0, h - 1) * w + ((x % w) + w) % w)];
    return c.rgb / max(c.a, 1e-4) * step(1e-4, c.a);
}
@vertex
fn sky_vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    // Furthest depth with reversed z, so everything draws in front.
    return vec4<f32>(p, 0.0, 1.0);
}
@fragment
fn sky_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let view = normalize(vec3<f32>((at.x - sky.view.x * 0.5) / sky.view.z, (at.y - sky.view.y * 0.5) / sky.view.z, -1.0));
    var d = (sky.camera * vec4<f32>(view, 0.0)).xyz;
    let c = cos(sky.more.x);
    let s = sin(sky.more.x);
    d = vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
    // Unblurred, the backdrop is the picture itself, at its own sharpness; the
    // lighting copy is only 1024 by 512.
    if sky.more.z > 0.5 {
        let n = normalize(vec3<f32>(d.x, -d.y, d.z));
        let uv = vec2<f32>(atan2(n.z, n.x) / 6.2831853 + 0.5, acos(clamp(n.y, -1.0, 1.0)) / 3.14159265);
        let q = uv * sky.more.zw - vec2<f32>(0.5);
        let i = vec2<i32>(floor(q));
        let f = fract(q);
        let sharp = mix(mix(sky_texel(i.x, i.y), sky_texel(i.x + 1, i.y), f.x),
                        mix(sky_texel(i.x, i.y + 1), sky_texel(i.x + 1, i.y + 1), f.x), f.y);
        return vec4<f32>(max(sharp, vec3<f32>(0.0)) * sky.view.w, 1.0);
    }
    let level = clamp(sky.more.y, 0.0, 8.0);
    let lo = u32(floor(level));
    let colour = mix(environment_layer(d, lo), environment_layer(d, min(lo + 1u, 8u)), fract(level));
    return vec4<f32>(colour * sky.view.w, 1.0);
}
