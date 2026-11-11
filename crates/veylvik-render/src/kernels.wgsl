struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> second: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
fn sample_source(x: i32, y: i32) -> vec4<f32> {
    if x < 0 || y < 0 || x >= i32(p.size_op.x) || y >= i32(p.size_op.y) { return vec4<f32>(0.0); }
    return source[u32(y) * p.size_op.x + u32(x)];
}
fn noise_hash(value: u32) -> u32 {
    var v = value;
    v = (v ^ (v >> 16u)) * 0x7feb352du;
    v = (v ^ (v >> 15u)) * 0x846ca68bu;
    return v ^ (v >> 16u);
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= p.size_op.x || gid.y >= p.size_op.y { return; }
    let index = gid.y * p.size_op.x + gid.x;
    var color = vec4<f32>(0.0);
    switch p.size_op.z {
        case 0u: { color = vec4<f32>(p.values.rgb * p.values.a, p.values.a); }
        case 1u: {
            let x = u32(f32(gid.x) / p.values.x);
            let y = u32(f32(gid.y) / p.values.x);
            let n = f32(noise_hash(x + y * 65537u + p.size_op.w) & 65535u) / 65535.0;
            color = vec4<f32>(vec3<f32>(0.1,0.5,1.0) * n, n);
        }
        case 2u: { color = vec4<f32>(source[index].rgb * p.values.x, source[index].a); }
        case 3u: {
            let pos = vec2<f32>(gid.xy) - p.values.xy;
            let base = vec2<i32>(floor(pos));
            let f = fract(pos);
            let a = mix(sample_source(base.x,base.y),sample_source(base.x+1,base.y),f.x);
            let b = mix(sample_source(base.x,base.y+1),sample_source(base.x+1,base.y+1),f.x);
            color = mix(a,b,f.y);
        }
        case 4u: {
            let radius = i32(round(p.values.x));
            var total = vec4<f32>(0.0);
            for (var y = -radius; y <= radius; y++) {
                for (var x = -radius; x <= radius; x++) { total += sample_source(i32(gid.x)+x,i32(gid.y)+y); }
            }
            color = total / f32((radius*2+1)*(radius*2+1));
        }
        case 5u: { let foreground = second[index]; color = foreground + source[index] * (1.0-foreground.a); }
        case 6u: {
            if gid.x < u32(p.extra.x) && gid.y < u32(p.extra.y) { color = source[gid.y * u32(p.extra.x) + gid.x]; }
        }
        default: {}
    }
    result[index] = color;
}
