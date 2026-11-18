// slider x 0 1 0.25
// slider y 0 1 0.25
// slider bright 0 4 1
// slider ghosts 0 2 1
// slider streak 0 2 1
// slider ring 0 2 1
// colour tint 1 0.85 0.65

// A lens flare from a light at x, y (fractions of the frame): the light's glow, a wide
// streak through it, a faint ring round it, and ghosts strung along the line from the
// light through the middle of the frame, each a soft disc tinted a little differently.
fn shade(at: vec2<f32>) -> vec4<f32> {
    let p = (at - size * 0.5) / size.y;
    let light = (vec2<f32>(u.x, u.y) - vec2<f32>(0.5)) * vec2<f32>(size.x / size.y, 1.0);
    let d = p - light;
    let r = length(d);
    var sum = u.tint * (0.012 / (r * r + 0.0004) * 0.03 + exp(-r * 9.0) * 0.5);
    sum += u.tint * exp(-abs(d.y) * 160.0) * exp(-abs(d.x) * 2.2) * 0.9 * u.streak;
    let halo = exp(-pow((r - 0.26) / 0.012, 2.0));
    sum += vec3<f32>(0.6, 0.75, 1.0) * halo * 0.12 * u.ring;
    for (var k = 0; k < 7; k++) {
        let t = f32(k);
        // Along the line through the middle: before it, at it and past it.
        let place = light * (0.62 - t * 0.33);
        let radius = 0.018 + 0.03 * fract(t * 0.618 + 0.3) + 0.012 * t * fract(t * 0.37);
        let away = length(p - place) / radius;
        let disc = smoothstep(1.0, 0.72, away) * (0.35 + 0.65 * smoothstep(0.0, 1.0, away));
        let hue = vec3<f32>(0.5 + 0.5 * cos(t * 1.7), 0.5 + 0.5 * cos(t * 1.7 + 2.1), 0.5 + 0.5 * cos(t * 1.7 + 4.2));
        sum += mix(u.tint, hue, 0.55) * disc * 0.10 * u.ghosts;
    }
    let out = sum * u.bright;
    let alpha = clamp(max(out.r, max(out.g, out.b)), 0.0, 1.0);
    return vec4<f32>(out, alpha);
}
