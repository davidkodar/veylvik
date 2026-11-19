// slider stars 0 3 1
// slider clouds 0 3 1
// slider band 0 3 1
// slider seed 0 50 4

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}
fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = mix(mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), w.x),
                mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), w.x), w.y);
    let b = mix(mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), w.x),
                mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), w.x), w.y);
    return mix(a, b, w.z);
}
// The whole sky as a panorama (left to right all the way round, top to bottom from
// pole to pole): faint clouds of gas, a brighter band of unresolved stars across it,
// and the far stars themselves, coloured by how hot each is. Made on the sphere, so
// it has no seam.
fn shade(at: vec2<f32>) -> vec4<f32> {
    let lon = (at.x / size.x) * 6.2831853;
    let lat = (0.5 - at.y / size.y) * 3.1415927;
    let dir = vec3<f32>(cos(lat) * cos(lon), sin(lat), cos(lat) * sin(lon));
    var gas = 0.0;
    var fine = 0.0;
    var scale = 1.0;
    // Each finer layer is turned against the one before, so no grid shows.
    var turned = dir;
    for (var k = 0; k < 6; k++) {
        gas += noise3(turned * 2.2 * scale + vec3<f32>(u.seed)) / scale;
        fine += noise3(turned * 9.0 * scale + vec3<f32>(u.seed * 2.0)) / scale;
        scale *= 2.0;
        turned = vec3<f32>(0.80 * turned.x - 0.60 * turned.y, 0.48 * turned.x + 0.64 * turned.y - 0.60 * turned.z,
                           0.36 * turned.x + 0.48 * turned.y + 0.80 * turned.z);
    }
    gas = gas / 1.97;
    fine = fine / 1.97;
    // A tilted great circle: the plane of the galaxy we are in.
    let across = dot(dir, normalize(vec3<f32>(0.25, 0.9, 0.36)));
    let lane = exp(-across * across / 0.035);
    let dusty = smoothstep(0.35, 0.7, fine);
    let cloud = pow(smoothstep(0.38, 0.85, gas), 2.0);
    var colour = vec3<f32>(0.035, 0.022, 0.075) * cloud * u.clouds
        + vec3<f32>(0.012, 0.02, 0.05) * pow(smoothstep(0.3, 0.9, 1.0 - gas), 2.0) * u.clouds
        + vec3<f32>(0.060, 0.052, 0.058) * lane * (0.35 + 0.65 * dusty) * u.band;
    // Stars, scattered freely: each cell of a coarse grid may hold one anywhere inside
    // it, so a pixel asks its own cell and the eight round it. Many faint and small, few
    // bright and larger; coloured by how hot each is. A second, finer grid holds the
    // multitude too faint to tell apart.
    for (var layer = 0; layer < 2; layer++) {
        let pitch = 11.0 - 6.0 * f32(layer);
        let home = floor(at / pitch);
        for (var j = -1; j <= 1; j++) {
            for (var i = -1; i <= 1; i++) {
                let cell = home + vec2<f32>(f32(i), f32(j));
                let key = vec3<f32>(cell, u.seed + 50.0 * f32(layer));
                if hash3(key) < 0.26 + 0.34 * lane {
                    let spot = (cell + vec2<f32>(hash3(key + vec3<f32>(0.0, 0.0, 7.0)), hash3(key + vec3<f32>(0.0, 0.0, 13.0)))) * pitch;
                    let d = length(at - spot);
                    let rank = hash3(key + vec3<f32>(0.0, 0.0, 21.0));
                    var power = 0.03 + 0.30 * pow(rank, 3.0) + 4.5 * pow(rank, 18.0);
                    var radius = 0.55 + 1.5 * pow(rank, 12.0);
                    if layer == 1 {
                        power = 0.015 + 0.07 * rank;
                        radius = 0.5;
                    }
                    let heat = hash3(key + vec3<f32>(0.0, 0.0, 34.0));
                    let tone = mix(mix(vec3<f32>(1.0, 0.60, 0.36), vec3<f32>(1.0, 0.95, 0.86), smoothstep(0.0, 0.45, heat)),
                                   vec3<f32>(0.64, 0.77, 1.0), smoothstep(0.55, 1.0, heat));
                    colour += tone * power * exp(-d * d / (radius * radius)) * u.stars;
                }
            }
        }
    }
    return vec4<f32>(colour, 1.0);
}
