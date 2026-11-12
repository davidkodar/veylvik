struct Parameters { size: vec4<u32>, region: vec4<u32>, view: vec4<f32> }
@group(0) @binding(0) var<storage, read> pixels: array<vec4<f32>>;
@group(0) @binding(1) var<uniform> params: Parameters;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Vertex(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
fn srgb(v: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), 12.92 * v, v <= vec3<f32>(0.0031308));
}
fn sample_pixel(position: vec2<i32>) -> vec4<f32> {
    let local = vec2<u32>(clamp(position, vec2<i32>(0), vec2<i32>(params.region.zw) - vec2<i32>(1)));
    let xy = local + params.region.xy;
    return pixels[xy.y * params.size.x + xy.x];
}
@fragment fn fragment(v: Vertex) -> @location(0) vec4<f32> {
    // Filter scene-linear premultiplied samples before applying the display
    // transform. Clamp to the crop, so neighboring storage pixels never bleed.
    let uv = (v.uv - params.view.xy) / params.view.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        var background = vec3<f32>(24.0,27.0,33.0) / 255.0;
        if (params.size.z & 1u) != 0u { background = pow((background + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)); }
        return vec4<f32>(background,1.0);
    }
    let position = uv * vec2<f32>(params.region.zw) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(position));
    let weight = fract(position);
    let top = mix(sample_pixel(base), sample_pixel(base + vec2<i32>(1,0)), weight.x);
    let bottom = mix(sample_pixel(base + vec2<i32>(0,1)), sample_pixel(base + vec2<i32>(1,1)), weight.x);
    let pixel = mix(top, bottom, weight.y);
    let alpha = clamp(pixel.a, 0.0, 1.0);
    if (params.size.z & 2u) != 0u {
        // Coverage is displayed directly as gray, independent of exposure.
        var gray = alpha;
        if (params.size.z & 1u) != 0u {
            gray = select(pow((alpha + 0.055) / 1.055, 2.4), alpha / 12.92, alpha <= 0.04045);
        }
        return vec4<f32>(vec3<f32>(gray), 1.0);
    }
    let gain = exp2(bitcast<f32>(params.size.w));
    var color = vec3<f32>(0.0);
    if alpha > 0.0 { color = clamp(pixel.rgb * gain / alpha, vec3<f32>(0.0), vec3<f32>(1.0)) * alpha; }
    let grid = (u32(v.position.x) / 12u + u32(v.position.y) / 12u) % 2u;
    let checker = select(0.045, 0.075, grid == 0u);
    color += vec3<f32>(checker * (1.0 - alpha));
    if (params.size.z & 1u) == 0u { color = srgb(color); }
    return vec4<f32>(color, 1.0);
}
