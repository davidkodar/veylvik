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

// Water uses the same filtered environment and area-light integrals as materials.
// Its dielectric F0 remains water's, even when the substrate is a conductor.
fn water_reflection(pixel:vec2<f32>,normal:vec3<f32>,rough:f32)->vec3<f32> {
    let position=vec3<f32>((pixel-vec2<f32>(p.size_op.xy)*0.5)/(f32(p.size_op.y)*0.5),0.0);
    let view=normalize(vec3<f32>(0.0,0.0,6.0)-position);
    let nv=max(dot(normal,view),0.0001);
    var tangent=view-normal*dot(normal,view);
    if dot(tangent,tangent)<0.000001 {tangent=cross(normal,vec3<f32>(0.0,1.0,0.0));}
    tangent=normalize(tangent);
    let frame=transpose(mat3x3<f32>(tangent,cross(normal,tangent),normal));
    let m=ltc_lookup(rough,nv,0u);let amplitude=ltc_lookup(rough,nv,4096u);
    let inverse=mat3x3<f32>(vec3<f32>(m.x,0.0,m.y),vec3<f32>(0.0,1.0,0.0),vec3<f32>(m.z,0.0,m.w));
    let c=cos(studio.environment.w);let s=sin(studio.environment.w);
    let rotation=mat3x3<f32>(vec3<f32>(c,0.0,s),vec3<f32>(0.0,1.0,0.0),vec3<f32>(-s,0.0,c));
    var radiance=vec3<f32>(studio.environment.x);
    radiance+=environment_specular(rotation*reflect(-view,normal),rough)*studio.environment.z;
    for(var i=0u;i<min(u32(studio.environment.y),8u);i++) {
        let light=studio.lights[i];
        if light.center.w>0.0 {radiance+=light.colour.rgb*light.center.w*ltc_rect(position,inverse*frame,light.center.xyz,light.u.xyz,light.v.xyz);}
    }
    return (studio.optics.rgb*amplitude.x+(vec3<f32>(1.0)-studio.optics.rgb)*amplitude.y)*radiance;
}
@compute @workgroup_size(8,8)
fn condensation(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let original=source[i];
    if original.a==0.0 || p.values.x==0.0 {result[i]=original;return;}
    let point=vec2<f32>(gid.xy);
    var coverage=0.0;var slope=vec2<f32>(0.0);
    // Two irregular bead populations: fine condensation and sparse larger drops.
    for(var layer=0u;layer<2u;layer++) {
        let scale=select(0.45,1.6,layer==1u);
        let size=max(0.6,p.values.y*scale);
        let cell_size=size*4.8;
        let cell=vec2<i32>(floor(point/cell_size));
        for(var dy=-1;dy<=1;dy++) {for(var dx=-1;dx<=1;dx++) {
            let cell_id=cell+vec2<i32>(dx,dy);
            let seed=bitcast<u32>(cell_id.x)*1973u+bitcast<u32>(cell_id.y)*9277u+layer*26699u+p.size_op.w;
            if particle_random(seed+71u)>p.values.z {continue;}
            let jitter=vec2<f32>(particle_random(seed),particle_random(seed+17u));
            var center=(vec2<f32>(cell_id)+vec2<f32>(0.15)+jitter*0.7)*cell_size;
            let sliding=layer==1u && particle_random(seed+97u)<0.3;
            let phase=fract(fract(p.extra.x/4.0)+particle_random(seed+101u));
            var visibility=1.0;
            if sliding {
                center.y+=(phase-0.5)*cell_size*p.extra.y;
                center.x+=sin(phase*6.283+jitter.x*6.283)*size*0.4*p.extra.y;
                visibility=mix(1.0,smoothstep(0.0,0.12,phase)*(1.0-smoothstep(0.88,1.0,phase)),p.extra.y);
            }
            let radius=size*(0.45+0.55*particle_random(seed+31u));
            let q=(point-center)/vec2<f32>(radius,radius*1.14);
            let distance=length(q);
            let bead=(1.0-smoothstep(1.0-0.65/radius,1.0+0.65/radius,distance))*visibility;
            if bead>coverage {coverage=bead;slope=q*0.8/sqrt(max(0.03,1.0-dot(q,q)));}
            if sliding && p.extra.y>0.0 {
                let trail_length=size*5.0*p.extra.y;
                let t=clamp((center.y-point.y)/max(trail_length,0.01),0.0,1.0);
                let width=radius*0.24*(1.0-t);
                let trail=(1.0-smoothstep(width,width+0.8,abs(point.x-center.x)))*smoothstep(0.0,1.0,center.y-point.y)*(1.0-t)*visibility*p.extra.y*0.6;
                if trail>coverage {coverage=trail;slope=vec2<f32>((point.x-center.x)/max(width,0.1),0.0);}
            }
        }}
    }
    let moisture=fire_fbm(vec3<f32>(point/max(p.values.y*24.0,1.0),0.3),p.size_op.w);
    let fog=p.values.w*smoothstep(0.3,0.7,moisture)*(1.0-coverage);
    let coat=clamp((coverage+fog)*p.values.x,0.0,1.0);
    if coat<0.0001 {result[i]=original;return;}
    let normal=normalize(vec3<f32>(slope,1.0));
    let displacement=-normal.xy*p.values.y*0.55;
    let refracted=bilinear(point+displacement);
    let substrate=original.rgb/original.a;
    var beneath=substrate;
    if refracted.a>0.001 {beneath=refracted.rgb/refracted.a;}
    if p.extra.z>0.0 && coverage>0.0 {
        let red=bilinear(point+displacement*(1.0+p.extra.z));
        let blue=bilinear(point+displacement*(1.0-p.extra.z));
        if red.a>0.001 {beneath.r=red.r/red.a;}
        if blue.a>0.001 {beneath.b=blue.b/blue.a;}
    }
    // Fine moisture scatters existing surface detail, rather than painting white fog.
    let radius=max(1.0,p.values.y*0.7);
    let blurred=(bilinear(point+vec2<f32>(radius,0.0))+bilinear(point-vec2<f32>(radius,0.0))+bilinear(point+vec2<f32>(0.0,radius))+bilinear(point-vec2<f32>(0.0,radius)))*0.25;
    if blurred.a>0.001 {beneath=mix(beneath,blurred.rgb/blurred.a,fog);}
    let fresnel=0.0204+0.9796*pow(1.0-normal.z,5.0);
    let reflection=water_reflection(point,normal,mix(0.07,0.32,fog));
    let colour=mix(substrate,beneath*(1.0-fresnel)+reflection,coat);
    result[i]=vec4<f32>(colour*original.a,original.a);
}
