// The environment behind the objects, seen through the camera.
struct Sky {
    // Camera to world, column-major.
    camera: mat4x4<f32>,
    // width, height, focal length in pixels, strength
    view: vec4<f32>,
    // rotation (radians about the vertical), roughness level (0..8), unused
    more: vec4<f32>,
}
@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var<storage, read> environment_map: array<vec4<f32>>;
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
    let level = clamp(sky.more.y, 0.0, 8.0);
    let lo = u32(floor(level));
    let colour = mix(environment_layer(d, lo), environment_layer(d, min(lo + 1u, 8u)), fract(level));
    return vec4<f32>(colour * sky.view.w, 1.0);
}
