// Matte refinement (appended to kernels.wgsl). values: edge radius, fusion radius, colour
// separation, decontaminate. Colour projection between local subject and background colours
// snaps the matte to image edges, then blur-fusion (Forte & Pitié) estimates foreground colour. Boxes clamp at the frame border, so
// subjects cut by the frame keep exact statistics.
fn clamped(x:i32,y:i32)->vec4<f32> {
    let q=clamp(vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(p.size_op.xy)-1);
    return source[u32(q.y)*p.size_op.x+u32(q.x)];
}
fn box_axis(gid:vec3<u32>,d:vec2<i32>,radius:i32)->vec4<f32> {
    var total=vec4<f32>(0.0);
    for(var t=-radius;t<=radius;t++) {total+=clamped(i32(gid.x)+d.x*t,i32(gid.y)+d.y*t);}
    return total/f32(2*radius+1);
}
fn straight(c:vec4<f32>)->vec3<f32> {
    if c.a<=1e-6 {return vec3<f32>(0.0);}
    return c.rgb/c.a;
}
// Perceptual colour for estimation: sqrt-encoded straight colour, clamped to 0..1.
fn encoded(c:vec4<f32>)->vec3<f32> {
    return sqrt(clamp(straight(c),vec3<f32>(0.0),vec3<f32>(1.0)));
}
// [encoded colour, coarse matte]; source: image, second: matte.
@compute @workgroup_size(8,8)
fn matte_pack(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); result[i]=vec4<f32>(encoded(source[i]),clamp(second[i].a,0.0,1.0));
}
// Colour of pixels the matte is sure about, weighted for averaging: [colour·w, w].
@compute @workgroup_size(8,8)
fn known_subject(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i]; let w=smoothstep(0.9,0.99,c.a);
    result[i]=vec4<f32>(c.rgb*w,w);
}
@compute @workgroup_size(8,8)
fn known_background(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i]; let w=1.0-smoothstep(0.01,0.1,c.a);
    result[i]=vec4<f32>(c.rgb*w,w);
}
@compute @workgroup_size(8,8)
fn matte_box_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=box_axis(gid,vec2<i32>(1,0),i32(p.values.x));
}
@compute @workgroup_size(8,8)
fn matte_box_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=box_axis(gid,vec2<i32>(0,1),i32(p.values.x));
}
// Alpha from where a pixel's colour lies between the nearby subject colour F and background
// colour B: dot(I − B, F − B) / |F − B|². Used only near the matte's edge, where both are
// present, and only as far as F and B differ by more than the separation in values.z; the
// coarse matte stands elsewhere. source: subject average, second: background average,
// third: [encoded colour, coarse matte].
@compute @workgroup_size(8,8)
fn matte_project(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let fg=source[i]; let bg=second[i]; let c=third[i];
    var alpha=c.a;
    if fg.a>1e-3 && bg.a>1e-3 {
        let f=fg.rgb/fg.a; let b=bg.rgb/bg.a; let d=f-b;
        let projected=clamp(dot(c.rgb-b,d)/max(dot(d,d),1e-8),0.0,1.0);
        let distinct=smoothstep(0.4*p.values.z,p.values.z,length(d));
        // Trust the estimate only when the pixel's colour lies near the line from B to F. A
        // patterned subject (stripes, print) averages to a colour none of its pixels have.
        let residual=length(c.rgb-(b+projected*d));
        let fits=1.0-smoothstep(0.25*length(d),0.6*length(d),residual);
        // Where the model was unsure, re-estimate fully. Where it was sure, only within about
        // three pixels of the edge (both sides well represented in the window), so lone
        // background grains that match the subject stay out.
        let unsure=smoothstep(0.0,0.1,c.a)*smoothstep(0.0,0.1,1.0-c.a);
        let band=smoothstep(0.05,0.3,fg.a)*smoothstep(0.05,0.3,bg.a);
        let available=smoothstep(0.0,0.05,fg.a)*smoothstep(0.0,0.05,bg.a);
        alpha=mix(c.a,projected,available*max(unsure,band)*distinct*fits);
    }
    // Clip faint responses to clean black and white, as a keyer would, and never weaken what
    // the model was sure of.
    alpha=clamp((alpha-0.04)/0.92,0.0,1.0);
    alpha=mix(alpha,max(alpha,c.a),smoothstep(0.9,0.99,c.a));
    result[i]=vec4<f32>(alpha);
}
// [straight image colour, refined alpha × image alpha]; source: refined alpha; second: image.
@compute @workgroup_size(8,8)
fn matte_join(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=second[i];
    result[i]=vec4<f32>(straight(c),source[i].x*clamp(c.a,0.0,1.0));
}
@compute @workgroup_size(8,8)
fn fuse_foreground(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} let i=index(gid); let c=source[i];
    // Weighted towards what is surely subject: a half-transparent rim pixel still holds
    // the old background's colour and would carry it into the estimate.
    let w=c.a*c.a*c.a;
    result[i]=vec4<f32>(c.rgb*w,w);
}
@compute @workgroup_size(8,8)
fn fuse_background(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} let i=index(gid); let c=source[i];
    let w=(1.0-c.a)*(1.0-c.a)*(1.0-c.a);
    result[i]=vec4<f32>(c.rgb*w,w);
}
@compute @workgroup_size(8,8)
fn fuse_box_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=box_axis(gid,vec2<i32>(1,0),i32(p.values.y));
}
@compute @workgroup_size(8,8)
fn fuse_box_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=box_axis(gid,vec2<i32>(0,1),i32(p.values.y));
}
// How far a pixel's own colour is trusted as subject detail: fully where it is mostly
// opaque, not at all where it is mostly transparent, and fully when no surely-subject
// colour is near to stand in for it.
fn fuse_trust(a:f32,known:f32)->f32 {
    return mix(1.0,smoothstep(0.3,0.9,a),smoothstep(0.0,1e-3,known));
}
// F = F̂ + α(I − αF̂ − (1−α)B̂), split over two passes: here F̂(1−α²) + αI, blended with I by
// the decontaminate amount; fuse_finish subtracts the background term.
@compute @workgroup_size(8,8)
fn fuse_estimate(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=second[i]; let a=c.a; let blurred=source[i];
    var estimate=c.rgb;
    if blurred.a>1e-4 {estimate=blurred.rgb/blurred.a;}
    // The fused colour keeps the pixel's own detail, which is right where the subject
    // dominates. A mostly transparent pixel shows mostly the old background, so its
    // detail is background too: there the surrounding subject colour stands instead.
    let trust=fuse_trust(a,blurred.a);
    let partial=mix(estimate,estimate*(1.0-a*a)+a*c.rgb,trust);
    result[i]=vec4<f32>(mix(c.rgb,partial,p.values.w),a);
}
@compute @workgroup_size(8,8)
fn fuse_finish(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let t=source[i]; let a=t.a; let blurred=second[i];
    var background=vec3<f32>(0.0);
    if blurred.a>1e-4 {background=blurred.rgb/blurred.a;}
    // Interior pixels keep the image exactly; HDR values are preserved, only negatives from
    // the estimate are clipped at the edge.
    // third: the surely-subject average, for the same trust as the estimate used.
    var colour=t.rgb-p.values.w*fuse_trust(a,third[i].a)*a*(1.0-a)*background;
    if a<1.0 {colour=max(colour,vec3<f32>(0.0));}
    result[i]=vec4<f32>(colour*a,a);
}
