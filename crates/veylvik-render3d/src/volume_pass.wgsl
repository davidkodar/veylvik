// Volumes in the interactive renderer: each pixel's camera ray steps through the scene's
// media up to the nearest solid, adding the light they scatter towards the camera (every
// light, shaded by its shadow map and dimmed on its way through the media, and the
// environment's average light) and dimming what lies behind. Appended to scene.wgsl (its
// lights, shadow maps and environment), the shared volume.wgsl and the environment code.
struct Fog {
    // Camera to world, as rows.
    camera: mat3x4<f32>,
    // focal length in pixels, width, height, the depth's constant (distance = it / depth)
    lens: vec4<f32>,
    // volumes, lights, environment strength, steps along each ray
    counts: vec4<f32>,
}
@group(1) @binding(0) var<uniform> fog: Fog;
@group(1) @binding(1) var fog_resolved: texture_2d<f32>;
@group(1) @binding(2) var fog_depth: texture_depth_multisampled_2d;
@group(1) @binding(3) var fogged: texture_storage_2d<rgba16float, write>;
@group(1) @binding(4) var<storage, read> media: array<vec4<f32>>;

fn medium_at(k: u32) -> Medium {
    let b = MEDIUM_VEC4S * k;
    return medium_of(media[b], media[b + 1u], media[b + 2u], media[b + 3u], media[b + 4u], media[b + 5u], media[b + 6u], media[b + 7u]);
}
// The light reaching `p` from light `i` as irradiance on a surface facing it (its colour
// times pi, as surfaces are lit), and the way to it; w is how far it is.
struct Arriving { light: vec3<f32>, toward: vec3<f32>, far: f32 }
fn arriving(light: Light, p: vec3<f32>) -> Arriving {
    var l = -light.direction.xyz;
    var e = light.colour.rgb * 3.14159265;
    var far = 1.0e9;
    if light.position.w > 2.5 {
        let to = light.position.xyz - p;
        let d2 = max(dot(to, to), 1.0);
        far = sqrt(d2);
        l = to / far;
        let front = max(dot(-l, light.direction.xyz), 0.0);
        e = light.colour.rgb * 4.0 * length(light.right.xyz) * length(light.up.xyz) * front / d2;
    } else if light.position.w < 1.5 {
        let to = light.position.xyz - p;
        let d2 = max(dot(to, to), 1.0);
        far = sqrt(d2);
        l = to / far;
        e *= 1.0e6 / d2;
        if light.position.w > 0.5 {
            e *= smoothstep(light.direction.w, light.colour.w, dot(-l, light.direction.xyz));
        }
    }
    return Arriving(e, l, far);
}
// How much medium lies from `p` along `l` (up to `far`), as optical depth: a few steps.
fn through_media(p: vec3<f32>, l: vec3<f32>, far: f32, jitter: f32) -> f32 {
    let count = u32(fog.counts.x);
    var reach = 0.0;
    for (var k = 0u; k < count; k++) {
        let span = medium_span(medium_at(k), p, l);
        if span.y > span.x { reach = max(reach, min(span.y, far)); }
    }
    if reach <= 0.0 { return 0.0; }
    let steps = 6u;
    let step = reach / f32(steps);
    var depth = 0.0;
    for (var s = 0u; s < steps; s++) {
        let q = p + l * (step * (f32(s) + jitter));
        for (var k = 0u; k < count; k++) {
            depth += medium_density(medium_at(k), q);
        }
    }
    return depth * step;
}
// Light scattered once and, approximately, many times (Wrenninge's octaves, as real-time
// cloud renderers do): each further octave weaker, less forward and less dimmed, so thick
// media glow from within as the path tracer finds.
fn scattered_from(g: f32, travelling: vec3<f32>, onward: vec3<f32>, depth: f32) -> f32 {
    var sum = 0.0;
    var a = 1.0;
    var b = 1.0;
    var c = 1.0;
    for (var n = 0u; n < 3u; n++) {
        sum += a * medium_phase(g * c, travelling, onward) * exp(-b * depth);
        a *= 0.5;
        b *= 0.5;
        c *= 0.5;
    }
    return sum;
}
// The same for light from all around (isotropic), dimmed by the medium above.
fn scattered_around(depth: f32) -> f32 {
    return exp(-depth) + 0.5 * exp(-0.5 * depth) + 0.25 * exp(-0.25 * depth);
}
fn fog_hash(x: u32, y: u32) -> f32 {
    var h = x * 1973u + y * 9277u + 26699u;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return f32(h ^ (h >> 16u)) / 4294967295.0;
}
@compute @workgroup_size(8, 8)
fn through_volumes(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(fog_resolved);
    if gid.x >= size.x || gid.y >= size.y { return; }
    let p = vec2<i32>(gid.xy);
    let behind = textureLoad(fog_resolved, p, 0);
    // The camera's ray through the pixel's middle (y down, looking along -z).
    let f = fog.lens.x;
    let ray = normalize(vec3<f32>((f32(gid.x) + 0.5 - fog.lens.y * 0.5) / f, (f32(gid.y) + 0.5 - fog.lens.z * 0.5) / f, -1.0));
    let o = vec4<f32>(0.0, 0.0, 0.0, 1.0) * fog.camera;
    let d = normalize(vec4<f32>(ray, 0.0) * fog.camera);
    // How far to the nearest solid along it.
    let z = textureLoad(fog_depth, p, 0);
    var solid = 1.0e9;
    if z > 0.0 { solid = (fog.lens.w / z) / max(-ray.z, 1e-6); }
    let count = u32(fog.counts.x);
    var start = solid;
    var end = 0.0;
    for (var k = 0u; k < count; k++) {
        let span = medium_span(medium_at(k), o, d);
        if span.y > span.x {
            start = min(start, span.x);
            end = max(end, min(span.y, solid));
        }
    }
    if end <= start {
        textureStore(fogged, p, behind);
        return;
    }
    // The environment's average light, for light scattered in from all around.
    let strength = fog.counts.z;
    let around = (environment_layer(vec3<f32>(1.0, 0.0, 0.0), 9u) + environment_layer(vec3<f32>(-1.0, 0.0, 0.0), 9u)
        + environment_layer(vec3<f32>(0.0, 1.0, 0.0), 9u) + environment_layer(vec3<f32>(0.0, -1.0, 0.0), 9u)
        + environment_layer(vec3<f32>(0.0, 0.0, 1.0), 9u) + environment_layer(vec3<f32>(0.0, 0.0, -1.0), 9u)) / 6.0 * strength;
    let steps = u32(fog.counts.w);
    let step = (end - start) / f32(steps);
    let jitter = fog_hash(gid.x, gid.y);
    var through = 1.0;
    var gathered = vec3<f32>(0.0);
    for (var s = 0u; s < steps; s++) {
        let t = start + step * (f32(s) + jitter);
        let q = o + d * t;
        var extinction = 0.0;
        var scattered = vec3<f32>(0.0);
        for (var k = 0u; k < count; k++) {
            let m = medium_at(k);
            let sigma = medium_density(m, q);
            if sigma <= 0.0 { continue; }
            extinction += sigma;
            let albedo = medium_albedo(m) * sigma;
            // Light from all around, dimmed by the medium above (the sky is mostly up).
            var lit = around * scattered_around(through_media(q, vec3<f32>(0.0, -1.0, 0.0), 1.0e9, jitter)) / 1.75;
            for (var i = 0u; i < u32(fog.counts.y); i++) {
                let light = lights[i];
                let a = arriving(light, q);
                if dot(a.light, a.light) <= 0.0 { continue; }
                var seen = 1.0;
                if light.shadow_info.x >= 0.0 {
                    seen = shadow_of(q, vec3<f32>(0.0), light.shadow, light.shadow_info);
                }
                if seen <= 0.0 { continue; }
                let depth = through_media(q, a.toward, a.far, jitter);
                lit += a.light * seen * scattered_from(m.forward, -a.toward, -d, depth);
            }
            scattered += albedo * lit;
        }
        // This step's light, dimmed by what lies before it, and its own dimming.
        let dim = exp(-extinction * step);
        if extinction > 0.0 {
            gathered += through * scattered / extinction * (1.0 - dim);
        }
        through *= dim;
        if through < 1e-3 { break; }
    }
    textureStore(fogged, p, vec4<f32>(behind.rgb * through + gathered, behind.a * through + (1.0 - through)));
}
