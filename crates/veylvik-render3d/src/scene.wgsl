// Scene frame: x right, y down, z towards the viewer; colours scene-linear Rec.709.
struct Object {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    // rgb colour, roughness
    surface: vec4<f32>,
    // metallic, unlit (1), image width, image height
    params: vec4<f32>,
    // light count, environment strength
    counts: vec4<f32>,
}
struct Light {
    // xyz position, kind (0 point, 1 spot, 2 sun)
    position: vec4<f32>,
    // xyz direction, cosine of the cone's outer half-angle
    direction: vec4<f32>,
    // rgb radiance, cosine of the inner half-angle
    colour: vec4<f32>,
}
@group(0) @binding(0) var<uniform> object: Object;
@group(0) @binding(1) var<storage, read> lights: array<Light>;
@group(0) @binding(2) var<storage, read> environment_map: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> image: array<vec4<f32>>;

struct Varying {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}
@vertex
fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> Varying {
    var out: Varying;
    let world = object.model * vec4<f32>(position, 1.0);
    out.world = world.xyz / world.w;
    out.normal = (object.normal * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    out.clip = object.view_proj * world;
    return out;
}
fn ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (3.14159265 * d * d);
}
fn smith(nv: f32, nl: f32, a: f32) -> f32 {
    let k = a * 0.5;
    return (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
}
@fragment
fn lit(in: Varying) -> @location(0) vec4<f32> {
    let v = normalize(object.camera.xyz - in.world);
    var n = normalize(in.normal);
    // Two-sided: a face seen from behind shades as its other side.
    if dot(n, v) < 0.0 { n = -n; }
    let base = object.surface.rgb;
    let rough = clamp(object.surface.a, 0.04, 1.0);
    let a = rough * rough;
    let metal = clamp(object.params.x, 0.0, 1.0);
    let f0 = mix(vec3<f32>(0.04), base, metal);
    let diffuse = base * (1.0 - metal);
    let nv = max(dot(n, v), 1e-4);
    // The same prefiltered studio that lights 2D materials.
    let fresnel = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - nv, 5.0);
    let strength = object.counts.y;
    var colour = environment_specular(n, 1.0) * diffuse * strength
        + environment_specular(reflect(-v, n), rough) * fresnel * strength;
    for (var i = 0u; i < u32(object.counts.x); i++) {
        let light = lights[i];
        var l = -light.direction.xyz;
        var radiance = light.colour.rgb;
        if light.position.w < 1.5 {
            let to = light.position.xyz - in.world;
            let d2 = max(dot(to, to), 1.0);
            l = to / sqrt(d2);
            // Inverse square, normalised to 1000 pixels.
            radiance *= 1.0e6 / d2;
            if light.position.w > 0.5 {
                let c = dot(-l, light.direction.xyz);
                radiance *= smoothstep(light.direction.w, light.colour.w, c);
            }
        }
        let nl = dot(n, l);
        if nl <= 0.0 { continue; }
        let h = normalize(l + v);
        let nh = max(dot(n, h), 0.0);
        let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
        let specular = ggx(nh, a) * smith(nv, nl, a) * f / max(4.0 * nv * nl, 1e-4);
        colour += (diffuse / 3.14159265 * (vec3<f32>(1.0) - f) + specular) * radiance * nl * 3.14159265;
    }
    return vec4<f32>(colour, 1.0);
}
fn texel(x: i32, y: i32) -> vec4<f32> {
    let w = i32(object.params.z);
    let h = i32(object.params.w);
    if x < 0 || y < 0 || x >= w || y >= h { return vec4<f32>(0.0); }
    return image[u32(y * w + x)];
}
@fragment
fn card(in: Varying) -> @location(0) vec4<f32> {
    // Bilinear at the picture's own pixel centres; premultiplied, unlit.
    let q = in.uv * object.params.zw - vec2<f32>(0.5);
    let i = vec2<i32>(floor(q));
    let f = fract(q);
    return mix(mix(texel(i.x, i.y), texel(i.x + 1, i.y), f.x),
               mix(texel(i.x, i.y + 1), texel(i.x + 1, i.y + 1), f.x), f.y);
}
