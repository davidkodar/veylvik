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
    // brushed strength, grain direction (radians), imperfections, visible grain
    finish: vec4<f32>,
    // ridges, frame width / 1920, shadow strength, reflections view (1)
    relief: vec4<f32>,
    // copies at particles (1), drawn from `instances`
    instancing: vec4<f32>,
    // A picture cast through a camera (a Camera projection): the camera's view and
    // projection; where it is; cast (1), lit (1), unseen left out (1).
    projector: mat4x4<f32>,
    projector_eye: vec4<f32>,
    projection: vec4<f32>,
    // The projecting camera's depth map, as a light's shadow (layer -1: none).
    projector_shadow: mat4x4<f32>,
    projector_shadow_info: vec4<f32>,
}
struct Light {
    // xyz position, kind (0 point, 1 spot, 2 sun, 3 area)
    position: vec4<f32>,
    // xyz direction, cosine of the cone's outer half-angle
    direction: vec4<f32>,
    // rgb radiance, cosine of the inner half-angle
    colour: vec4<f32>,
    // Its shadow map's view-projection, and layer (-1: none), normal offset, filter width.
    shadow: mat4x4<f32>,
    shadow_info: vec4<f32>,
    // An area light's half-width and half-height axes.
    right: vec4<f32>,
    up: vec4<f32>,
}
@group(0) @binding(0) var<uniform> object: Object;
@group(0) @binding(1) var<storage, read> lights: array<Light>;
@group(0) @binding(2) var<storage, read> environment_map: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> image: array<vec4<f32>>;
@group(0) @binding(4) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(5) var shadow_sampler: sampler_comparison;
@group(0) @binding(6) var sky_maps: texture_depth_2d_array;
@group(0) @binding(7) var<uniform> ambient: Ambient;
// Copies at particles, three values each: where and size, turn (a quaternion), tint.
@group(0) @binding(8) var<storage, read> instances: array<vec4<f32>>;
fn turned_by(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    return v + 2.0 * cross(q.xyz, cross(q.xyz, v) + q.w * v);
}
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
    // Object-space position and normal, so a finish stays on the surface as it moves.
    @location(3) local: vec3<f32>,
    @location(4) facing: vec3<f32>,
    // A copy's tint (white for an object drawn once).
    @location(5) @interpolate(flat) tint: vec4<f32>,
}
@vertex
fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>, @builtin(instance_index) copy: u32) -> Varying {
    var out: Varying;
    var world = object.model * vec4<f32>(position, 1.0);
    out.normal = (object.normal * vec4<f32>(normal, 0.0)).xyz;
    out.tint = vec4<f32>(1.0);
    if object.instancing.x > 0.5 {
        // The object centred on itself, turned and sized by the copy, at its particle.
        let at = instances[3u * copy]; let q = instances[3u * copy + 1u];
        out.tint = instances[3u * copy + 2u];
        world = vec4<f32>(at.xyz + turned_by(q, world.xyz / world.w * at.w), 1.0);
        out.normal = turned_by(q, out.normal);
    }
    out.world = world.xyz / world.w;
    out.uv = uv;
    out.local = position;
    out.facing = normal;
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
fn finish_hash(q: vec2<f32>) -> f32 {
    var h = vec3<f32>(q.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973);
    h = fract(h);
    h += dot(h, h.yzx + 33.33);
    return fract((h.x + h.y) * h.z);
}
// Smooth value noise in 0..1.
fn finish_noise(q: vec2<f32>) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let s = f * f * (3.0 - 2.0 * f);
    return mix(mix(finish_hash(i), finish_hash(i + vec2<f32>(1.0, 0.0)), s.x),
               mix(finish_hash(i + vec2<f32>(0.0, 1.0)), finish_hash(i + vec2<f32>(1.0, 1.0)), s.x), s.y);
}
// The surface's own frame at this point, in world space: `along` runs with the
// brushing's grain and `across` over it, both at right angles to the normal.
struct Grain { along: vec3<f32>, across: vec3<f32>, q: vec2<f32> }
fn grain(in: Varying, n: vec3<f32>) -> Grain {
    let m = normalize(in.facing);
    var side = cross(m, vec3<f32>(0.0, 1.0, 0.0));
    if dot(side, side) < 1e-4 { side = vec3<f32>(1.0, 0.0, 0.0); }
    side = normalize(side);
    let up = cross(side, m);
    let c = cos(object.finish.y);
    let s = sin(object.finish.y);
    let along = side * c + up * s;
    let across = up * c - side * s;
    var g: Grain;
    // Patterns are sized for a 1920-wide frame and grow with the frame.
    g.q = vec2<f32>(dot(in.local, along), dot(in.local, across)) / max(object.relief.y, 1e-3);
    let wa = (object.normal * vec4<f32>(across, 0.0)).xyz;
    g.across = normalize(wa - n * dot(wa, n));
    g.along = normalize(cross(g.across, n));
    return g;
}
// Low-contrast blotches of several sizes in -1..1: how handling and wear leave a
// surface a little more or less polished from place to place.
fn smudges(q: vec2<f32>) -> f32 {
    return (finish_noise(q * 0.006 + vec2<f32>(3.0, 9.0)) - 0.5) * 1.0
        + (finish_noise(q * 0.019 + vec2<f32>(11.0, 2.0)) - 0.5) * 0.6
        + (finish_noise(q * 0.07 + vec2<f32>(7.0, 4.0)) - 0.5) * 0.3
        + (finish_noise(q * 0.9 + vec2<f32>(1.0, 6.0)) - 0.5) * 0.1;
}
// GGX with separate roughness along and across the grain.
fn ggx_grain(h: vec3<f32>, n: vec3<f32>, g: Grain, ax: f32, ay: f32) -> f32 {
    let x = dot(h, g.along) / ax;
    let y = dot(h, g.across) / ay;
    let z = dot(h, n);
    let d = x * x + y * y + z * z;
    return 1.0 / (3.14159265 * ax * ay * d * d);
}
@fragment
fn lit(in: Varying) -> @location(0) vec4<f32> {
    let v = normalize(object.camera.xyz - in.world);
    var n = normalize(in.normal);
    // Where the surface curves faster than the pixel grid can show (a tight bevel),
    // a highlight would be thinner than a pixel and break into dots: widen the lobe
    // by how much the normal changes across the pixel instead.
    let bend_x = dpdx(n);
    let bend_y = dpdy(n);
    // Measured per pixel of a 1920-wide frame too, so a bevel's highlight is as soft
    // in a larger frame as in that one (and never thinner than this frame's pixels).
    let unit = max(object.relief.y, 1.0);
    let curvature = min((dot(bend_x, bend_x) + dot(bend_y, bend_y)) * unit * unit, 0.03);
    // Two-sided: a face seen from behind shades as its other side.
    if dot(n, v) < 0.0 { n = -n; }
    let brushed = clamp(object.finish.x, 0.0, 1.0);
    let worn = clamp(object.finish.z, 0.0, 1.0);
    let grainy = clamp(object.finish.w, 0.0, 1.0);
    let ridged = clamp(object.relief.x, 0.0, 1.0);
    var g: Grain;
    var blotch = 0.0;
    if brushed > 0.0 || worn > 0.0 || grainy > 0.0 || ridged > 0.0 {
        g = grain(in, n);
        blotch = smudges(g.q);
        // A plate is never perfectly flat: slow waves, and a trace of grain.
        // A worked plate undulates in long shallow ridges that follow the grain, so
        // parts of it tip towards a light and parts away: the soft bands on real metal.
        let ridge = (finish_noise(vec2<f32>(g.q.x * 0.0026, g.q.y * 0.0105)) - 0.5)
            + 0.4 * (finish_noise(vec2<f32>(g.q.x * 0.006 + 13.0, g.q.y * 0.028 + 5.0)) - 0.5);
        let wave = vec2<f32>(finish_noise(g.q * 0.012 + vec2<f32>(21.0, 5.0)) - 0.5,
                             finish_noise(g.q * 0.012 + vec2<f32>(4.0, 31.0)) - 0.5);
        let fine = finish_noise(vec2<f32>(g.q.x * 0.01, g.q.y * 1.9)) - 0.5;
        n = normalize(n + g.across * ridge * 0.22 * ridged
            + (g.along * wave.x + g.across * wave.y) * 0.025 * worn
            + g.across * fine * 0.01 * grainy);
        g.across = normalize(g.across - n * dot(g.across, n));
        g.along = normalize(cross(g.across, n));
    }
    var base = object.surface.rgb * in.tint.rgb;
    // A model part's base colour texture (straight linear RGBA, uv repeating).
    if object.counts.w > 0.5 {
        let q = fract(in.uv) * object.params.zw - vec2<f32>(0.5);
        let i = vec2<i32>(floor(q));
        let f = fract(q);
        let t = mix(mix(wrapped(i.x, i.y), wrapped(i.x + 1, i.y), f.x),
                    mix(wrapped(i.x, i.y + 1), wrapped(i.x + 1, i.y + 1), f.x), f.y);
        if object.counts.w > 1.5 {
            // A shape's picture is premultiplied; under half opaque it is a hole.
            if t.a < 0.5 {
                discard;
            }
            base *= t.rgb / t.a;
        } else {
            base *= t.rgb;
        }
    }
    // A picture cast through a camera: where the camera saw this point, if it did.
    if object.projection.x > 0.5 {
        let seen = object.projector * vec4<f32>(in.world, 1.0);
        var shown = false;
        var picture = vec4<f32>(0.0);
        if seen.w > 0.0 {
            let ndc = seen.xy / seen.w;
            let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            let toward = object.projector_eye.xyz - in.world;
            // In view, facing the camera and not hidden from it behind something else.
            if all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)) && dot(in.normal, toward) > 0.0
                && shadow_of(in.world, normalize(in.normal), object.projector_shadow, object.projector_shadow_info) > 0.5 {
                let q = uv * object.params.zw - vec2<f32>(0.5);
                let i = vec2<i32>(floor(q));
                let f = fract(q);
                picture = mix(mix(texel(i.x, i.y), texel(i.x + 1, i.y), f.x),
                           mix(texel(i.x, i.y + 1), texel(i.x + 1, i.y + 1), f.x), f.y);
                shown = true;
            }
        }
        if !shown {
            if object.projection.z > 0.5 { discard; }
        } else if object.projection.y < 0.5 {
            // As it is: the picture's own colours, premultiplied.
            return picture;
        } else {
            base = picture.rgb / max(picture.a, 1e-4);
        }
    }
    // Wear changes the polish from place to place.
    let polish = clamp(object.surface.a * (1.0 + blotch * 1.1 * worn), 0.03, 1.0);
    let a = sqrt(min(polish * polish * polish * polish + curvature, 1.0));
    let rough = sqrt(a);
    // Brushing scatters light across the grain far more than along it.
    let ax = a;
    let ay = clamp(a + brushed * 0.12, 0.0, 1.0);
    let metal = clamp(object.params.x, 0.0, 1.0);
    var f0 = mix(vec3<f32>(0.04), base, metal);
    var diffuse = base * (1.0 - metal);
    // The reflections view: every surface a neutral mirror, so only where light lands
    // shows, each light in a colour of its own.
    let mapping = object.relief.w > 0.5;
    if mapping {
        f0 = vec3<f32>(0.9);
        diffuse = vec3<f32>(0.0);
    }
    let nv = max(dot(n, v), 1e-4);
    // The same prefiltered studio that lights 2D materials.
    let fresnel = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - nv, 5.0);
    let strength = object.counts.y;
    // How the grain brightens or dulls reflections at this point (lights included).
    var grain_gain = 1.0;
    var mirrored = environment_specular(turned(reflect(-v, n)), rough);
    if brushed > 0.0 {
        // The environment seen through that scatter: blurred across the grain only.
        mirrored = vec3<f32>(0.0);
        var total = 0.0;
        for (var k = -5; k <= 5; k++) {
            let t = f32(k) / 5.0;
            let w = exp(-2.5 * t * t);
            let tilted = normalize(n + g.across * (t * ay * 0.6));
            mirrored += environment_specular(turned(reflect(-v, tilted)), rough) * w;
            total += w;
        }
        mirrored /= total;
    }
    if grainy > 0.0 {
        // The grain itself shows as faint streaks of brighter and duller polish.
        let streaks = (finish_noise(vec2<f32>(g.q.x * 0.010, g.q.y * 2.6)) - 0.5) * 0.55
            + (finish_noise(vec2<f32>(g.q.x * 0.016 + 9.0, g.q.y * 1.1)) - 0.5) * 0.7
            + (finish_noise(vec2<f32>(g.q.x * 0.006 + 2.0, g.q.y * 0.4)) - 0.5) * 0.6
            + (finish_noise(vec2<f32>(g.q.x * 0.003 + 31.0, g.q.y * 0.13)) - 0.5) * 0.5;
        grain_gain = max(1.0 + streaks * grainy, 0.0);
        mirrored *= grain_gain;
    }
    if worn > 0.0 {
        // Wear: sparse fine scratches and wisps, a little off the grain either way,
        // some catching the light and some dull.
        var bright = 0.0;
        var dull = 0.0;
        for (var k = 0; k < 3; k++) {
            let turn = (f32(k) - 1.0) * 0.22;
            let c = cos(turn);
            let s = sin(turn);
            let r = vec2<f32>(g.q.x * c - g.q.y * s, g.q.x * s + g.q.y * c);
            let o = f32(k) * 37.0;
            bright += smoothstep(0.62, 0.95, finish_noise(vec2<f32>(r.x * 0.014 + o, r.y * 0.7 + o)))
                * smoothstep(0.35, 0.6, finish_noise(r * 0.03 + vec2<f32>(o, 5.0)));
            dull += smoothstep(0.62, 0.95, finish_noise(vec2<f32>(r.x * 0.012 + o + 50.0, r.y * 0.55 + o)))
                * smoothstep(0.35, 0.6, finish_noise(r * 0.025 + vec2<f32>(9.0, o)));
        }
        mirrored *= max(1.0 + worn * (0.3 * bright - 0.16 * dull), 0.0);
    }
    // A matte surface takes the light of everything above it, each direction by its angle
    // (the environment's diffuse layer, as 2D materials and the path tracer take it).
    var colour = (environment_layer(turned(n), 9u) * diffuse * strength
        + mirrored * fresnel * strength)
        * sky_visibility(in.world, n);
    // For the reflections view: what each light added, painted in its own colour.
    let surroundings = colour;
    var painted = vec3<f32>(0.0);
    var mark = colour;
    for (var i = 0u; i < u32(object.counts.x); i++) {
        if i > 0u {
            painted += light_hue(u32(lights[i - 1u].shadow_info.w)) * min(dot(colour - mark, vec3<f32>(0.2126, 0.7152, 0.0722)), 1.5);
        }
        mark = colour;
        let light = lights[i];
        if light.position.w > 2.5 {
            // An area light: a glowing panel. Its mirror image shows in the surface,
            // blurred by the surface's lobe; matte surfaces take light by how much of
            // their view it fills.
            let facing = light.direction.xyz;
            let hw = length(light.right.xyz);
            let hh = length(light.up.xyz);
            let ax_r = light.right.xyz / hw;
            let ax_u = light.up.xyz / hh;
            let to = light.position.xyz - in.world;
            // Where the mirrored view ray meets the panel's plane, held to the panel.
            let r = reflect(-v, n);
            let closing = dot(r, facing);
            var offset = vec3<f32>(0.0);
            if closing < -1e-4 {
                let t = dot(to, facing) / closing;
                offset = r * t - to;
            }
            let on = light.position.xyz
                + ax_r * clamp(dot(offset, ax_r), -hw, hw)
                + ax_u * clamp(dot(offset, ax_u), -hh, hh);
            // A feathered panel fades towards its edges, so its reflection does too.
            var edge = 1.0;
            let feather = light.right.w;
            if feather > 0.0 {
                let across = abs(dot(offset, ax_r)) / max(hw, 1e-3);
                let down = abs(dot(offset, ax_u)) / max(hh, 1e-3);
                edge = (1.0 - smoothstep(1.0 - feather, 1.0, across)) * (1.0 - smoothstep(1.0 - feather, 1.0, down));
            }
            let lp = on - in.world;
            let d = max(length(lp), 1.0);
            let l = lp / d;
            let nl = dot(n, l);
            // Only the panel's front glows, and only towards surfaces that face it.
            let front = max(dot(-normalize(to), facing), 0.0);
            if nl > 0.0 && front > 0.0 {
                let h = normalize(l + v);
                let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
                var seen = 0.0;
                let bx = max(ax, 0.004);
                let by = max(ay, 0.004);
                if brushed > 0.0 {
                    seen = ggx_grain(h, n, g, bx, by) * 3.14159265 * bx * by;
                } else {
                    seen = ggx(max(dot(n, h), 0.0), bx) * 3.14159265 * bx * bx;
                }
                let solid = 4.0 * hw * hh * front / max(dot(to, to), 1.0);
                // A lobe wider than the panel looks past it on every side: the panel's
                // light is spread over the lobe, so its reflection dims by the share
                // of the lobe it fills.
                let fills = clamp(solid / (4.0 * 3.14159265 * bx * by), 0.0, 1.0);
                let reaches = shadow_of(in.world, n, light.shadow, light.shadow_info);
                colour += (light.colour.rgb * f * min(seen, 1.0) * fills * grain_gain * edge
                    + diffuse / 3.14159265 * light.colour.rgb * solid * max(dot(n, normalize(to)), 0.0)) * reaches;
            }
            continue;
        }
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
        var lobe = ggx(nh, a);
        if brushed > 0.0 { lobe = ggx_grain(h, n, g, max(ax, 0.002), max(ay, 0.002)); }
        let specular = lobe * smith(nv, nl, sqrt(ax * ay)) * f / max(4.0 * nv * nl, 1e-4);
        colour += (diffuse / 3.14159265 * (vec3<f32>(1.0) - f) + specular) * radiance * nl * 3.14159265;
    }
    // Its own light: the colour added as it is, whatever lights the scene.
    colour += object.surface.rgb * in.tint.rgb * object.instancing.y;
    if mapping {
        let count = u32(object.counts.x);
        if count > 0u {
            painted += light_hue(u32(lights[count - 1u].shadow_info.w)) * min(dot(colour - mark, vec3<f32>(0.2126, 0.7152, 0.0722)), 1.5);
        }
        // The surroundings (studio or panorama) as a faint grey, so the form still reads.
        let grey = 0.1 * min(dot(surroundings, vec3<f32>(0.2126, 0.7152, 0.0722)), 1.0) + 0.012;
        return vec4<f32>(painted + vec3<f32>(grey), 1.0);
    }
    return vec4<f32>(colour, 1.0);
}
// A colour for light number `i` (its node's number): well separated hues, the same as
// `veylvik_scene::light_hue`, which the viewer's guides use.
fn light_hue(i: u32) -> vec3<f32> {
    let h = fract(f32(i) * 0.61803399 + 0.08) * 6.0;
    let rgb = clamp(abs(vec3<f32>(h - 3.0, h - 2.0, h - 4.0)) * vec3<f32>(1.0, -1.0, -1.0) + vec3<f32>(-1.0, 2.0, 2.0), vec3<f32>(0.0), vec3<f32>(1.0));
    return mix(vec3<f32>(1.0), rgb, 0.85);
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
// A card's picture: bilinear at its own pixel centres; premultiplied, unlit.
fn card_colour(in: Varying) -> vec4<f32> {
    let q = in.uv * object.params.zw - vec2<f32>(0.5);
    let i = vec2<i32>(floor(q));
    let f = fract(q);
    let c = mix(mix(texel(i.x, i.y), texel(i.x + 1, i.y), f.x),
                mix(texel(i.x, i.y + 1), texel(i.x + 1, i.y + 1), f.x), f.y);
    // The card's tint, and its opacity in place of a roughness.
    return vec4<f32>(c.rgb * object.surface.rgb * in.tint.rgb, c.a) * object.surface.a;
}
@fragment
fn card(in: Varying) -> @location(0) vec4<f32> {
    return card_colour(in);
}
// For the depth picture: the card is an object where it is more than half opaque.
@fragment
fn card_solid(in: Varying) -> @location(0) vec4<f32> {
    let c = card_colour(in);
    if c.a < 0.5 {
        discard;
    }
    return c;
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
        if light.position.w > 2.5 {
            // An area light: by how much of the surface's view its panel fills.
            let to = light.position.xyz - in.world;
            let d2 = max(dot(to, to), 1.0);
            l = to / sqrt(d2);
            let front = max(dot(-l, light.direction.xyz), 0.0);
            radiance *= 4.0 * length(light.right.xyz) * length(light.up.xyz) * front / d2 / 3.14159265;
        } else if light.position.w < 1.5 {
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
    let ambient_light = dot(environment_layer(turned(n), 9u), vec3<f32>(0.2126, 0.7152, 0.0722)) * object.counts.y;
    let enclosed = ambient_light * (1.0 - sky_visibility(in.world, n));
    let shade = clamp((blocked + enclosed) / max(direct + ambient_light, 1e-4), 0.0, 1.0);
    // Strength above 1 deepens a shadow that the scene's other lights wash out.
    return vec4<f32>(0.0, 0.0, 0.0, clamp(shade * clamp(object.relief.z, 0.0, 8.0), 0.0, 1.0));
}
