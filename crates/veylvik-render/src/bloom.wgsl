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

// An Own size picture placed in the frame: source = the picture, values = (its width,
// its height, fit: 0 actual, 1 fit, 2 fill, 3 stretch). `scale` is frame pixels for
// each picture pixel; scaling down averages the picture pixels an output pixel covers
// (up to 8 by 8 samples).
fn own_texel(x:i32,y:i32)->vec4<f32> {
    let w=i32(p.values.x); let h=i32(p.values.y);
    return source[u32(clamp(y,0,h-1)*w+clamp(x,0,w-1))];
}
fn own_sample(q:vec2<f32>)->vec4<f32> {
    let b=q-vec2<f32>(0.5); let i=vec2<i32>(floor(b)); let f=fract(b);
    return mix(mix(own_texel(i.x,i.y),own_texel(i.x+1,i.y),f.x),
               mix(own_texel(i.x,i.y+1),own_texel(i.x+1,i.y+1),f.x),f.y);
}
fn own_placed(gid:vec3<u32>,scale:vec2<f32>)->vec4<f32> {
    let frame=vec2<f32>(p.size_op.xy);
    let q=(vec2<f32>(gid.xy)+vec2<f32>(0.5)-frame*0.5)/scale+p.values.xy*0.5;
    let span=1.0/scale;
    if any(q+span*0.5<=vec2<f32>(0.0)) || any(q-span*0.5>=p.values.xy) {return vec4<f32>(0.0);}
    let n=vec2<i32>(clamp(ceil(span),vec2<f32>(1.0),vec2<f32>(8.0)));
    var sum=vec4<f32>(0.0);
    for(var j=0;j<n.y;j++) { for(var i=0;i<n.x;i++) {
        let at=q+(vec2<f32>(f32(i),f32(j))+vec2<f32>(0.5))/vec2<f32>(n)*span-span*0.5;
        // Beyond the picture is empty, so its edge stays an edge.
        if all(at>=vec2<f32>(0.0)) && all(at<=p.values.xy) { sum+=own_sample(at); }
    } }
    return sum/f32(n.x*n.y);
}
@compute @workgroup_size(8,8)
fn own_size_fit(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let ratio=vec2<f32>(p.size_op.xy)/p.values.xy;
    var scale=vec2<f32>(1.0);
    if p.values.z>2.5 { scale=ratio; }
    else if p.values.z>1.5 { scale=vec2<f32>(max(ratio.x,ratio.y)); }
    else if p.values.z>0.5 { scale=vec2<f32>(min(ratio.x,ratio.y)); }
    result[gid.y*p.size_op.x+gid.x]=own_placed(gid,scale);
}
@compute @workgroup_size(8,8)
fn own_size_stretch(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[gid.y*p.size_op.x+gid.x]=own_placed(gid,vec2<f32>(p.size_op.xy)/p.values.xy);
}
