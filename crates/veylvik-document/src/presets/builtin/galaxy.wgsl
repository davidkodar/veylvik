// slider x 0 1 0.72
// slider y 0 1 0.3
// slider size 0.05 2 0.45
// slider angle -180 180 20
// slider tilt 0 88 55
// slider wind 1 6 2.6
// slider arms 0 1 1
// slider bulge 0.2 3 1
// slider blue 0 1 0.75
// slider bright 0 4 1
// slider seed 0 50 3

fn hash(p: vec2<f32>) -> f32 {
    // Steady at any size of p (a sine of a large number is not).
    var q = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    q += dot(q, q.yzx + vec3<f32>(33.33));
    return fract((q.x + q.y) * q.z);
}
fn slope(i: vec2<f32>) -> vec2<f32> {
    let turn = hash(i) * 6.2831853;
    return vec2<f32>(cos(turn), sin(turn));
}
// Smooth noise about 0.5, from slopes at the grid's corners, so it has no facets.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(slope(i), f);
    let b = dot(slope(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0));
    let c = dot(slope(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0));
    let d = dot(slope(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0));
    return clamp(0.5 + 0.75 * mix(mix(a, b, w.x), mix(c, d, w.x), w.y), 0.0, 1.0);
}
// A far galaxy as a telescope shows it: faint, soft and without an edge. Open ragged
// arms (arms 0 gives an elliptical), dust on the arms' inner sides, a small warm bulge,
// and for a galaxy seen edge on a dark lane across it. x and y place its middle as
// fractions of the frame; size is its radius as a fraction of the frame's height.
fn shade(at: vec2<f32>) -> vec4<f32> {
    let away = (at - size * vec2<f32>(u.x, u.y)) / (size.y * u.size);
    let turn = radians(u.angle);
    let flat = vec2<f32>(away.x * cos(turn) + away.y * sin(turn), away.y * cos(turn) - away.x * sin(turn));
    let p = vec2<f32>(flat.x, flat.y / cos(radians(u.tilt)));
    let r = length(p);
    if r > 1.0 {
        return vec4<f32>(0.0);
    }
    let around = atan2(p.y, p.x);
    var cloud = 0.0;
    var clumps = 0.0;
    var scale = 1.0;
    // Each finer layer of noise is turned against the one before, so no grid shows.
    var q = p;
    for (var k = 0; k < 5; k++) {
        cloud += noise(q * 3.0 * scale + vec2<f32>(u.seed * 7.0)) / scale;
        clumps += noise(q * 14.0 * scale + vec2<f32>(u.seed * 3.0)) / scale;
        scale *= 2.0;
        q = vec2<f32>(q.x * 0.7986 - q.y * 0.6018, q.x * 0.6018 + q.y * 0.7986);
    }
    cloud = cloud / 1.94;
    clumps = clumps / 1.94;
    let phase = 2.0 * (around - u.wind * log(r + 0.03)) + (cloud - 0.5) * 5.0;
    let ridge = 0.5 + 0.5 * cos(phase);
    let reach = smoothstep(0.03, 0.14, r) * smoothstep(0.85, 0.3, r);
    let arm = ridge * ridge * reach * u.arms;
    let lane = pow(0.5 + 0.5 * cos(phase + 1.0), 4.0) * reach * u.arms;
    let disc = exp(-r * 4.6);
    let old = disc * mix(1.0, 0.3, u.arms) + exp(-r * 1.9) * 0.035;
    let young = disc * arm * (0.25 + 1.5 * clumps * clumps) * 2.6;
    let dust = 1.0 - 0.7 * lane * (0.4 + 0.6 * clumps);
    let middle = length(flat * vec2<f32>(1.0, 1.5)) / u.bulge;
    let bulge = exp(-middle * 22.0) * 1.6 + exp(-middle * middle * 500.0) * 1.4;
    let edge_on = smoothstep(68.0, 84.0, u.tilt);
    let band = 1.0 - 0.85 * edge_on * exp(-pow(flat.y / 0.012, 2.0)) * smoothstep(0.7, 0.1, abs(flat.x))
        * (0.6 + 0.4 * noise(vec2<f32>(flat.x * 40.0, u.seed)));
    let warm = vec3<f32>(1.0, 0.80, 0.58);
    let cool = mix(vec3<f32>(0.95, 0.88, 0.80), vec3<f32>(0.55, 0.70, 1.0), u.blue);
    let knots = pow(clumps, 7.0) * arm * disc * 14.0;
    let light = (warm * (old * 0.55 + bulge) + cool * young + vec3<f32>(1.0, 0.45, 0.6) * knots * 0.3)
        * dust * band * u.bright * smoothstep(0.95, 0.25, r);
    let alpha = clamp(max(light.r, max(light.g, light.b)), 0.0, 1.0);
    return vec4<f32>(light, alpha);
}
