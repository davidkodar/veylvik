// Stateless effects: scrubbing and out-of-order renders produce identical frames.
fn particle_random(v:u32)->f32 {return f32(noise_hash(v)&0x00ffffffu)/16777216.0;}
@compute @workgroup_size(8,8)
fn dust(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let scale=f32(p.size_op.y)/540.0;
    let pixel=vec2<f32>(gid.xy)/scale;
    let phase=fract(p.values.x/4.0)*6.283185307;
    var coverage=0.0;
    // Separate focal planes, with a restrained, non-emissive scattering response.
    for(var layer=0u;layer<3u;layer++) {
        let depth=f32(layer);
        let drift=vec2<f32>(sin(phase+depth)*18.0,cos(phase+depth*1.7)*8.0)*p.values.w;
        let point=pixel-drift;
        let cell_size=vec2<f32>(66.0+depth*24.0);
        let cell=vec2<i32>(floor(point/cell_size));
        for(var dy=-1;dy<=1;dy++) {for(var dx=-1;dx<=1;dx++) {
            let c=cell+vec2<i32>(dx,dy);
            let seed=bitcast<u32>(c.x)*1973u+bitcast<u32>(c.y)*9277u+layer*26699u+p.size_op.w;
            if particle_random(seed+71u)>p.values.y {continue;}
            let jitter=vec2<f32>(particle_random(seed),particle_random(seed+17u));
            let flutter=vec2<f32>(sin(phase+jitter.x*6.283),cos(phase+jitter.y*6.283))*2.5;
            let center=(vec2<f32>(c)+jitter)*cell_size+flutter*p.values.w;
            let radius=p.values.z/scale*(0.35+particle_random(seed+31u)*0.65);
            let defocus=select(0.35,1.8+depth,layer>0u);
            let sigma=max(0.35,sqrt(radius*radius+defocus*defocus));
            let d=(point-center)/sigma;
            // Depth of field conserves particle energy: blurred motes become dimmer.
            let beam=0.2+0.8*exp(-pow((center.x-center.y*0.65-240.0)/260.0,2.0));
            let alpha=exp(-dot(d,d)*0.5)*radius*radius/(sigma*sigma)*0.48*beam;
            coverage=1.0-(1.0-coverage)*(1.0-alpha);
        }}
    }
    result[index(gid)]=vec4<f32>(vec3<f32>(0.64,0.57,0.45)*coverage,coverage);
}

// One scan per column, not one scan per output pixel. Root coordinates remain
// in composition pixels and are derived from coverage, never from the text string.
@compute @workgroup_size(8,8)
fn drip_roots(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=vec4<f32>(0.0);
    if gid.y!=0u {return;}
    var root=-1.0;
    for(var y=0u;y<p.size_op.y;y++) {
        if source[y*p.size_op.x+gid.x].a>0.5 {root=f32(y);}
    }
    result[gid.x]=vec4<f32>(root,0.0,0.0,1.0);
}
fn drip_ellipse(point:vec2<f32>,center:vec2<f32>,r:vec2<f32>)->f32 {
    return (length((point-center)/r)-1.0)*min(r.x,r.y);
}
@compute @workgroup_size(8,8)
fn drip(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let point=vec2<f32>(gid.xy);
    let spacing=p.values.y;
    let reach=p.values.z;
    let radius=min(p.values.w,spacing*0.22);
    var coverage=source[index(gid)].a;
    if reach>0.0 {
        let cell=i32(floor(point.x/spacing));
        for(var offset=-1;offset<=1;offset++) {
            let id=cell+offset;
            let seed=bitcast<u32>(id)*7919u+p.size_op.w;
            let x=(f32(id)+0.2+particle_random(seed)*0.6)*spacing;
            if x<0.0 || x>=f32(p.size_op.x) {continue;}
            let root=second[u32(x)].x;
            if root<0.0 {continue;}
            let u=fract(fract(p.values.x/4.0)+particle_random(seed+31u));
            let r=radius*(0.65+0.35*particle_random(seed+7u));
            let pendant=min(reach*0.32,r*6.5);
            let growth=smoothstep(0.0,0.66,u);
            let retract=smoothstep(0.67,0.84,u);
            let tip=root+mix(r,pendant,growth*(1.0-retract));
            let h=max(tip-root,0.1);
            let t=clamp((point.y-root)/h,0.0,1.0);
            let neck=r*mix(0.7,0.24,t)*(1.0-smoothstep(0.54,0.69,u)*(1.0-retract)*0.78);
            let segment=length(vec2<f32>(point.x-x,max(max(root-point.y,point.y-tip),0.0)))-neck;
            let bulb=drip_ellipse(point,vec2<f32>(x,tip),vec2<f32>(r*(0.5+0.5*growth*(1.0-retract)),r*1.25));
            var distance=min(segment,bulb);
            // Detachment follows neck thinning; the drop then accelerates under gravity.
            if u>0.67 {
                let fall=(u-0.67)/0.33;
                let y=root+pendant+reach*fall*fall;
                let drop=drip_ellipse(point,vec2<f32>(x,y),vec2<f32>(r,r*(1.25+fall*0.7)));
                let visibility=1.0-smoothstep(0.88,1.0,fall);
                coverage=max(coverage,(1.0-smoothstep(-0.6,0.6,drop))*visibility);
            }
            coverage=max(coverage,1.0-smoothstep(-0.6,0.6,distance));
        }
    }
    result[index(gid)]=vec4<f32>(coverage);
}
