// Match grain: unit gaussian noise per pixel in four channels (red, green, blue and a
// part they share), blurred to the grain size by the blur passes, then added scaled
// by each pixel's brightness. p.size_op.z seeds the frame; values = (size, gaussian,
// colour, red); extra = (shadows, midtones, highlights, blue), amount applied.
fn grain_hash(v:vec3<u32>)->u32 {
    var x=(v.x*1664525u)^(v.y*1013904223u)^(v.z*2654435769u);
    x=x^(x>>16u);x=x*0x7feb352du;x=x^(x>>15u);x=x*0x846ca68bu;x=x^(x>>16u);
    return x;
}
fn grain_unit(v:vec3<u32>)->f32 {return (f32(grain_hash(v)>>8u)+0.5)/16777216.0;}
@compute @workgroup_size(8,8)
fn grain_noise(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let seed=p.size_op.z*8u;
    var n=vec4<f32>(0.0);
    for(var k=0u;k<2u;k++) {
        let u1=grain_unit(vec3<u32>(gid.x,gid.y,seed+2u*k));
        let u2=grain_unit(vec3<u32>(gid.x,gid.y,seed+2u*k+1u));
        let r=sqrt(-2.0*log(u1));
        n[2u*k]=r*cos(6.28318531*u2);
        n[2u*k+1u]=r*sin(6.28318531*u2);
    }
    result[index(gid)]=n;
}
// Grain strength at luminance l: linear in log2 luminance between 0.02, 0.18 and 0.9.
fn grain_sigma(l:f32)->f32 {
    let x=log2(max(l,1e-4));let a=log2(0.02);let b=log2(0.18);let c=log2(0.9);
    if x<=a {return p.extra.x;}
    if x<=b {return mix(p.extra.x,p.extra.y,(x-a)/(b-a));}
    if x<=c {return mix(p.extra.y,p.extra.z,(x-b)/(c-b));}
    return p.extra.z;
}
@compute @workgroup_size(8,8)
fn grain_apply(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let c=source[i];
    if c.a<=0.0 {result[i]=c;return;}
    // The blurred noise's spread is Σw² of the normalised separable weights; undo it.
    let r=i32(round(p.values.x));var s=0.0;var s2=0.0;
    for(var o=-r;o<=r;o++) {let w=blur_weight(o,r,true);s+=w;s2+=w*w;}
    let n=second[i]*(s*s/s2);
    let mixed=clamp(p.values.z,0.0,1.0);
    let grain=sqrt(mixed)*n.w+sqrt(1.0-mixed)*n.xyz;
    let rgb=c.rgb/c.a;
    let l=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
    let out=rgb+grain_sigma(l)*vec3<f32>(p.values.w,1.0,p.extra.w)*grain;
    result[i]=vec4<f32>(out*c.a,c.a);
}
