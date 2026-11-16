// Surfaces with a library look, shaded by the material kernels' own
// `material_colour` (group 0 is theirs). Pattern coordinates come from the mesh's
// pixel-sized uv; the normal is turned into camera space, where a face looking at
// the camera faces +z exactly as a flat title does.
struct LookObject {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    // World to camera rotation.
    view: mat4x4<f32>,
    camera: vec4<f32>,
    // Pixels per mesh uv unit (xy), scene light count (z).
    uv_scale: vec4<f32>,
}
struct SceneLight {
    position: vec4<f32>,
    direction: vec4<f32>,
    colour: vec4<f32>,
}
@group(1) @binding(0) var<uniform> look_object: LookObject;
@group(1) @binding(1) var<storage, read> scene_lights: array<SceneLight>;
struct LookVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}
@vertex
fn look_vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> LookVarying {
    var out: LookVarying;
    let world = look_object.model * vec4<f32>(position, 1.0);
    out.world = world.xyz / world.w;
    out.normal = (look_object.normal * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    out.clip = look_object.view_proj * world;
    return out;
}
@fragment
fn look_surface(in: LookVarying) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    if dot(n, look_object.camera.xyz - in.world) < 0.0 { n = -n; }
    let facing = normalize((look_object.view * vec4<f32>(n, 0.0)).xyz);
    let uv = in.uv * look_object.uv_scale.xy / max(p.values.x, 1e-3);
    let surface = material_colour(uv, facing);
    // Scene lights add to the look's own studio. Metals reflect them in their
    // reflectance; other looks also scatter them with the colour the look shows.
    let v = normalize(look_object.camera.xyz - in.world);
    let rough = clamp(surface_roughness(), 0.04, 1.0);
    let a = rough * rough;
    let metal = studio.optics.w > 0.5;
    let f0 = studio.optics.rgb;
    let albedo = select(clamp(surface, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.0), metal);
    let nv = max(dot(n, v), 1e-4);
    var added = vec3<f32>(0.0);
    for (var i = 0u; i < u32(look_object.uv_scale.z); i++) {
        let light = scene_lights[i];
        var l = -light.direction.xyz;
        var radiance = light.colour.rgb;
        if light.position.w < 1.5 {
            let to = light.position.xyz - in.world;
            let d2 = max(dot(to, to), 1.0);
            l = to / sqrt(d2);
            radiance *= 1.0e6 / d2;
            if light.position.w > 0.5 {
                radiance *= smoothstep(light.direction.w, light.colour.w, dot(-l, light.direction.xyz));
            }
        }
        let nl = dot(n, l);
        if nl <= 0.0 { continue; }
        let h = normalize(l + v);
        let nh = max(dot(n, h), 0.0);
        let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
        let d = nh * nh * (a * a - 1.0) + 1.0;
        let ggx = a * a / (3.14159265 * d * d);
        let k = a * 0.5;
        let g = (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
        let specular = ggx * g * f / max(4.0 * nv * nl, 1e-4);
        added += (albedo / 3.14159265 * (vec3<f32>(1.0) - f) + specular) * radiance * nl * 3.14159265;
    }
    return vec4<f32>(surface + added, 1.0);
}
