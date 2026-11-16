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
    // light count, environment strength, environment turn (radians), textured (1)
    counts: vec4<f32>,
}
struct Light {
    // xyz position, kind (0 point, 1 spot, 2 sun)
    position: vec4<f32>,
    // xyz direction, cosine of the cone's outer half-angle
    direction: vec4<f32>,
    // rgb radiance, cosine of the inner half-angle
    colour: vec4<f32>,
    // Its shadow map's view-projection, and layer (-1: none), normal offset, filter width.
    shadow: mat4x4<f32>,
    shadow_info: vec4<f32>,
}
@group(0) @binding(0) var<uniform> object: Object;
@group(0) @binding(1) var<storage, read> lights: array<Light>;
@group(0) @binding(2) var<storage, read> environment_map: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> image: array<vec4<f32>>;
@group(0) @binding(4) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(5) var shadow_sampler: sampler_comparison;
@group(0) @binding(6) var sky_maps: texture_depth_2d_array;
@group(0) @binding(7) var<uniform> ambient: Ambient;
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
// The environment turned about the vertical by the scene's rotation.
fn turned(d: vec3<f32>) -> vec3<f32> {
    let c = cos(object.counts.z);
    let s = sin(object.counts.z);
    return vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
}
@fragment
fn lit(in: Varying) -> @location(0) vec4<f32> {
    let v = normalize(object.camera.xyz - in.world);
    var n = normalize(in.normal);
    // Two-sided: a face seen from behind shades as its other side.
    if dot(n, v) < 0.0 { n = -n; }
    var base = object.surface.rgb;
    // A model part's base colour texture (straight linear RGBA, uv repeating).
    if object.counts.w > 0.5 {
        let q = fract(in.uv) * object.params.zw - vec2<f32>(0.5);
        let i = vec2<i32>(floor(q));
        let f = fract(q);
        let t = mix(mix(wrapped(i.x, i.y), wrapped(i.x + 1, i.y), f.x),
                    mix(wrapped(i.x, i.y + 1), wrapped(i.x + 1, i.y + 1), f.x), f.y);
        base *= t.rgb;
    }
    let rough = clamp(object.surface.a, 0.04, 1.0);
    let a = rough * rough;
    let metal = clamp(object.params.x, 0.0, 1.0);
    let f0 = mix(vec3<f32>(0.04), base, metal);
    let diffuse = base * (1.0 - metal);
    let nv = max(dot(n, v), 1e-4);
    // The same prefiltered studio that lights 2D materials.
    let fresnel = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - nv, 5.0);
    let strength = object.counts.y;
    var colour = (environment_specular(turned(n), 1.0) * diffuse * strength
        + environment_specular(turned(reflect(-v, n)), rough) * fresnel * strength)
        * sky_visibility(in.world, n);
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
        radiance *= shadow_of(in.world, n, light.shadow, light.shadow_info);
        let h = normalize(l + v);
        let nh = max(dot(n, h), 0.0);
        let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
        let specular = ggx(nh, a) * smith(nv, nl, a) * f / max(4.0 * nv * nl, 1e-4);
        colour += (diffuse / 3.14159265 * (vec3<f32>(1.0) - f) + specular) * radiance * nl * 3.14159265;
    }
    return vec4<f32>(colour, 1.0);
}
// A texel of a repeating texture of params.zw pixels.
fn wrapped(x: i32, y: i32) -> vec4<f32> {
    let w = max(i32(object.params.z), 1);
    let h = max(i32(object.params.w), 1);
    return image[u32(((y % h + h) % h) * w + (x % w + w) % w)];
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

// A shadow catcher: transparent where lit, darkening by the share of the direct
// light that is blocked, weighed against the environment's ambient light.
@fragment
fn catcher(in: Varying) -> @location(0) vec4<f32> {
    let v = normalize(object.camera.xyz - in.world);
    var n = normalize(in.normal);
    if dot(n, v) < 0.0 { n = -n; }
    var direct = 0.0;
    var blocked = 0.0;
    for (var i = 0u; i < u32(object.counts.x); i++) {
        let light = lights[i];
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
        let amount = dot(radiance, vec3<f32>(0.2126, 0.7152, 0.0722)) * nl;
        direct += amount;
        blocked += amount * (1.0 - shadow_of(in.world, n, light.shadow, light.shadow_info));
    }
    let ambient_light = dot(environment_specular(turned(n), 1.0), vec3<f32>(0.2126, 0.7152, 0.0722)) * object.counts.y;
    let enclosed = ambient_light * (1.0 - sky_visibility(in.world, n));
    let shade = clamp((blocked + enclosed) / max(direct + ambient_light, 1e-4), 0.0, 1.0);
    return vec4<f32>(0.0, 0.0, 0.0, shade);
}
