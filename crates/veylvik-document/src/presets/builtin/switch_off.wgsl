// slider off 0 1 0

// An old television switched off: the picture collapses to a bright line across the
// screen, the line draws in to a point, and the point glows a moment and dies.
fn shade(at: vec2<f32>) -> vec4<f32> {
    if u.off <= 0.0 {
        return image(at);
    }
    let centre = size * 0.5;
    let fall = clamp(u.off / 0.36, 0.0, 1.0);
    let draw = clamp((u.off - 0.36) / 0.26, 0.0, 1.0);
    let die = clamp((u.off - 0.62) / 0.38, 0.0, 1.0);
    let tall = mix(1.0, 0.0035, 1.0 - pow(1.0 - fall, 3.0));
    let wide = mix(1.0, 0.004, draw * draw);
    let from_centre = at - centre;
    var light = vec3<f32>(0.0);
    if abs(from_centre.y) <= centre.y * tall && abs(from_centre.x) <= centre.x * wide {
        let seen = image(centre + from_centre / vec2<f32>(wide, tall));
        // The same light in less room: brighter, and whiter as it piles up.
        let most = max(seen.r, max(seen.g, seen.b));
        light = seen.rgb * (1.0 + 5.0 * fall) + vec3<f32>(most) * fall * 2.0;
    }
    let reach = centre.x * wide + 3.0;
    let across = smoothstep(reach, reach * 0.6, abs(from_centre.x));
    let glow = exp(-pow(from_centre.y / (1.5 + size.y * 0.0022), 2.0)) * across * fall * (1.0 - die);
    let halo = exp(-pow(from_centre.y / (size.y * 0.02), 2.0)) * across * fall * (1.0 - draw) * 0.12;
    let spot = exp(-dot(from_centre, from_centre) / pow(size.y * 0.011 * (1.0 - 0.6 * die), 2.0)) * draw * pow(1.0 - die, 2.0) * 5.0;
    light += vec3<f32>(0.82, 0.93, 1.0) * (glow * 2.2 + halo + spot);
    return vec4<f32>(light, 1.0);
}
