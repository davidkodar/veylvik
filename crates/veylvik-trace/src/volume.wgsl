// What a volume (smoke, mist, haze) is at a point, shared by the interactive renderer and
// the path tracer so both see the same medium. Binding-free: each renderer reads a volume's
// eight vec4s (MEDIUM_VEC4S, packed by veylvik-trace's `Medium`) from its own buffer and
// calls `medium_of`. Distances are scene units (pixels at the home view); density is the
// medium's extinction per unit of length in the world.
const MEDIUM_VEC4S: u32 = 8u;
struct Medium {
    // World to the volume's own box frame, as rows (x = dot(row, (p, 1))).
    to_box: mat3x4<f32>,
    // Half the box's size; the shape (0 box, 1 sphere, 2 ground mist).
    half: vec3<f32>,
    shape: u32,
    // Extinction per world unit; the share absorbed; the scattering's forward lean (-1 to 1);
    // how far in from its edges it fades, as a share of its half size.
    sigma: f32,
    absorption: f32,
    forward: f32,
    softness: f32,
    // The colour of the light it scatters; how much noise breaks it up.
    colour: vec3<f32>,
    noise: f32,
    // The noise's largest clumps (units); its layers (1 to 6); which noise.
    noise_size: f32,
    detail: f32,
    seed: u32,
    // How far the noise has drifted, in the box frame.
    drift: vec3<f32>,
}
fn medium_of(a0: vec4<f32>, a1: vec4<f32>, a2: vec4<f32>, a3: vec4<f32>, a4: vec4<f32>, a5: vec4<f32>, a6: vec4<f32>, a7: vec4<f32>) -> Medium {
    return Medium(mat3x4<f32>(a0, a1, a2), a3.xyz, u32(a3.w), a4.x, a4.y, a4.z, a4.w, a5.xyz, a5.w, a6.x, a6.y, bitcast<u32>(a6.z), a7.xyz);
}
fn medium_local(m: Medium, p: vec3<f32>) -> vec3<f32> {
    return vec4<f32>(p, 1.0) * m.to_box;
}
// Where a world ray (o + t d, t >= 0) is inside the volume's box: (enter, leave); enter >
// leave when it misses.
fn medium_span(m: Medium, o: vec3<f32>, d: vec3<f32>) -> vec2<f32> {
    let lo = medium_local(m, o);
    let ld = (vec4<f32>(d, 0.0) * m.to_box);
    let inv = 1.0 / select(ld, vec3<f32>(1e-20), abs(ld) < vec3<f32>(1e-20));
    let a = (-m.half - lo) * inv;
    let b = (m.half - lo) * inv;
    let near = max(max(min(a.x, b.x), min(a.y, b.y)), max(min(a.z, b.z), 0.0));
    let far = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
    return vec2<f32>(near, far);
}
fn medium_hash(p: vec3<i32>, seed: u32) -> f32 {
    var h = (bitcast<u32>(p.x) * 73856093u) ^ (bitcast<u32>(p.y) * 19349663u) ^ (bitcast<u32>(p.z) * 83492791u) ^ (seed * 2654435761u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}
// Smooth value noise in 0 to 1.
fn medium_noise(p: vec3<f32>, seed: u32) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let x00 = mix(medium_hash(i, seed), medium_hash(i + vec3<i32>(1, 0, 0), seed), s.x);
    let x10 = mix(medium_hash(i + vec3<i32>(0, 1, 0), seed), medium_hash(i + vec3<i32>(1, 1, 0), seed), s.x);
    let x01 = mix(medium_hash(i + vec3<i32>(0, 0, 1), seed), medium_hash(i + vec3<i32>(1, 0, 1), seed), s.x);
    let x11 = mix(medium_hash(i + vec3<i32>(0, 1, 1), seed), medium_hash(i + vec3<i32>(1, 1, 1), seed), s.x);
    return mix(mix(x00, x10, s.y), mix(x01, x11, s.y), s.z);
}
// Layers of noise, each twice as fine and half as strong; a fractional `detail` fades the
// last layer in.
fn medium_fbm(p: vec3<f32>, detail: f32, seed: u32) -> f32 {
    var sum = 0.0;
    var weight = 0.0;
    var amplitude = 1.0;
    var q = p;
    for (var k = 0u; k < 6u; k++) {
        let share = clamp(detail - f32(k), 0.0, 1.0);
        if share <= 0.0 { break; }
        sum += medium_noise(q, seed + k * 7919u) * amplitude * share;
        weight += amplitude * share;
        amplitude *= 0.5;
        q = q * 2.03 + vec3<f32>(17.1, 31.7, 47.3);
    }
    return sum / max(weight, 1e-6);
}
// The extinction (per world unit) at world point `p`.
fn medium_density(m: Medium, p: vec3<f32>) -> f32 {
    let q = medium_local(m, p);
    var edge: f32;
    var falloff = 1.0;
    if m.shape == 1u {
        edge = 1.0 - length(q / m.half);
    } else {
        let a = abs(q) / m.half;
        edge = 1.0 - max(max(a.x, a.y), a.z);
        if m.shape == 2u {
            // Lying on the bottom (+y is down), thinning upwards.
            let up = clamp((m.half.y - q.y) / (2.0 * m.half.y), 0.0, 1.0);
            falloff = exp(-4.0 * up);
            // Only the sides and the top fade; it rests on the bottom.
            edge = 1.0 - max(max(a.x, a.z), select(0.0, a.y, q.y < 0.0));
        }
    }
    if edge <= 0.0 { return 0.0; }
    var fade = 1.0;
    if m.softness > 0.0 { fade = smoothstep(0.0, m.softness, edge); }
    var clumps = 1.0;
    if m.noise > 0.0 {
        let n = medium_fbm((q + m.drift) / m.noise_size, m.detail, m.seed);
        clumps = mix(1.0, 2.0 * clamp((n - 0.5) * 3.0 + 0.5, 0.0, 1.0), m.noise);
    }
    return m.sigma * falloff * fade * clumps;
}
// The most extinction anywhere in the volume (for drawing free paths through it).
fn medium_most(m: Medium) -> f32 {
    return m.sigma * (1.0 + m.noise);
}
// The share of light it scatters (the rest of what it stops is absorbed), by colour.
fn medium_albedo(m: Medium) -> vec3<f32> {
    return clamp(m.colour, vec3<f32>(0.0), vec3<f32>(1.0)) * (1.0 - m.absorption);
}
// The Henyey-Greenstein phase function: how much of the light arriving along `from_light`
// (travelling) goes on along `onward`, per steradian.
fn medium_phase(g: f32, travelling: vec3<f32>, onward: vec3<f32>) -> f32 {
    let c = dot(travelling, onward);
    let d = 1.0 + g * g - 2.0 * g * c;
    return (1.0 - g * g) / (12.566371 * d * sqrt(max(d, 1e-6)));
}
// A direction drawn by the phase function about `travelling`, from two random numbers.
fn medium_draw(g: f32, travelling: vec3<f32>, u1: f32, u2: f32) -> vec3<f32> {
    var c: f32;
    if abs(g) < 1e-3 {
        c = 1.0 - 2.0 * u1;
    } else {
        let s = (1.0 - g * g) / (1.0 - g + 2.0 * g * u1);
        c = (1.0 + g * g - s * s) / (2.0 * g);
    }
    let sine = sqrt(max(1.0 - c * c, 0.0));
    let phi = 6.2831853 * u2;
    var t = cross(travelling, vec3<f32>(0.0, 1.0, 0.0));
    if dot(t, t) < 1e-6 { t = cross(travelling, vec3<f32>(1.0, 0.0, 0.0)); }
    t = normalize(t);
    let b = cross(travelling, t);
    return normalize(travelling * c + (t * cos(phi) + b * sin(phi)) * sine);
}
