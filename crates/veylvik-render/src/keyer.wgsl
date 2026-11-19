// The chroma key's finishing passes, used when its matte is shaped (clip, keep darks,
// shrink, soften), masked or shown. values = key RGB, tolerance; extra = softness,
// despill, clip black, clip white; size_op.z = keep darks (float bits); size_op.w =
// shrink radius (bits 0-5), grow flag (6), soften radius (7-12), show matte (13),
// keep mask connected (14), garbage mask connected (15).
fn key_bits()->u32 { return p.size_op.w; }
// The screen matte alone, in every channel: the key's own answer, then pixels too dark
// to carry a reliable colour pushed back to opaque, then clipped.
@compute @workgroup_size(8,8)
fn chroma_matte(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i];
    if c.a<=0.0 {result[i]=vec4<f32>(0.0);return;}
    let positive=max(c.rgb/c.a,vec3<f32>(0.0));
    let key=max(p.values.xyz,vec3<f32>(0.0));
    let k=dominant(key);
    let key_lean=max(lean(key,k),1e-6);
    var alpha=1.0;
    if dot(positive,vec3<f32>(0.2126,0.7152,0.0722))>0.002 {
        let ratio=lean(positive,k)/key_lean;
        let edge=1.0-p.values.w;
        if p.extra.x>0.0 {alpha=clamp((edge-ratio)/p.extra.x,0.0,1.0);}
        else {alpha=select(1.0,0.0,ratio>=edge);}
    }
    let darks=bitcast<f32>(p.size_op.z);
    if darks>0.0 {
        // A lit screen is bright in its own channel; hair and shadow detail far darker
        // than the screen is subject, however its little colour leans.
        let lit=positive[k]/max(key[k],1e-6);
        alpha=mix(1.0,alpha,smoothstep(darks*0.5,darks*1.5,lit));
    }
    alpha=clamp((alpha-p.extra.z)/max(p.extra.w-p.extra.z,1e-4),0.0,1.0);
    result[i]=vec4<f32>(alpha);
}
fn matte_at(x:i32,y:i32)->f32 {
    let cx=clamp(x,0,i32(p.size_op.x)-1); let cy=clamp(y,0,i32(p.size_op.y)-1);
    return source[u32(cy)*p.size_op.x+u32(cx)].a;
}
fn key_shrink(gid:vec3<u32>,dir:vec2<i32>) {
    if !inside(gid) {return;}
    let r=i32(key_bits()&63u); let grow=(key_bits()&64u)!=0u;
    let at=vec2<i32>(gid.xy);
    var m=matte_at(at.x,at.y);
    for(var k=-32;k<=32;k++) {
        if abs(k)>r {continue;}
        let v=matte_at(at.x+dir.x*k,at.y+dir.y*k);
        if grow {m=max(m,v);} else {m=min(m,v);}
    }
    result[index(gid)]=vec4<f32>(m);
}
@compute @workgroup_size(8,8)
fn key_shrink_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) { key_shrink(gid,vec2<i32>(1,0)); }
@compute @workgroup_size(8,8)
fn key_shrink_vertical(@builtin(global_invocation_id) gid:vec3<u32>) { key_shrink(gid,vec2<i32>(0,1)); }
fn key_soften(gid:vec3<u32>,dir:vec2<i32>) {
    if !inside(gid) {return;}
    let r=i32((key_bits()>>7u)&63u);
    let at=vec2<i32>(gid.xy);
    let sigma=max(f32(r)*0.5,0.5);
    var sum=0.0; var weight=0.0;
    for(var k=-32;k<=32;k++) {
        if abs(k)>r {continue;}
        let w=exp(-f32(k*k)/(2.0*sigma*sigma));
        sum+=matte_at(at.x+dir.x*k,at.y+dir.y*k)*w; weight+=w;
    }
    result[index(gid)]=vec4<f32>(sum/weight);
}
@compute @workgroup_size(8,8)
fn key_soften_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) { key_soften(gid,vec2<i32>(1,0)); }
@compute @workgroup_size(8,8)
fn key_soften_vertical(@builtin(global_invocation_id) gid:vec3<u32>) { key_soften(gid,vec2<i32>(0,1)); }
// The keyed picture: source = the image, second = its finished matte, third = the keep
// mask (its alpha is always subject), fourth = the garbage mask (its alpha is never).
@compute @workgroup_size(8,8)
fn chroma_apply(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i];
    var m=second[i].a;
    if (key_bits()&(1u<<14u))!=0u {m=max(m,clamp(third[i].a,0.0,1.0));}
    if (key_bits()&(1u<<15u))!=0u {m*=1.0-clamp(fourth[i].a,0.0,1.0);}
    let a=m*c.a;
    if (key_bits()&(1u<<13u))!=0u {result[i]=vec4<f32>(a,a,a,1.0);return;}
    if c.a<=0.0 {result[i]=vec4<f32>(0.0);return;}
    var rgb=c.rgb/c.a;
    let k=dominant(max(p.values.xyz,vec3<f32>(0.0)));
    if p.extra.y>0.0 {
        var others=vec2<f32>(rgb.g,rgb.b);
        if k==1u {others=vec2<f32>(rgb.r,rgb.b);}
        if k==2u {others=vec2<f32>(rgb.r,rgb.g);}
        let limit=0.5*(others.x+others.y);
        if rgb[k]>limit {rgb[k]=mix(rgb[k],limit,p.extra.y);}
    }
    result[i]=vec4<f32>(rgb*a,a);
}
