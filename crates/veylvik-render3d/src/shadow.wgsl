// Depth from a light's point of view, for shadow maps.
struct ShadowObject {
    model: mat4x4<f32>,
    light: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> shadow_object: ShadowObject;
@vertex
fn shadow_vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> @builtin(position) vec4<f32> {
    return shadow_object.light * (shadow_object.model * vec4<f32>(position, 1.0));
}
