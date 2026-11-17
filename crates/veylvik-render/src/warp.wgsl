// Grid warp: `second` holds the mesh's point offsets (red, green: pixels), columns x
// rows, row by row. values = (columns, rows, amount). The offset field is Catmull-Rom
// across the mesh; each output pixel solves q + offset(q) = pixel for its source q by
// fixed-point steps, then samples there.
fn warp_point(x: i32, y: i32) -> vec2<f32> {
    let c = i32(p.values.x); let r = i32(p.values.y);
    let i = clamp(x, 0, c - 1); let j = clamp(y, 0, r - 1);
    return second[u32(j * c + i)].rg;
}
fn warp_weights(t: f32) -> vec4<f32> {
    let t2 = t * t; let t3 = t2 * t;
    return vec4<f32>(-0.5 * t3 + t2 - 0.5 * t, 1.5 * t3 - 2.5 * t2 + 1.0,
                     -1.5 * t3 + 2.0 * t2 + 0.5 * t, 0.5 * t3 - 0.5 * t2);
}
fn warp_offset(q: vec2<f32>) -> vec2<f32> {
    let size = vec2<f32>(p.size_op.xy);
    let g = clamp(q / size, vec2<f32>(0.0), vec2<f32>(1.0)) * (p.values.xy - vec2<f32>(1.0));
    let base = vec2<i32>(floor(g));
    let wx = warp_weights(g.x - floor(g.x));
    let wy = warp_weights(g.y - floor(g.y));
    var sum = vec2<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            sum += warp_point(base.x + i - 1, base.y + j - 1) * wx[i] * wy[j];
        }
    }
    return sum * p.values.z;
}
@compute @workgroup_size(8,8)
fn grid_warp(@builtin(global_invocation_id) gid: vec3<u32>) {
    if !inside(gid) { return; }
    let pixel = vec2<f32>(gid.xy) + vec2<f32>(0.5);
    var q = pixel;
    for (var k = 0; k < 8; k++) { q = pixel - warp_offset(q); }
    result[index(gid)] = bilinear(q - vec2<f32>(0.5));
}
