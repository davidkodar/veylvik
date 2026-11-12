struct Parameters { size: vec4<u32> }
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
@fragment fn fragment(v: Vertex) -> @location(0) vec4<f32> {
    let xy = min(vec2<u32>(v.uv * vec2<f32>(params.size.xy)), params.size.xy - vec2<u32>(1u));
    let pixel = pixels[xy.y * params.size.x + xy.x];
    let alpha = clamp(pixel.a, 0.0, 1.0);
    var color = vec3<f32>(0.0);
    if alpha > 0.0 { color = clamp(pixel.rgb / alpha, vec3<f32>(0.0), vec3<f32>(1.0)) * alpha; }
    let grid = (u32(v.position.x) / 12u + u32(v.position.y) / 12u) % 2u;
    let checker = select(0.045, 0.075, grid == 0u);
    color += vec3<f32>(checker * (1.0 - alpha));
    if params.size.z == 0u { color = srgb(color); }
    return vec4<f32>(color, 1.0);
}
