// slider x 0 1 0.3
// slider y 0 1 0.32
// slider bright 0 4 1
// slider rays 0 2 1
// slider streak 0 2 1
// slider ring 0 2 1
// slider ghosts 0 2 1
// colour tint 1 0.93 0.82

fn chance(n: f32) -> f32 {
    return fract(sin(n * 12.9898 + 4.1) * 43758.5453);
}
fn band(r: f32, centre: f32, width: f32) -> f32 {
    return exp(-pow((r - centre) / width, 2.0));
}
// What a lens makes of a bright light at x, y (fractions of the frame): its glow, a
// burst of thin rays of uneven length, a long streak across it, a wide faint coloured
// ring round it (warm on the inside, teal on the outside, strongest to the sides),
// and soft coloured ghosts on the line from the light through the frame's middle.
// Key x and y to move the light, or connect a scene and set Follow to one of its
// objects: the flare then sits on that object, fades as it nears the frame's edge or
// slides behind another object, and goes out when it is behind the camera. Everything else moves with the light.
// Connect a picture to Second to say where the light can be seen: the flare is as strong
// as that picture's alpha round the light (a matte of the open sky, or the light's own
// picture with dunes, clouds or dust taken away), so it dims and goes out as anything in
// the picture passes in front of the light.
fn shade(at: vec2<f32>) -> vec4<f32> {
    let p = (at - size * 0.5) / size.y;
    if followed < -0.5 {
        return vec4<f32>(0.0);
    }
    var spot = vec2<f32>(u.x, u.y);
    var seen = 1.0;
    if followed > 0.5 {
        spot = follow / size;
        let inside = min(min(spot.x, 1.0 - spot.x), min(spot.y, 1.0 - spot.y));
        seen = smoothstep(-0.02, 0.06, inside) * follow_seen;
    }
    if has_second() {
        // The share of the light's disc the Second picture lets through: its alpha at the
        // middle and at eight places round it.
        let reach = size.y * 0.012;
        var open = second(spot * size).a;
        for (var k = 0; k < 8; k++) {
            let a = f32(k) * 0.7853982;
            open += second(spot * size + vec2<f32>(cos(a), sin(a)) * reach).a;
        }
        seen *= clamp(open / 9.0, 0.0, 1.0);
    }
    let light = (spot - vec2<f32>(0.5)) * vec2<f32>(size.x / size.y, 1.0);
    let d = p - light;
    let r = length(d);
    let turn = atan2(d.y, d.x) / 6.2831853 + 0.5;
    var sum = u.tint * (exp(-r * r / 0.00012) * 5.0 + 0.00022 / (r * r + 0.0005) + exp(-r * 11.0) * 0.16);
    // Rays: three sets of thin spikes, each spike its own length, width and strength.
    var burst = 0.0;
    for (var k = 0; k < 3; k++) {
        let count = 13.0 + 11.0 * f32(k);
        let along = turn * count + 0.37 * f32(k);
        let spike = floor(along) - count * floor(floor(along) / count);
        let across = fract(along) - 0.5;
        let lot = chance(spike + 31.0 * f32(k));
        let reach = 0.016 + 0.11 * lot * lot * lot;
        let width = 0.035 + 0.07 * chance(spike * 1.7 + 5.0 + f32(k));
        burst += exp(-across * across / (width * width)) * exp(-r / reach) * (0.35 + 0.65 * lot);
    }
    sum += mix(u.tint, vec3<f32>(0.80, 0.88, 1.0), 0.55) * burst * 0.42 * u.rays;
    // The streak: a thin bright line and a wider faint one.
    let cold = vec3<f32>(0.74, 0.82, 1.0);
    sum += cold * (exp(-abs(d.y) * 420.0) * exp(-abs(d.x) * 3.2) * 0.55 + exp(-abs(d.y) * 70.0) * exp(-abs(d.x) * 6.0) * 0.04) * u.streak;
    // The ring round the light.
    let sides = 0.22 + 0.78 * pow(abs(cos(turn * 6.2831853)), 3.0);
    sum += (vec3<f32>(1.0, 0.40, 0.10) * band(r, 0.60, 0.07) * 0.030
        + vec3<f32>(0.90, 0.62, 0.20) * band(r, 0.665, 0.04) * 0.010
        + vec3<f32>(0.04, 0.52, 0.72) * band(r, 0.735, 0.075) * 0.040) * sides * u.ring;
    // Ghosts: before the middle, at it and past it.
    for (var k = 0; k < 6; k++) {
        let t = f32(k);
        let place = light * (0.58 - t * 0.37);
        let radius = 0.022 + 0.045 * chance(t + 2.0) + 0.012 * t;
        let away = length(p - place) / radius;
        let disc = smoothstep(1.0, 0.55, away) * (0.30 + 0.70 * smoothstep(0.0, 1.0, away));
        let hue = vec3<f32>(0.5 + 0.5 * cos(t * 1.9 + 2.0), 0.5 + 0.5 * cos(t * 1.9 + 0.2), 0.5 + 0.5 * cos(t * 1.9 + 4.4));
        sum += mix(u.tint, hue, 0.7) * disc * 0.020 * u.ghosts;
    }
    let light_out = sum * u.bright * seen;
    // Mostly added to what is behind: only the hot middle covers it.
    let alpha = clamp(max(light_out.r, max(light_out.g, light_out.b)) - 0.6, 0.0, 1.0);
    return vec4<f32>(light_out, alpha);
}
