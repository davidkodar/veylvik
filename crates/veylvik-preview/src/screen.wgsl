// A frame as the viewer shows it, in one word a pixel, and back. `look`: x = exposure gain, y = filmic (1) or clip at white (0).
struct Settings { size: vec4<u32>, look: vec4<f32> }
@group(0) @binding(0) var<uniform> settings: Settings;
fn srgb(v: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), 12.92 * v, v <= vec3<f32>(0.0031308));
}
fn srgb_inverse(v: vec3<f32>) -> vec3<f32> {
    return select(pow((v + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}
// The viewer's display transform, kept the same as display.wgsl in the editor.
fn filmic(rgb: vec3<f32>) -> vec3<f32> {
    let input = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777));
    let output = mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602));
    let v = input * max(rgb, vec3<f32>(0.0));
    let t = (v * (v + 0.0245786) - 0.000090537) / (v * (0.983729 * v + 0.4329510) + 0.238081);
    return clamp(output * t, vec3<f32>(0.0), vec3<f32>(1.0));
}
