// slider x 0 1 0.7
// slider y 0 1 0.3
// slider size 0.002 0.2 0.012
// slider glow 0 4 1
// slider spikes 0 2 0.5
// colour tint 1 0.78 0.5

// A far sun: a small white-hot point, the glow a lens gives it, and faint spikes. x and
// y place it as fractions of the frame; size is its radius as a fraction of the height.
fn shade(at: vec2<f32>) -> vec4<f32> {
    let p = (at - size * vec2<f32>(u.x, u.y)) / size.y;
    let r = length(p);
    let near = r / u.size;
    let point = exp(-near * near) * 30.0;
    let halo = (1.0 / (1.0 + near * near * 0.6) * 1.2 + 0.05 / (0.02 + r * 6.0)) * u.glow;
    let rays = (exp(-abs(p.y) * 220.0) + exp(-abs(p.x) * 220.0)) * exp(-r * 9.0) * u.spikes;
    let light = vec3<f32>(1.0) * point + mix(u.tint, vec3<f32>(1.0), 0.25) * (halo + rays) * smoothstep(0.9, 0.1, r);
    let alpha = clamp(max(light.r, max(light.g, light.b)), 0.0, 1.0);
    return vec4<f32>(light, alpha);
}
