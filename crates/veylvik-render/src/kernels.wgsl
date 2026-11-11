struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> second: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
fn inside(gid:vec3<u32>)->bool { return gid.x<p.size_op.x && gid.y<p.size_op.y; }
fn index(gid:vec3<u32>)->u32 { return gid.y*p.size_op.x+gid.x; }
fn sample_source(x:i32,y:i32)->vec4<f32> {
    if x<0 || y<0 || x>=i32(p.size_op.x) || y>=i32(p.size_op.y) {return vec4<f32>(0.0);}
    return source[u32(y)*p.size_op.x+u32(x)];
}
fn noise_hash(value:u32)->u32 {
    var v=value;v=(v^(v>>16u))*0x7feb352du;v=(v^(v>>15u))*0x846ca68bu;return v^(v>>16u);
}
@compute @workgroup_size(8,8)
fn solid(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=vec4<f32>(p.values.rgb*p.values.a,p.values.a);
}
@compute @workgroup_size(8,8)
fn noise(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let x=u32(f32(gid.x)/p.values.x);let y=u32(f32(gid.y)/p.values.x);
    let n=f32(noise_hash(x+y*65537u+p.size_op.w)&65535u)/65535.0;
    result[index(gid)]=vec4<f32>(vec3<f32>(0.1,0.5,1.0)*n,n);
}
@compute @workgroup_size(8,8)
fn exposure(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);result[i]=vec4<f32>(source[i].rgb*p.values.x,source[i].a);
}
@compute @workgroup_size(8,8)
fn transform(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let pos=vec2<f32>(gid.xy)-p.values.xy;let base=vec2<i32>(floor(pos));let f=fract(pos);
    let a=mix(sample_source(base.x,base.y),sample_source(base.x+1,base.y),f.x);
    let b=mix(sample_source(base.x,base.y+1),sample_source(base.x+1,base.y+1),f.x);
    result[index(gid)]=mix(a,b,f.y);
}
fn blur_axis(gid:vec3<u32>,axis:vec2<i32>)->vec4<f32> {
    let radius=i32(round(p.values.x));var total=vec4<f32>(0.0);
    for (var offset=-radius;offset<=radius;offset++) {
        let pos=vec2<i32>(gid.xy)+axis*offset;total+=sample_source(pos.x,pos.y);
    }
    return total/f32(radius*2+1);
}
@compute @workgroup_size(8,8)
fn blur_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=blur_axis(gid,vec2<i32>(1,0));
}
@compute @workgroup_size(8,8)
fn blur_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=blur_axis(gid,vec2<i32>(0,1));
}
@compute @workgroup_size(8,8)
fn merge(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let foreground=second[i];result[i]=foreground+source[i]*(1.0-foreground.a);
}
@compute @workgroup_size(8,8)
fn image(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var color=vec4<f32>(0.0);
    if gid.x<u32(p.extra.x) && gid.y<u32(p.extra.y) {color=source[gid.y*u32(p.extra.x)+gid.x];}
    result[index(gid)]=color;
}
