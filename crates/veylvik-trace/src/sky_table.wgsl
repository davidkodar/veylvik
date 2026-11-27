// The environment as a table to draw directions from: its 256 x 128 level (layer 2) by
// brightness times each row's share of the sphere (sin of its angle from the pole), as
// running sums along each row and down the rows, so a direction is drawn where light is.
// Laid out: each row's running share (256 a row, ending at 1), each row's running share
// of the whole (128, ending at 1), then the whole's sum. Appended: the environment
// lookups, which read `environment_map`.
@group(0) @binding(0) var<storage, read> environment_map: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> table: array<f32>;
const ACROSS: u32 = 256u;
const DOWN: u32 = 128u;
const PI: f32 = 3.14159265;
// The direction through the middle of texel (x, y) of the environment's layout (the
// inverse of `environment_layer`'s, which turns y round first).
fn texel_direction(u: f32, v: f32) -> vec3<f32> {
    let phi = (u - 0.5) * 2.0 * PI;
    let theta = v * PI;
    return vec3<f32>(sin(theta) * cos(phi), -cos(theta), sin(theta) * sin(phi));
}
fn weight(x: u32, y: u32) -> f32 {
    let u = (f32(x) + 0.5) / f32(ACROSS);
    let v = (f32(y) + 0.5) / f32(DOWN);
    let c = environment_layer(texel_direction(u, v), 2u);
    // Never nothing, so every direction can be drawn and the result stays exact.
    return max(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0) * sin(v * PI) + 1e-6;
}
@compute @workgroup_size(64)
fn sky_rows(@builtin(global_invocation_id) gid: vec3<u32>) {
    let y = gid.x;
    if y >= DOWN { return; }
    var sum = 0.0;
    for (var x = 0u; x < ACROSS; x++) {
        sum += weight(x, y);
        table[y * ACROSS + x] = sum;
    }
    for (var x = 0u; x < ACROSS; x++) {
        table[y * ACROSS + x] /= sum;
    }
    table[ACROSS * DOWN + y] = sum;
}
@compute @workgroup_size(1)
fn sky_down() {
    var sum = 0.0;
    for (var y = 0u; y < DOWN; y++) {
        sum += table[ACROSS * DOWN + y];
        table[ACROSS * DOWN + y] = sum;
    }
    for (var y = 0u; y < DOWN; y++) {
        table[ACROSS * DOWN + y] /= sum;
    }
    table[ACROSS * DOWN + DOWN] = sum;
}
