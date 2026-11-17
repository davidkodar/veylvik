// Bloom: light brighter than a threshold spreads as glow. values = (radius, intensity,
// threshold, knee), extra = (tint, streak). The bright part is blurred twice (a wide gaussian of
// the radius and a tight one of a fifth of it) and both are added to the image, so
// lights get a hot core and a long soft falloff. Alpha is left alone: the glow is
// added light, and shows over whatever the image is merged onto.
fn bloom_blur(gid:vec3<u32>,axis:vec2<i32>,radius:f32)->vec4<f32> {
    return blur_axis(gid,axis,i32(clamp(round(radius),1.0,256.0)),true,false);
}
@compute @workgroup_size(8,8)
fn bloom_bright(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let rgb=max(source[index(gid)].rgb,vec3<f32>(0.0));
    let l=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
    // Soft knee: light starts to count a little below the threshold, without a hard edge.
    let k=max(p.values.w*p.values.z,1e-5);
    let soft=clamp(l-p.values.z+k,0.0,2.0*k);
    let amount=max(soft*soft/(4.0*k),l-p.values.z)/max(l,1e-5);
    result[index(gid)]=vec4<f32>(rgb*amount,0.0);
}
@compute @workgroup_size(8,8)
fn bloom_wide_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(1,0),p.values.x);
}
@compute @workgroup_size(8,8)
fn bloom_wide_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(0,1),p.values.x);
}
@compute @workgroup_size(8,8)
fn bloom_tight_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(1,0),p.values.x*0.2);
}
@compute @workgroup_size(8,8)
fn bloom_tight_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(0,1),p.values.x*0.2);
}
@compute @workgroup_size(8,8)
fn bloom_combine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let c=source[i];
    let glow=(0.55*second[i].rgb+0.45*third[i].rgb+p.extra.w*fourth[i].rgb)*p.extra.rgb*p.values.y;
    result[i]=vec4<f32>(c.rgb+glow,c.a);
}
