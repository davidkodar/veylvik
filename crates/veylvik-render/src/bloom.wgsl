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
// The streak is the sideways blur given a little height (a twentieth of the radius):
// a lens streak is a soft band, and a blur that is one row tall draws hard lines
// along bright horizontal edges and stripes where rows differ.
@compute @workgroup_size(8,8)
fn bloom_streak_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(0,1),p.values.x*0.05);
}
@compute @workgroup_size(8,8)
fn bloom_combine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let c=source[i];
    let glow=(0.55*second[i].rgb+0.45*third[i].rgb+p.extra.w*fourth[i].rgb)*p.extra.rgb*p.values.y;
    result[i]=vec4<f32>(c.rgb+glow,c.a);
}
// Light wrap: source = the foreground, second = the background blurred by the radius,
// third = the foreground blurred by the radius. values = (radius, amount). The
// background's light is added just inside the foreground's edges, where its blurred
// alpha has fallen below its own; alpha and everything outside it stay.
@compute @workgroup_size(8,8)
fn light_wrap_combine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let c=source[i];
    let edge=clamp(c.a-third[i].a,0.0,1.0)*2.0;
    let light=max(second[i].rgb,vec3<f32>(0.0));
    result[i]=vec4<f32>(c.rgb+light*min(edge,c.a)*p.values.y,c.a);
}
// Zoom blur: values = (amount, x, y). Each pixel is the average of 32 samples along
// the line towards the point, over `amount` of its distance from it.
@compute @workgroup_size(8,8)
fn zoom_blur(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let at=vec2<f32>(gid.xy)+vec2<f32>(0.5);
    let away=at-p.values.yz;
    var sum=vec4<f32>(0.0);
    for(var k=0;k<32;k++) {
        let along=1.0-clamp(p.values.x,0.0,1.0)*(f32(k)+0.5)/32.0;
        sum+=bilinear(p.values.yz+away*along-vec2<f32>(0.5));
    }
    result[index(gid)]=sum/32.0;
}
