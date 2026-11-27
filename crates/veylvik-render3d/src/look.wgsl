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
    shadow: mat4x4<f32>,
    shadow_info: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
}
@group(1) @binding(0) var<uniform> look_object: LookObject;
@group(1) @binding(1) var<storage, read> scene_lights: array<SceneLight>;
@group(1) @binding(2) var shadow_maps: texture_depth_2d_array;
@group(1) @binding(3) var shadow_sampler: sampler_comparison;
@group(1) @binding(4) var sky_maps: texture_depth_2d_array;
@group(1) @binding(5) var<uniform> ambient: Ambient;
struct AmbientView {
    matrix: mat4x4<f32>,
    // towards the sky, normal offset
    direction: vec4<f32>,
}
struct Ambient {
    views: array<AmbientView, 16>,
    // views in use, amount, contact reach, depth span (scene units)
    info: vec4<f32>,
}
// How open `position` is to the surrounding light (1 open, 0 enclosed): depth maps
// of the scene seen from many directions, counting only blockers within the
// contact reach (fading with distance) and weighting directions towards the normal.
fn sky_visibility(position: vec3<f32>, normal: vec3<f32>) -> f32 {
    let count = u32(ambient.info.x);
    if count == 0u || ambient.info.y <= 0.0 { return 1.0; }
    let reach = ambient.info.z;
    let span = ambient.info.w;
    var total = 0.0;
    var seen = 0.0;
    for (var k = 0u; k < count; k++) {
        let view = ambient.views[k];
        let w = max(dot(normal, view.direction.xyz), 0.0);
        if w <= 0.0 { continue; }
        let p = view.matrix * vec4<f32>(position + normal * view.direction.w, 1.0);
        let ndc = p.xyz / p.w;
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        var open = 1.0;
        if uv.x >= 0.0 && uv.y >= 0.0 && uv.x <= 1.0 && uv.y <= 1.0 && ndc.z <= 1.0 {
            let texel = vec2<i32>(uv * 1024.0);
            var blocked = 0.0;
            for (var t = 0; t < 4; t++) {
                let at = clamp(texel + vec2<i32>(t % 2 * 2 - 1, t / 2 * 2 - 1), vec2<i32>(0), vec2<i32>(1023));
                let gap = (ndc.z - textureLoad(sky_maps, at, i32(k), 0)) * span;
                if gap > view.direction.w {
                    blocked += 1.0 - smoothstep(reach * 0.5, reach, gap);
                }
            }
            open = 1.0 - blocked * 0.25;
        }
        total += w;
        seen += w * open;
    }
    if total <= 0.0 { return 1.0; }
    return mix(1.0, seen / total, ambient.info.y);
}

// How much of light `index` reaches `world` (1 lit, 0 shadowed): a 3x3 filtered
// shadow-map test, offset along the normal so surfaces never shadow themselves.
fn shadow_of(position: vec3<f32>, normal: vec3<f32>, shadow: mat4x4<f32>, info: vec4<f32>) -> f32 {
    let layer = i32(info.x);
    if layer < 0 { return 1.0; }
    let p = shadow * vec4<f32>(position + normal * info.y, 1.0);
    if p.w <= 0.0 { return 1.0; }
    let ndc = p.xyz / p.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // Outside the map nothing casts; beyond its far end a point is behind every caster.
    if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 { return 1.0; }
    let depth = min(ndc.z, 1.0);
    let step = info.z / 2048.0;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            lit += textureSampleCompareLevel(shadow_maps, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * step, layer, depth);
        }
    }
    return lit / 9.0;
}

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
    // A neon tube is white-hot where it faces the viewer, as its 2D bevel's peak is.
    tube_core = smoothstep(0.55, 1.0, facing.z);
    let surface = material_colour(uv, facing) * sky_visibility(in.world, n);
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
        if light.position.w > 2.5 {
            // An area light: its true share of the look's lobe and of a matte cosine
            // (linearly transformed cosines, the 2D materials' own `ltc_rect`).
            let facing_light = light.direction.xyz;
            let to = light.position.xyz - in.world;
            if dot(-to, facing_light) > 0.0 {
                var u = light.right.xyz;
                var w = light.up.xyz;
                // `ltc_rect` lights only from the side u x w turns away from.
                if dot(cross(u, w), facing_light) > 0.0 {
                    u = light.up.xyz;
                    w = light.right.xyz;
                }
                var tangent = v - n * dot(n, v);
                if dot(tangent, tangent) < 1e-6 { tangent = cross(n, vec3<f32>(0.0, 1.0, 0.0)); }
                tangent = normalize(tangent);
                let frame = transpose(mat3x3<f32>(tangent, cross(n, tangent), n));
                let rough = sqrt(max(a, 0.0));
                let nv = max(dot(n, v), 1e-4);
                let m = ltc_lookup(rough, nv, 0u);
                let amplitude = ltc_lookup(rough, nv, 4096u);
                let inverse = mat3x3<f32>(vec3<f32>(m.x, 0.0, m.y), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(m.z, 0.0, m.w));
                var shine = ltc_rect(in.world, inverse * frame, light.position.xyz, u, w);
                // A feathered panel: the mean of the panels shrunk evenly across its fade.
                let feather = clamp(light.right.w, 0.0, 1.0);
                if feather > 0.0 {
                    shine = 0.0;
                    for (var k = 0u; k < 4u; k++) {
                        let s = 1.0 - feather * (f32(k) + 0.5) / 4.0;
                        shine += 0.25 * ltc_rect(in.world, inverse * frame, light.position.xyz, u * s, w * s);
                    }
                }
                let reflected = f0 * amplitude.x + (vec3<f32>(1.0) - f0) * amplitude.y;
                added += (light.colour.rgb * reflected * shine * light.up.w
                    + albedo * light.colour.rgb * ltc_rect(in.world, frame, light.position.xyz, u, w))
                    * shadow_of(in.world, n, light.shadow, light.shadow_info);
            }
            continue;
        }
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
        radiance *= shadow_of(in.world, n, light.shadow, light.shadow_info);
        let h = normalize(l + v);
        let nh = max(dot(n, h), 0.0);
        let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
        let d = nh * nh * (a * a - 1.0) + 1.0;
        let ggx = a * a / (3.14159265 * d * d);
        let k = a * 0.5;
        let g = (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
        let specular = ggx * g * f / max(4.0 * nv * nl, 1e-4) * light.up.w;
        added += (albedo / 3.14159265 * (vec3<f32>(1.0) - f) + specular) * radiance * nl * 3.14159265;
    }
    return vec4<f32>(surface + added, 1.0);
}
