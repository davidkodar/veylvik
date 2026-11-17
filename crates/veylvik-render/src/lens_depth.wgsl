// Lens blur driven by a depth pass (second input, red): each pixel's disc is
// radius x |1 - focus / depth| capped at radius (depth <= 0 is infinitely far).
// values = (radius, highlights, threshold, focus). Prefilter boxes follow each pixel's
// own disc; the gather samples within it, and nearer samples count only when their own
// disc reaches the pixel, so sharp foreground does not smear into blurred background.
fn depth_coc(z:f32)->f32 {
    let radius=max(p.values.x,0.0);
    if z<=0.0 {return radius;}
    return min(radius*abs(1.0-p.values.w/z),radius);
}
fn depth_at(q:vec2<i32>)->f32 {return sample_second(q).r;}
@compute @workgroup_size(8,8)
fn lens_depth_prefilter_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let at=vec2<i32>(gid.xy);
    let s=lens_prefilter(depth_coc(depth_at(at)));var total=vec4<f32>(0.0);
    for(var o=-s;o<=s;o++) {total+=lens_boosted(sample_source(at.x+o,at.y));}
    result[index(gid)]=total/f32(2*s+1);
}
@compute @workgroup_size(8,8)
fn lens_depth_prefilter_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let at=vec2<i32>(gid.xy);
    let s=lens_prefilter(depth_coc(depth_at(at)));var total=vec4<f32>(0.0);
    for(var o=-s;o<=s;o++) {total+=sample_source(at.x,at.y+o);}
    result[index(gid)]=total/f32(2*s+1);
}
@compute @workgroup_size(8,8)
fn lens_depth_blur(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let at=vec2<i32>(gid.xy);
    let z=depth_at(at);
    let far=select(z,3.0e38,z<=0.0);
    let c=depth_coc(z);
    let own=sample_source(at.x,at.y);
    if c<0.5 {result[index(gid)]=own;return;}
    let count=lens_samples(c);
    var total=own;var weight=1.0;
    for(var n=0u;n<count;n++) {
        let r=c*sqrt((f32(n)+0.5)/f32(count));
        let a=f32(n)*2.39996323;
        let q=vec2<i32>(round(vec2<f32>(at)+vec2<f32>(cos(a),sin(a))*r));
        let zq=depth_at(q);
        let fq=select(zq,3.0e38,zq<=0.0);
        // Behind or level with this pixel, or near enough that its own disc reaches here.
        if fq>=far*0.98 || depth_coc(zq)>=r {total+=sample_source(q.x,q.y);weight+=1.0;}
    }
    result[index(gid)]=total/weight;
}
