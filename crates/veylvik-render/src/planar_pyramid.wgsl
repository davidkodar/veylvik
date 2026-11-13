// Area-filtered 2:1 reduction, including odd dimensions. Pixel centres retain
// the same normalized coordinates at every level. No host image readback.
@group(0) @binding(0) var<uniform> size: vec4<u32>;
@group(0) @binding(1) var<storage,read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage,read_write> reduced: array<vec4<f32>>;
@compute @workgroup_size(8,8)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy>=size.zw) {return;}
    let ratio=vec2<f32>(size.xy)/vec2<f32>(size.zw);
    let lo=vec2<f32>(id.xy)*ratio; let hi=lo+ratio;
    var sum=vec4<f32>(0.0); var weight=0.0;
    for(var y=u32(floor(lo.y));y<min(u32(ceil(hi.y)),size.y);y++) {
        for(var x=u32(floor(lo.x));x<min(u32(ceil(hi.x)),size.x);x++) {
            let w=max(0.0,min(hi.x,f32(x+1u))-max(lo.x,f32(x)))*max(0.0,min(hi.y,f32(y+1u))-max(lo.y,f32(y)));
            sum+=source[y*size.x+x]*w; weight+=w;
        }
    }
    reduced[id.y*size.z+id.x]=sum/weight;
}
