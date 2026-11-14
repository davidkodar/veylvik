struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> second: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> third: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> ltc: array<vec4<f32>>;
struct StudioLight { center:vec4<f32>, u:vec4<f32>, v:vec4<f32>, colour:vec4<f32> }
struct StudioLighting { optics:vec4<f32>, environment:vec4<f32>, lights:array<StudioLight,8> }
@group(0) @binding(6) var<uniform> studio:StudioLighting;
fn inside(gid:vec3<u32>)->bool { return gid.x<p.size_op.x && gid.y<p.size_op.y; }
fn index(gid:vec3<u32>)->u32 { return gid.y*p.size_op.x+gid.x; }
fn sample_source(x:i32,y:i32)->vec4<f32> {
    if x<0 || y<0 || x>=i32(p.size_op.x) || y>=i32(p.size_op.y) {return vec4<f32>(0.0);}
    return source[u32(y)*p.size_op.x+u32(x)];
}
fn noise_hash(value:u32)->u32 {
    var v=value;v=(v^(v>>16u))*0x7feb352du;v=(v^(v>>15u))*0x846ca68bu;return v^(v>>16u);
}
@compute @workgroup_size(8,8)
fn solid(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=vec4<f32>(p.values.rgb*p.values.a,p.values.a);
}
@compute @workgroup_size(8,8)
fn noise(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let x=u32(f32(gid.x)/p.values.x);let y=u32(f32(gid.y)/p.values.x);
    let n=f32(noise_hash(x+y*65537u+p.size_op.w)&65535u)/65535.0;
    result[index(gid)]=vec4<f32>(vec3<f32>(0.1,0.5,1.0)*n,n);
}
@compute @workgroup_size(8,8)
fn exposure(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);result[i]=vec4<f32>(source[i].rgb*p.values.x,source[i].a);
}
// Colour-difference chroma key on unpremultiplied colour. values: key RGB, tolerance;
// extra: softness, despill. The key's dominant channel defines the backdrop direction.
fn dominant(c:vec3<f32>)->u32 {
    if c.g>=c.r && c.g>=c.b {return 1u;}
    if c.b>=c.r {return 2u;}
    return 0u;
}
fn lean(c:vec3<f32>,k:u32)->f32 {
    // Dominant channel minus the larger other channel, relative to the dominant channel.
    var others=vec2<f32>(c.g,c.b);
    if k==1u {others=vec2<f32>(c.r,c.b);}
    if k==2u {others=vec2<f32>(c.r,c.g);}
    let top=max(c[k],1e-6);
    return (c[k]-max(others.x,others.y))/top;
}
@compute @workgroup_size(8,8)
fn chroma_key(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i];
    if c.a<=0.0 {result[i]=c;return;}
    var rgb=c.rgb/c.a;
    let key=max(p.values.xyz,vec3<f32>(0.0)); let tolerance=p.values.w;
    let softness=p.extra.x; let despill=p.extra.y;
    let k=dominant(key);
    let key_lean=max(lean(key,k),1e-6);
    var alpha=1.0;
    let positive=max(rgb,vec3<f32>(0.0));
    // Near-black pixels carry no reliable colour; keep them.
    if dot(positive,vec3<f32>(0.2126,0.7152,0.0722))>0.002 {
        let ratio=lean(positive,k)/key_lean;
        let edge=1.0-tolerance;
        if softness>0.0 {alpha=clamp((edge-ratio)/softness,0.0,1.0);}
        else {alpha=select(1.0,0.0,ratio>=edge);}
    }
    if despill>0.0 {
        var others=vec2<f32>(rgb.g,rgb.b);
        if k==1u {others=vec2<f32>(rgb.r,rgb.b);}
        if k==2u {others=vec2<f32>(rgb.r,rgb.g);}
        let limit=0.5*(others.x+others.y);
        if rgb[k]>limit {rgb[k]=mix(rgb[k],limit,despill);}
    }
    let a=alpha*c.a;
    result[i]=vec4<f32>(rgb*a,a);
}
// Luma key: transparent at or below low, opaque at or above high (reversed when low > high).
@compute @workgroup_size(8,8)
fn luma_key(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i];
    if c.a<=0.0 {result[i]=c;return;}
    let l=dot(c.rgb/c.a,vec3<f32>(0.2126,0.7152,0.0722));
    let low=p.values.x; let high=p.values.y;
    var alpha=select(0.0,1.0,l>=high);
    if high!=low {alpha=clamp((l-low)/(high-low),0.0,1.0);}
    result[i]=c*alpha;
}
// Fade: premultiplied colour and alpha scale together by the visibility in values.x.
@compute @workgroup_size(8,8)
fn fade(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); result[i]=source[i]*p.values.x;
}
// Primary grade on unpremultiplied colour; each stage at its default is skipped so the
// identity is exact. values: temperature, tint, lift, gain; extra: contrast, gamma, saturation.
fn signed_pow(v:vec3<f32>,e:f32)->vec3<f32> {return sign(v)*pow(abs(v),vec3<f32>(e));}
@compute @workgroup_size(8,8)
fn color_correct(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i];
    if c.a<=0.0 {result[i]=c;return;}
    var rgb=c.rgb/c.a;
    let temperature=p.values.x; let tint=p.values.y; let lift=p.values.z; let gain=p.values.w;
    let contrast=p.extra.x; let gamma=p.extra.y; let saturation=p.extra.z;
    if temperature!=0.0 || tint!=0.0 {
        rgb*=vec3<f32>(exp2(0.5*temperature),exp2(-0.5*tint),exp2(-0.5*temperature));
    }
    if gain!=1.0 || lift!=0.0 {rgb=rgb*gain+vec3<f32>(lift);}
    if contrast!=1.0 {rgb=0.18*signed_pow(rgb/0.18,contrast);}
    if gamma!=1.0 {rgb=signed_pow(rgb,1.0/gamma);}
    if saturation!=1.0 {
        let y=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
        rgb=vec3<f32>(y)+(rgb-vec3<f32>(y))*saturation;
    }
    result[i]=vec4<f32>(rgb*c.a,c.a);
}
fn bilinear(pos:vec2<f32>)->vec4<f32> {
    // Reject far-away coordinates before float-to-int conversion or adding one.
    if any(pos < vec2<f32>(-1.0)) || any(pos > vec2<f32>(p.size_op.xy)) {return vec4<f32>(0.0);}
    let base=vec2<i32>(floor(pos));let f=fract(pos);
    let a=mix(sample_source(base.x,base.y),sample_source(base.x+1,base.y),f.x);
    let b=mix(sample_source(base.x,base.y+1),sample_source(base.x+1,base.y+1),f.x);
    return mix(a,b,f.y);
}
@compute @workgroup_size(8,8)
fn transform(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=bilinear(vec2<f32>(gid.xy)-p.values.xy);
}
@compute @workgroup_size(8,8)
fn affine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var color=vec4<f32>(0.0);
    if p.extra.z>0.0 {
        let pos=vec2<f32>(gid.xy);
        color=bilinear(vec2<f32>(dot(p.values.xy,pos),dot(p.values.zw,pos))+p.extra.xy);
    }
    result[index(gid)]=color;
}
@compute @workgroup_size(8,8)
fn ellipse(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var alpha=0.0;
    if p.values.z>0.0 && p.values.w>0.0 {
        let distance=(length((vec2<f32>(gid.xy)-p.values.xy)/p.values.zw)-1.0)*min(p.values.z,p.values.w);
        alpha=1.0-smoothstep(-p.extra.x*0.5,p.extra.x*0.5,distance);
    }
    result[index(gid)]=vec4<f32>(alpha);
}
@compute @workgroup_size(8,8)
fn mask(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);result[i]=source[i]*clamp(second[i].a,0.0,1.0);
}
fn blur_axis(gid:vec3<u32>,axis:vec2<i32>)->vec4<f32> {
    let radius=i32(round(p.values.x));var total=vec4<f32>(0.0);
    for (var offset=-radius;offset<=radius;offset++) {
        let pos=vec2<i32>(gid.xy)+axis*offset;total+=sample_source(pos.x,pos.y);
    }
    return total/f32(radius*2+1);
}
// A 64-pixel row plus a 256-pixel halo at either end (9 KiB).
var<workgroup> blur_row:array<vec4<f32>,576>;
@compute @workgroup_size(64,1)
fn blur_horizontal(@builtin(global_invocation_id) gid:vec3<u32>,@builtin(local_invocation_id) lid:vec3<u32>) {
    let radius=u32(round(p.values.x));
    if radius<=2u {
        if inside(gid) {result[index(gid)]=blur_axis(gid,vec2<i32>(1,0));}
        return;
    }
    let origin=i32(gid.x-lid.x)-i32(radius);
    for(var item=lid.x;item<64u+2u*radius;item+=64u) {
        blur_row[item]=sample_source(origin+i32(item),i32(gid.y));
    }
    workgroupBarrier();
    if !inside(gid) {return;}
    var total=vec4<f32>(0.0);
    for(var tap=0u;tap<=2u*radius;tap++) {total+=blur_row[lid.x+tap];}
    result[index(gid)]=total/f32(radius*2u+1u);
}
@compute @workgroup_size(8,8)
fn blur_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=blur_axis(gid,vec2<i32>(0,1));
}
@compute @workgroup_size(8,8)
fn merge(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let foreground=second[i];result[i]=foreground+source[i]*(1.0-foreground.a);
}
fn source_pixel(q:vec2<i32>) -> vec4<f32> {
    let bounded=clamp(q,vec2<i32>(0),vec2<i32>(p.values.xy)-vec2<i32>(1));
    return source[(u32(bounded.y)+u32(p.extra.w))*u32(p.extra.x)+u32(bounded.x)+u32(p.extra.z)];
}
@compute @workgroup_size(8,8)
fn image(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    if p.size_op.w==0u {
        var color=vec4<f32>(0.0);
        if gid.x<u32(p.values.x) && gid.y<u32(p.values.y) {color=source[(gid.y+u32(p.extra.w))*u32(p.extra.x)+gid.x+u32(p.extra.z)];}
        result[index(gid)]=color;
        return;
    }
    let native=p.values.xy;
    let canvas=vec2<f32>(p.size_op.xy);
    var scale=vec2<f32>(1.0);
    let ratio=canvas/native;
    if p.size_op.w==1u {scale=vec2<f32>(min(ratio.x,ratio.y));}
    if p.size_op.w==2u {scale=vec2<f32>(max(ratio.x,ratio.y));}
    if p.size_op.w==4u {scale=ratio;}
    var offset=vec2<f32>(0.0);
    if p.size_op.w!=0u {offset=(canvas-native*scale)*0.5;}
    let pos=(vec2<f32>(gid.xy)+vec2<f32>(0.5)-offset)/scale;
    var color=vec4<f32>(0.0);
    if all(pos>=vec2<f32>(0.0)) && all(pos<native) {
        // Pixel-centre sampling in original source storage, before canvas clipping.
        let q=pos-vec2<f32>(0.5);
        let base=vec2<i32>(floor(q));
        let f=fract(q);
        color=mix(mix(source_pixel(base),source_pixel(base+vec2<i32>(1,0)),f.x),
                  mix(source_pixel(base+vec2<i32>(0,1)),source_pixel(base+vec2<i32>(1,1)),f.x),f.y);
    }
    result[index(gid)]=color;
}

// Source-over blending, algebraically premultiplied to avoid division by alpha.
@compute @workgroup_size(8,8)
fn blend(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);
    let b=source[i]; let f=second[i]*p.values.x;
    var rgb=f.rgb+b.rgb*(1.0-f.a);
    if p.size_op.w != 0u {
        var overlap=f.rgb*b.a+b.rgb*f.a;
        switch p.size_op.w {
            case 2u: { overlap=f.rgb*b.rgb; }
            case 3u: { overlap-=f.rgb*b.rgb; }
            case 4u: { overlap=min(f.rgb*b.a,b.rgb*f.a); }
            case 5u: { overlap=max(f.rgb*b.a,b.rgb*f.a); }
            case 6u: { overlap=abs(f.rgb*b.a-b.rgb*f.a); }
            default: {}
        }
        rgb=f.rgb*(1.0-b.a)+b.rgb*(1.0-f.a)+overlap;
    }
    result[i]=vec4<f32>(rgb,f.a+b.a*(1.0-f.a));
}

@compute @workgroup_size(8,8)
fn displace(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let map=second[index(gid)];
    // Premultiplied algebra avoids division by tiny alpha. Transparent maps
    // are neutral, including emissive RGB stored with zero alpha.
    let delta=(2.0*clamp(map.rg,vec2<f32>(0.0),vec2<f32>(map.a))-vec2<f32>(map.a))*p.values.xy;
    result[index(gid)]=bilinear(vec2<f32>(gid.xy)-delta);
}

fn lattice_noise(pos:vec3<i32>,seed:u32)->f32 {
    let bits=bitcast<vec3<u32>>(pos);
    let hash=noise_hash(bits.x ^ noise_hash(bits.y ^ noise_hash(bits.z ^ seed)));
    return f32(hash & 0x00ffffffu)/16777215.0;
}
fn value_noise(pos:vec3<f32>,seed:u32)->f32 {
    let base=vec3<i32>(floor(pos));
    let f=fract(pos);
    let t=f*f*f*(f*(f*6.0-15.0)+10.0);
    let a=mix(lattice_noise(base,seed),lattice_noise(base+vec3<i32>(1,0,0),seed),t.x);
    let b=mix(lattice_noise(base+vec3<i32>(0,1,0),seed),lattice_noise(base+vec3<i32>(1,1,0),seed),t.x);
    let c=mix(lattice_noise(base+vec3<i32>(0,0,1),seed),lattice_noise(base+vec3<i32>(1,0,1),seed),t.x);
    let d=mix(lattice_noise(base+vec3<i32>(0,1,1),seed),lattice_noise(base+vec3<i32>(1,1,1),seed),t.x);
    return mix(mix(a,b,t.y),mix(c,d,t.y),t.z);
}
@compute @workgroup_size(8,8)
fn fractal_noise(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var pos=vec3<f32>(vec2<f32>(gid.xy)/p.values.x,p.values.w);
    var total=0.0; var weight=0.0; var amplitude=1.0;
    for (var octave=0u;octave<u32(p.values.y);octave++) {
        total+=amplitude*value_noise(pos,noise_hash(p.size_op.w+octave));
        weight+=amplitude;
        amplitude*=p.values.z;
        pos*=2.0;
    }
    let n=clamp(total/weight,0.0,1.0);
    result[index(gid)]=vec4<f32>(n,n,n,1.0);
}

@compute @workgroup_size(8,8)
fn channel_mask(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let color=source[index(gid)];
    var coverage=dot(color.rgb,vec3<f32>(0.2126,0.7152,0.0722));
    switch p.size_op.w {
        case 1u: { coverage=color.r; }
        case 2u: { coverage=color.g; }
        case 3u: { coverage=color.b; }
        case 4u: { coverage=color.a; }
        default: {}
    }
    result[index(gid)]=vec4<f32>(clamp(coverage,0.0,color.a));
}

@compute @workgroup_size(8,8)
fn colorize(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let color=source[index(gid)];
    if color.a<=0.0 {
        result[index(gid)]=vec4<f32>(0.0);
        return;
    }
    // Clamp before dividing: HDR RGB / tiny alpha must never overflow.
    let luminance=dot(color.rgb,vec3<f32>(0.2126,0.7152,0.0722));
    let t=clamp(luminance,0.0,color.a)/color.a;
    let low=vec4<f32>(p.values.rgb*p.values.a,p.values.a);
    let high=vec4<f32>(p.extra.rgb*p.extra.a,p.extra.a);
    result[index(gid)]=(low*(1.0-t)+high*t)*color.a;
}

@compute @workgroup_size(8,8)
fn gradient(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let axis=p.values.zw-p.values.xy;
    let span=dot(axis,axis);
    var level=0.0;
    if span>=1e-12 {
        let relative=vec2<f32>(gid.xy)-p.values.xy;
        if p.size_op.w==0u {
            level=dot(relative,axis)/span;
        } else {
            level=length(relative)/sqrt(span);
        }
    }
    level=clamp(level,0.0,1.0);
    result[index(gid)]=vec4<f32>(vec3<f32>(level),1.0);
}

// A square alpha neighborhood factors exactly into horizontal and vertical extrema.
// Zero radius still extracts white coverage, so it cannot alias a colored input.
fn morphology_axis(gid:vec3<u32>,axis:vec2<i32>)->vec4<f32> {
    let erode=p.values.x<0.0;
    let radius=i32(abs(p.values.x));
    var coverage=select(0.0,1.0,erode);
    for(var offset=-radius;offset<=radius;offset++) {
        let pos=vec2<i32>(gid.xy)+axis*offset;
        let alpha=clamp(sample_source(pos.x,pos.y).a,0.0,1.0);
        coverage=select(max(coverage,alpha),min(coverage,alpha),erode);
    }
    return vec4<f32>(coverage);
}
@compute @workgroup_size(8,8)
fn morphology_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=morphology_axis(gid,vec2<i32>(1,0));
}
@compute @workgroup_size(8,8)
fn morphology_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=morphology_axis(gid,vec2<i32>(0,1));
}

@compute @workgroup_size(8,8)
fn combine_mask(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);
    let a=clamp(source[i].a,0.0,1.0);let b=clamp(second[i].a,0.0,1.0);
    var coverage=a+b-a*b;
    switch p.size_op.w {
        case 1u: {coverage=a*b;}
        case 2u: {coverage=a*(1.0-b);}
        case 3u: {coverage=a*(1.0-b)+b*(1.0-a);}
        default: {}
    }
    result[i]=vec4<f32>(clamp(coverage,0.0,1.0));
}

// Periodic on the rising axis. Evolution can advance indefinitely without
// simulation history; four units move exactly one lattice period upwards.
fn fire_lattice(pos:vec3<i32>,seed:u32)->f32 {
    return lattice_noise(vec3<i32>(pos.x,((pos.y%8)+8)%8,pos.z),seed);
}
fn fire_noise(pos:vec3<f32>,seed:u32)->f32 {
    let b=vec3<i32>(floor(pos)); let f=fract(pos); let t=f*f*(3.0-2.0*f);
    let a=mix(fire_lattice(b,seed),fire_lattice(b+vec3<i32>(1,0,0),seed),t.x);
    let c=mix(fire_lattice(b+vec3<i32>(0,1,0),seed),fire_lattice(b+vec3<i32>(1,1,0),seed),t.x);
    let d=mix(fire_lattice(b+vec3<i32>(0,0,1),seed),fire_lattice(b+vec3<i32>(1,0,1),seed),t.x);
    let e=mix(fire_lattice(b+vec3<i32>(0,1,1),seed),fire_lattice(b+vec3<i32>(1,1,1),seed),t.x);
    return mix(mix(a,c,t.y),mix(d,e,t.y),t.z);
}
fn fire_fbm(pos:vec3<f32>,seed:u32)->f32 {
    return fire_noise(pos,seed)*0.48 + fire_noise(pos*2.0,seed+17u)*0.28
        + fire_noise(pos*4.0,seed+37u)*0.16 + fire_noise(pos*8.0,seed+53u)*0.08;
}
@compute @workgroup_size(8,8)
fn fire(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let uv=vec2<f32>((f32(gid.x)-p.values.x)/p.values.z+0.5,(p.values.y-f32(gid.y))/p.values.w);
    if uv.x<=0.0 || uv.x>=1.0 || uv.y<0.0 || uv.y>=1.15 || p.extra.y==0.0 {
        result[index(gid)]=vec4<f32>(0.0);return;
    }
    let edge=smoothstep(0.0,0.08,uv.x)*(1.0-smoothstep(0.92,1.0,uv.x));
    let base=smoothstep(0.0,0.055,uv.y);
    var rgb=vec3<f32>(0.0);var transmittance=1.0;
    // Emission/absorption through eight turbulent depth slices. Each slice
    // contributes a different thin sheet, avoiding a solid silhouette fill.
    for(var slice=0u;slice<8u;slice++) {
        let z=(f32(slice)+0.5)/8.0*2.0-1.0;
        let depth=1.0-z*z;
        let warp=fire_noise(vec3<f32>(uv.x*7.0,uv.y*2.5-p.extra.x*2.0,z*1.7),p.size_op.w+83u)-0.5;
        let pos=vec3<f32>(uv.x*18.0+warp*p.extra.z*4.0*uv.y,uv.y*5.5-p.extra.x*2.0,z*2.0);
        let n=fire_fbm(pos,p.size_op.w);
        let fuel=0.74-uv.y*0.63;
        let density=max(fuel-n,0.0)*edge*base*depth;
        let alpha=1.0-exp(-density*4.5);
        let heat=clamp(density*3.0+(1.0-uv.y)*0.27,0.0,1.0);
        let warm=mix(vec3<f32>(0.9,0.018,0.0008),vec3<f32>(1.2,0.23,0.009),smoothstep(0.05,0.5,heat));
        let color=mix(warm,vec3<f32>(1.9,1.1,0.3),smoothstep(0.5,0.95,heat));
        rgb+=transmittance*alpha*color*p.extra.y;
        transmittance*=1.0-alpha;
    }
    result[index(gid)]=vec4<f32>(rgb,1.0-transmittance);
}

// Add reflected radiance without changing coverage, even on antialiased edges.
@compute @workgroup_size(8,8)
fn metallic_shine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let original=source[i];
    let normal=vec2<f32>(cos(p.values.w),sin(p.values.w));
    let distance=abs(dot(vec2<f32>(gid.xy)-p.values.xy,normal))/p.values.z;
    let shoulder=pow(max(1.0-distance,0.0),3.0);
    let core=pow(max(1.0-distance/0.16,0.0),2.0);
    let light=(0.32*shoulder+0.68*core)*p.extra.x*original.a;
    result[i]=vec4<f32>(original.rgb+vec3<f32>(1.0,0.94,0.82)*light,original.a);
}

// RGB albedo and a height channel in authored texture coordinates. The same
// height field drives shading, so pores and mortar remain attached to the grain.
fn surface_sample(uv:vec2<f32>, kind:u32)->vec4<f32> {
    let coarse=fire_fbm(vec3<f32>(uv*3.0,0.7),719u);
    let fine=fire_noise(vec3<f32>(uv*110.0,1.2),311u);
    var color=vec3<f32>(0.5); var height=0.0;
    if kind==6u || kind==7u {
        let warp=fire_fbm(vec3<f32>(uv.x*1.2,uv.y*0.28,0.2),173u);
        let rings=0.5+0.5*sin(length(vec2<f32>(uv.x*0.7-1.9,uv.y*0.24+0.4))*65.0+warp*9.0);
        let fiber=fire_noise(vec3<f32>(uv.x*180.0,uv.y*18.0,0.1),137u);
        let pores=pow(max(0.0,1.0-fiber*1.8),3.0);
        let latewood=pow(rings,9.0);
        let tone=clamp(0.78-0.32*latewood-0.25*pores+(coarse-0.5)*0.5,0.0,1.0);
        color=mix(vec3<f32>(0.16,0.065,0.018),vec3<f32>(0.69,0.46,0.23),tone);
        if kind==7u {color*=vec3<f32>(0.32,0.23,0.16);}
        height=0.12*latewood-0.25*pores+fiber*0.03;
    } else if kind==8u {
        let warp=vec2<f32>(coarse,fire_fbm(vec3<f32>(uv*2.2,5.1),193u));
        let veins=abs(sin((uv.x+uv.y*0.6+warp.x*1.7)*8.0+warp.y*7.0));
        let vein=1.0-smoothstep(0.015,0.14,veins);
        let branches=1.0-smoothstep(0.015,0.05,abs(sin((uv.y-uv.x+warp.y)*21.0)));
        color=mix(vec3<f32>(0.78,0.77,0.71),vec3<f32>(0.15,0.18,0.19),vein*0.72+branches*0.09);
        color*=0.88+coarse*0.18; height=fine*0.005;
    } else if kind==9u || kind==10u {
        let row=floor(uv.y*3.0); let q=vec2<f32>(uv.x*1.5+fract(row*0.5),uv.y*3.0);
        let cell=floor(q); let f=fract(q);
        let edge=min(min(f.x,1.0-f.x)*2.0,min(f.y,1.0-f.y));
        let brick=smoothstep(0.035,0.075,edge+(fine-0.5)*0.012);
        let variation=fire_noise(vec3<f32>(cell*7.0,0.5),173u);
        var base=mix(vec3<f32>(0.22,0.035,0.012),vec3<f32>(0.55,0.17,0.055),variation);
        if kind==9u {base=mix(vec3<f32>(0.42,0.36,0.24),vec3<f32>(0.75,0.67,0.49),variation);}
        base*=0.72+0.5*coarse+0.16*(fine-0.5);
        color=mix(vec3<f32>(0.28,0.26,0.22)*(0.85+fine*0.25),base,brick);
        height=brick*0.5+brick*(coarse*0.12+fine*0.04);
    } else if kind==11u {
        let aggregate=fire_noise(vec3<f32>(uv*38.0,0.3),937u);
        let pits=1.0-smoothstep(0.19,0.30,aggregate);
        color=vec3<f32>(0.37,0.36,0.33)*(0.7+coarse*0.5+fine*0.12-pits*0.38);
        height=coarse*0.45+aggregate*0.09-pits*0.18;
    } else if kind==12u || kind==13u {
        let fibers=fire_noise(vec3<f32>(uv.x*160.0,uv.y*35.0,0.3),811u);
        color=vec3<f32>(0.88,0.865,0.82)*(0.96+fibers*0.06);
        height=fibers*0.018+fine*0.009;
        if kind==13u {
            let stain=smoothstep(0.43,0.72,coarse);
            let crease=exp(-pow(sin(uv.x*4.2+sin(uv.y*1.8)*0.45)*45.0,2.0));
            color=mix(vec3<f32>(0.64,0.49,0.28),vec3<f32>(0.26,0.13,0.045),stain*0.65);
            color*=0.94+fibers*0.1-crease*0.22;
            height+=coarse*0.12-crease*0.055;
        }
    } else {
        // Jittered Voronoi cells produce irregular leather pebbles rather than a grid.
        let q=uv*78.0;let cell=floor(q);let f=fract(q);
        var first=10.0; var second=10.0;
        for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {
            let offset=vec2<f32>(f32(x),f32(y));let c=cell+offset;
            let jitter=vec2<f32>(fire_noise(vec3<f32>(c*17.0,0.4),19u),fire_noise(vec3<f32>(c*17.0,5.2),83u));
            let distance=length(offset+jitter-f);
            if distance<first {second=first;first=distance;} else {second=min(second,distance);}
        }}
        let groove=1.0-smoothstep(0.015,0.13,second-first);
        var base=vec3<f32>(0.25,0.012,0.018);
        if kind==15u {base=vec3<f32>(0.19,0.064,0.022);}
        if kind==16u {base=vec3<f32>(0.52,0.29,0.12);}
        if kind==17u {base=vec3<f32>(0.055,0.018,0.008);}
        if kind==18u {base=vec3<f32>(0.008,0.009,0.012);}
        color=base*(0.8+coarse*0.36-groove*0.22+fine*0.08);
        height=(1.0-groove)*0.018+coarse*0.035+fine*0.004;
    }
    return vec4<f32>(color,height);
}
fn detailed_surface(uv:vec2<f32>, n:vec3<f32>, kind:u32)->vec3<f32> {
    let sample=surface_sample(uv,kind);
    // Finite differences in pixel space avoid resolution-dependent bump strength.
    let e=max(0.0005,1.0/p.values.x);
    let dx=surface_sample(uv+vec2<f32>(e,0.0),kind).a-surface_sample(uv-vec2<f32>(e,0.0),kind).a;
    let dy=surface_sample(uv+vec2<f32>(0.0,e),kind).a-surface_sample(uv-vec2<f32>(0.0,e),kind).a;
    let normal=normalize(n+vec3<f32>(-dx,-dy,0.0)*p.extra.x/e*select(0.18,0.035,kind==6u || kind==7u));
    return studio_surface(uv,normal,sample.rgb);
}

// Alpha-derived bevel normals, shared optical response and explicit studio lights.
// Metals reflect coloured radiance; dielectrics also carry a diffuse lobe.
// Shared surface evaluation for authored shapes and library sample geometry.
// Fitted GGX LTC tables: Heitz et al., SIGGRAPH 2016; assets/ltc/NOTICE.
// Bilinear lookup matches the source table's roughness / sqrt(1-N.V) axes.
fn ltc_lookup(rough:f32,nv:f32,offset:u32)->vec4<f32> {
    let q=clamp(vec2<f32>(rough,sqrt(1.0-clamp(nv,0.0,1.0))),vec2<f32>(0.0),vec2<f32>(1.0))*63.0;
    let a=vec2<u32>(floor(q));let b=min(a+vec2<u32>(1u),vec2<u32>(63u));let f=fract(q);
    return mix(mix(ltc[offset+a.y*64u+a.x],ltc[offset+a.y*64u+b.x],f.x),
        mix(ltc[offset+b.y*64u+a.x],ltc[offset+b.y*64u+b.x],f.x),f.y);
}
fn ltc_rect(position:vec3<f32>,basis:mat3x3<f32>,center:vec3<f32>,u:vec3<f32>,v:vec3<f32>)->f32 {
    if dot(center-position,cross(u,v))<=0.0 {return 0.0;}
    var vertices=array<vec3<f32>,4>(center-u-v,center+u-v,center+u+v,center-u+v);
    var polygon:array<vec3<f32>,8>;var count=0u;
    // Clip against the shading horizon before spherical edge integration.
    for(var i=0u;i<4u;i++) {
        let a=basis*(vertices[i]-position);let b=basis*(vertices[(i+1u)%4u]-position);
        if a.z>0.0 {polygon[count]=a;count++;}
        if (a.z>0.0)!=(b.z>0.0) {polygon[count]=mix(a,b,a.z/(a.z-b.z));count++;}
    }
    if count<3u {return 0.0;}
    var sum=0.0;
    for(var i=0u;i<count;i++) {
        let pa=polygon[i];let pb=polygon[(i+1u)%count];
        if dot(pa,pa)<1e-12 || dot(pb,pb)<1e-12 {continue;}
        let a=normalize(pa);let b=normalize(pb);
        let c=cross(a,b);let len=length(c);
        if len>0.000001 {sum+=c.z*atan2(len,clamp(dot(a,b),-1.0,1.0))/len;}
    }
    return max(0.0,sum)/6.2831853;
}
fn studio_surface(uv:vec2<f32>, normal:vec3<f32>,albedo:vec3<f32>)->vec3<f32> {
    let kind=p.size_op.w;
    // Lighting coordinates follow canvas proportions, independent of texture scale.
    var position=vec3<f32>((uv*p.values.x-vec2<f32>(p.size_op.xy)*0.5)/(f32(p.size_op.y)*0.5),0.0);
    let texture_position=vec3<f32>(uv,0.0);
    if p.extra.z>0.5 {position=normal;}
    let v=normalize(vec3<f32>(0.0,0.0,6.0)-position);
    let fine=fire_noise(texture_position*140.0+vec3<f32>(0.4,1.3,2.7),71u)-0.5;
    let fine_y=fire_noise(texture_position*140.0+vec3<f32>(4.1,9.2,0.7),191u)-0.5;
    let texture=fire_fbm(texture_position*19.0+vec3<f32>(1.0),257u);
    var bump=vec3<f32>(fine,fine_y,0.0)*0.06*p.extra.x;
    if kind==2u {
        let brush=fire_noise(vec3<f32>(uv.x*3.0,uv.y*min(500.0,p.values.x*0.65),0.0),71u)-0.5;
        bump=vec3<f32>(0.0,brush*0.065,0.0)*p.extra.x;
    }
    let n=normalize(normal+bump);
    let rough=clamp(p.values.z+(texture-0.5)*p.extra.x*select(select(0.35,0.5,kind==3u),0.03,kind==2u),0.03,1.0);
    let f0=studio.optics.rgb;
    let nv=max(dot(n,v),0.0001);
    var tangent=v-n*dot(n,v);
    if dot(tangent,tangent)<0.000001 {tangent=cross(n,vec3<f32>(0.0,1.0,0.0));}
    tangent=normalize(tangent);
    let frame=transpose(mat3x3<f32>(tangent,cross(n,tangent),n));
    let m=ltc_lookup(rough,nv,0u);let amplitude=ltc_lookup(rough,nv,4096u);
    let inverse=mat3x3<f32>(vec3<f32>(m.x,0.0,m.y),vec3<f32>(0.0,1.0,0.0),vec3<f32>(m.z,0.0,m.w));
    let angle=p.values.w*6.2831853;let c=cos(angle);let s=sin(angle);
    let rotation=mat3x3<f32>(vec3<f32>(c,0.0,s),vec3<f32>(0.0,1.0,0.0),vec3<f32>(-s,0.0,c));
    let origin=rotation*position;
    let basis=inverse*frame*transpose(rotation);
    let reflected=f0*amplitude.x+(vec3<f32>(1.0)-f0)*amplitude.y;
    var radiance=vec3<f32>(studio.environment.x);
    var diffuse=vec3<f32>(studio.environment.x);
    let diffuse_basis=frame*transpose(rotation);
    for(var i=0u;i<min(u32(studio.environment.y),8u);i++) {
        let light=studio.lights[i];
        if light.center.w>0.0 {
            radiance+=light.colour.rgb*light.center.w*ltc_rect(origin,basis,light.center.xyz,light.u.xyz,light.v.xyz);
            diffuse+=light.colour.rgb*light.center.w*ltc_rect(origin,diffuse_basis,light.center.xyz,light.u.xyz,light.v.xyz);
        }
    }
    return reflected*radiance+(1.0-studio.optics.w)*(vec3<f32>(1.0)-reflected)*albedo*diffuse;

}
fn material_colour(uv:vec2<f32>,n:vec3<f32>)->vec3<f32> {
    if p.size_op.w<=3u {return studio_surface(uv,n,vec3<f32>(0.0));}
    let rim=pow(max(dot(n,normalize(vec3<f32>(-0.6,-0.8,0.6))),0.0),8.0)*2.4;
    var color=vec3<f32>(0.0);
    if p.size_op.w==4u {
        let phosphor=0.8+p.extra.x*0.45*(fire_noise(vec3<f32>(uv*150.0,0.0),431u)-0.5);
        let pulse=0.92+0.08*cos(p.values.w*6.2831853);
        color=vec3<f32>(0.12,1.9,0.38)*phosphor*p.extra.y*pulse+vec3<f32>(rim*0.08);
    }
    if p.size_op.w==5u {
        let warp=fire_fbm(vec3<f32>(uv.x*0.7,uv.y*2.0,0.5),617u);
        let knot=length((uv-vec2<f32>(2.7,1.5))*vec2<f32>(0.7,2.3));
        let rings=0.5+0.5*sin(uv.y*80.0+warp*50.0+exp(-knot*1.4)*25.0);
        let pores=fire_noise(vec3<f32>(uv.x*7.0,uv.y*220.0,0.0),313u);
        let grain=clamp(mix(0.5,0.75-0.55*pow(rings,6.0)+0.12*pores,p.extra.x),0.0,1.0);
        color=mix(vec3<f32>(0.065,0.017,0.004),vec3<f32>(0.5,0.22,0.065),grain);
        let fibers=fire_noise(vec3<f32>(uv.x*5.0,uv.y*180.0+warp*15.0,0.0),733u);
        let pores_dark=pow(max(0.0,1.0-fibers*2.0),3.0);
        color*=max(0.1,1.0+p.extra.x*((fibers-0.5)*0.45-pores_dark*0.6));
        color=studio_surface(uv,n,color);
    }
    if p.size_op.w>=6u {color=detailed_surface(uv,n,p.size_op.w);}
    return color;
}
@compute @workgroup_size(8,8)
fn material_swatch(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let xy=(vec2<f32>(gid.xy)+vec2<f32>(0.5)-vec2<f32>(128.0,124.0))/108.0;
    let radius=length(xy);
    let coverage=clamp((1.0-radius)*108.0,0.0,1.0);
    if coverage==0.0 {result[index(gid)]=vec4<f32>(0.0);return;}
    let n=vec3<f32>(xy,sqrt(max(0.0,1.0-dot(xy,xy))));
    let uv=vec2<f32>(atan2(n.x,max(0.001,n.z)),asin(clamp(n.y,-1.0,1.0)))*0.9+vec2<f32>(1.2,0.7);
    let color=material_colour(uv,n);
    result[index(gid)]=vec4<f32>(color*coverage,coverage);
}
@compute @workgroup_size(8,8)
fn material(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let coverage=source[i].a;
    if coverage==0.0 {result[i]=vec4<f32>(0.0);return;}
    let uv=vec2<f32>(gid.xy)/p.values.x;
    let r=i32(round(p.values.y)); let xy=vec2<i32>(gid.xy);
    var slope=vec2<f32>(0.0);
    if r>0 {
        for(var ring=1;ring<=8;ring++) {
            let offset=max(1,i32(round(f32(r)*f32(ring)/8.0)));
            let weight=(1.0-f32(ring)/9.0)*0.5;
            slope+=weight*vec2<f32>(sample_source(xy.x-offset,xy.y).a-sample_source(xy.x+offset,xy.y).a,
                                    sample_source(xy.x,xy.y-offset).a-sample_source(xy.x,xy.y+offset).a);
        }
    }
    let n=normalize(vec3<f32>(slope*1.6,1.0));
    let color=material_colour(uv,n);
    result[i]=vec4<f32>(color*coverage,coverage);
}
fn atmosphere_field(q:vec3<f32>,kind:u32)->f32 {
    let time=p.extra.x*2.0; let detail=p.extra.z;
    var coord=vec3<f32>(q.x*6.0,q.y*5.0,q.z*3.2);
    if kind>=2u {coord=vec3<f32>(q.y*3.0,q.x*4.0,q.z*2.3);}
    coord*=detail; coord.y-=time;
    let warp=fire_noise(vec3<f32>(coord.x*0.5,coord.y,coord.z*0.5),p.size_op.z+11u)-0.5;
    let field=fire_fbm(coord+vec3<f32>(warp*1.5,0.0,warp),p.size_op.z);
    return field;
}
// Coherent density rather than a blurred image: lobes are eroded by advected
// turbulence. Samples toward the light estimate interior extinction.
fn atmosphere_density(q:vec3<f32>,kind:u32)->f32 {
    let uv=q.xy;let z=q.z;
    let n=atmosphere_field(q,kind);
    let bounds=(1.0-smoothstep(0.48,0.65,abs(uv.x)))*smoothstep(0.0,0.06,uv.y)*(1.0-smoothstep(0.88,1.0,uv.y));
    if kind==4u {
        var shape=-10.0;
        for(var blob=0u;blob<5u;blob++) {
            let f=f32(blob);
            let center=vec3<f32>(-0.35+f*0.175,0.43+0.19*sin(f*1.3),0.14*cos(f*2.0));
            let radius=0.21+0.055*sin(f*1.9);
            shape=max(shape,radius-length((q-center)*vec3<f32>(1.0,1.0,0.44)));
        }
        let erosion=atmosphere_field(q*2.7,kind)*0.12;
        return smoothstep(-0.025,0.07,shape-erosion+0.015)*bounds*p.extra.y;
    }
    if kind<2u {
        let drift=(fire_noise(vec3<f32>(uv.y*3.0,-p.extra.x*2.0,z),p.size_op.z+61u)-0.5)*0.5*uv.y;
        let radius=0.055+0.39*uv.y;
        let shape=exp(-pow((uv.x+drift)/radius,2.0)*2.0)*max(0.0,1.0-z*z);
        let threshold=0.48+(1.0-shape)*0.13;
        return max(n-threshold,0.0)*shape*bounds*(1.0-smoothstep(0.6,1.0,uv.y))*p.extra.y*9.0;
    }
    let shift=select(0.0,0.2*sin(p.extra.x*1.5707963),kind==3u);
    let shape=exp(-pow((uv.x-shift)/select(0.48,0.32,kind==3u),4.0))*pow(max(sin(uv.y*3.14159265),0.0),2.0)*max(0.0,1.0-z*z);
    return max(n-0.34,0.0)*shape*bounds*p.extra.y;
}
@compute @workgroup_size(8,8)
fn atmosphere(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let uv=vec2<f32>((f32(gid.x)-p.values.x)/p.values.z,(p.values.y-f32(gid.y))/p.values.w);
    let kind=p.size_op.w;
    if abs(uv.x)>0.65 || uv.y<0.0 || uv.y>1.0 || p.extra.y==0.0 {result[index(gid)]=vec4<f32>(0.0);return;}
    var rgb=vec3<f32>(0.0);var transmittance=1.0;
    for(var slice=0u;slice<12u;slice++) {
        let z=(f32(slice)+0.5)/12.0*2.0-1.0;
        let q=vec3<f32>(uv,z);
        let density=atmosphere_density(q,kind);
        let alpha=1.0-exp(-density*0.65);
        if alpha<0.0001 {continue;}
        var extinction=0.0;
        for(var step=1u;step<=3u;step++) {
            extinction+=atmosphere_density(q+vec3<f32>(-0.07,0.10,-0.10)*f32(step),kind);
        }
        let shadow=exp(-extinction*0.85);
        var color=vec3<f32>(0.55,0.59,0.65)*(0.15+shadow*1.2*p.extra.w);
        if kind==1u {color=vec3<f32>(0.055,0.047,0.04)*(0.15+shadow*1.7*p.extra.w);}
        if kind==4u {color=vec3<f32>(0.12,0.17,0.24)+vec3<f32>(0.65,0.60,0.51)*shadow*p.extra.w;}
        rgb+=transmittance*alpha*color;
        transmittance*=1.0-alpha;
    }
    result[index(gid)]=vec4<f32>(rgb,1.0-transmittance);
}

fn bevel_normal(pos:vec2<i32>,radius:f32)->vec3<f32> {
    var slope=vec2<f32>(0.0);
    if radius>0.0 {
        for(var ring=1;ring<=8;ring++) {
            let r=max(1,i32(round(radius*f32(ring)/8.0)));
            let weight=(1.0-f32(ring)/9.0)*0.5;
            slope+=weight*vec2<f32>(sample_source(pos.x-r,pos.y).a-sample_source(pos.x+r,pos.y).a,
                                    sample_source(pos.x,pos.y-r).a-sample_source(pos.x,pos.y+r).a);
        }
    }
    return normalize(vec3<f32>(slope*1.6,1.0));
}
fn sample_second(pos:vec2<i32>)->vec4<f32> {
    let q=clamp(pos,vec2<i32>(0),vec2<i32>(p.size_op.xy)-vec2<i32>(1));
    return second[u32(q.y)*p.size_op.x+u32(q.x)];
}
fn second_bilinear(pos:vec2<f32>)->vec4<f32> {
    let base=vec2<i32>(floor(pos));let f=fract(pos);
    return mix(mix(sample_second(base),sample_second(base+vec2<i32>(1,0)),f.x),
               mix(sample_second(base+vec2<i32>(0,1)),sample_second(base+vec2<i32>(1,1)),f.x),f.y);
}
fn reflected_sample(pos:vec2<f32>,roughness:f32)->vec4<f32> {
    if roughness<0.01 {return second_bilinear(pos);}
    var total=vec4<f32>(0.0);
    for(var i=0u;i<16u;i++) {
        let angle=f32(i)*2.39996323;
        let radius=sqrt((f32(i)+0.5)/16.0)*roughness;
        total+=second_bilinear(pos+vec2<f32>(cos(angle),sin(angle))*radius);
    }
    return total/16.0;
}
@compute @workgroup_size(8,8)
fn surface_reflection(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let original=source[i];
    if original.a==0.0 || p.values.w==0.0 {result[i]=original;return;}
    let n=bevel_normal(vec2<i32>(gid.xy),p.values.x);
    let center=(vec2<f32>(p.size_op.xy)-vec2<f32>(1.0))*0.5;
    let q=(vec2<f32>(gid.xy)-center)/p.extra.y;
    let c=cos(p.extra.x);let s=sin(p.extra.x);
    let rotated=vec2<f32>(c*q.x-s*q.y,s*q.x+c*q.y);
    let mapping=center+vec2<f32>(-rotated.x,rotated.y)+p.extra.zw+n.xy*p.values.y*min(f32(p.size_op.x),f32(p.size_op.y))*0.2;
    let reflected=reflected_sample(mapping,p.values.z);
    var weight=p.values.w;var tint=vec3<f32>(1.0);
    if p.size_op.w==0u {
        let straight=max(original.rgb/original.a,vec3<f32>(0.0));
        tint=mix(vec3<f32>(1.0),straight/max(max(straight.r,straight.g),max(straight.b,0.001)),0.85);
    } else {
        weight*=0.08+0.92*pow(1.0-n.z,5.0);
    }
    result[i]=vec4<f32>(original.rgb*(1.0-weight*reflected.a)+reflected.rgb*tint*weight*original.a,original.a);
}
@compute @workgroup_size(8,8)
fn glass(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let coverage=source[i].a;
    if coverage==0.0 {result[i]=vec4<f32>(0.0);return;}
    let n=bevel_normal(vec2<i32>(gid.xy),p.values.x);
    let pos=vec2<f32>(gid.xy);
    let displacement=-n.xy*p.values.y*(1.0-1.0/p.values.w);
    let center=reflected_sample(pos+displacement,p.values.z);
    var color=center.rgb;
    if p.extra.z>0.0 {
        color.r=reflected_sample(pos+displacement*(1.0+p.extra.z),p.values.z).r;
        color.b=reflected_sample(pos+displacement*(1.0-p.extra.z),p.values.z).b;
    }
    let f0=pow((p.values.w-1.0)/(p.values.w+1.0),2.0);
    let fresnel=f0+(1.0-f0)*pow(1.0-n.z,5.0);
    let spec=pow(max(dot(n,normalize(vec3<f32>(-0.6,-0.7,0.7))),0.0),18.0)*fresnel*3.0;
    let absorption=exp(-p.extra.y*vec3<f32>(0.55,0.14,0.05));
    let rim=spec+fresnel*(1.0-n.z)*0.25;
    let alpha=coverage*(center.a+(1.0-center.a)*clamp(rim,0.0,1.0));
    result[i]=vec4<f32>((color*absorption*p.extra.x*(1.0-fresnel)+vec3<f32>(rim))*coverage,alpha);
}
fn glint_random(cell:vec2<i32>,salt:u32)->f32 {
    return f32(noise_hash(bitcast<u32>(cell.x)*747796405u+bitcast<u32>(cell.y)*2891336453u+p.size_op.w+salt)&65535u)/65535.0;
}
@compute @workgroup_size(8,8)
fn sun_glints(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let original=source[i];
    if p.values.y==0.0 || p.extra.x==0.0 {result[i]=original;return;}
    let cell_size=max(8.0,p.values.x*2.0);
    let cell=vec2<i32>(floor(vec2<f32>(gid.xy)/cell_size));
    var light=0.0;
    for(var y=-1;y<=1;y++) { for(var x=-1;x<=1;x++) {
        let candidate=cell+vec2<i32>(x,y);
        if glint_random(candidate,31u)>=p.extra.x {continue;}
        let point=(vec2<f32>(candidate)+vec2<f32>(glint_random(candidate,79u),glint_random(candidate,149u)))*cell_size;
        let hit=sample_source(i32(round(point.x)),i32(round(point.y)));
        let luminance=dot(hit.rgb,vec3<f32>(0.2126,0.7152,0.0722));
        if hit.a<0.1 || luminance<p.values.z {continue;}
        let pulse=pow(max(0.0,sin(p.values.w*1.570796327+glint_random(candidate,257u)*6.2831853)),8.0);
        let delta=(vec2<f32>(gid.xy)-point)/p.values.x;
        let c=cos(p.extra.y);let s=sin(p.extra.y);
        let q=abs(vec2<f32>(c*delta.x-s*delta.y,s*delta.x+c*delta.y));
        let taper=pow(max(0.0,1.0-length(q)),2.0);
        let spikes=(exp(-q.x*80.0)+exp(-q.y*80.0))*taper;
        let core=exp(-dot(q,q)*220.0);
        light+=(spikes+core)*pulse*hit.a*p.values.y;
    }}
    let alpha=1.0-(1.0-original.a)*exp(-light*0.35);
    result[i]=vec4<f32>(original.rgb+vec3<f32>(1.0,0.96,0.85)*light,alpha);
}
@compute @workgroup_size(8,8)
fn patina(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let original=source[i];
    if original.a==0.0 {result[i]=original;return;}
    let uv=vec2<f32>(gid.xy)/p.values.z;
    let oxidation_field=fire_fbm(vec3<f32>(uv*2.0,0.7),p.size_op.z);
    let fine=fire_noise(vec3<f32>(uv*85.0,0.1),p.size_op.z+41u);
    let dirty=fire_fbm(vec3<f32>(uv*4.0,1.3),p.size_op.z+137u);
    let edge=1.0-bevel_normal(vec2<i32>(gid.xy),p.values.z*0.035).z;
    let wear=1.0-clamp(edge*p.values.w*2.0,0.0,1.0);
    let tarnish=smoothstep(0.25,0.75,oxidation_field)*p.values.x*wear;
    let dirt=smoothstep(0.35,0.7,dirty)*p.values.y*wear;
    var stain=original.rgb*(0.16+fine*0.12);
    if p.size_op.w==1u {stain=vec3<f32>(0.025,0.16,0.105)*(0.45+fine*0.55)*original.a;}
    if p.size_op.w==2u {stain=vec3<f32>(0.3,0.065,0.014)*(0.45+fine*0.55)*original.a;}
    var color=mix(original.rgb,stain,tarnish);
    color=mix(color,vec3<f32>(0.012,0.009,0.006)*original.a,dirt);
    let scratches=smoothstep(0.78,0.92,fire_noise(vec3<f32>(uv.x*0.8,uv.y*210.0,0.3),p.size_op.z+331u))*p.extra.x;
    color=mix(color,original.rgb*0.35,scratches);
    result[i]=vec4<f32>(color,original.a);
}

@compute @workgroup_size(8,8)
fn text_image(@builtin(global_invocation_id) gid:vec3<u32>) {
 if !inside(gid) {return;}
 let a=source[index(gid)].a*p.values.a;
 result[index(gid)]=vec4<f32>(p.values.rgb*a,a);
}

@compute @workgroup_size(8,8)
fn planar_warp(@builtin(global_invocation_id) gid: vec3<u32>) {
    if !inside(gid) { return; }
    let uv = (vec2<f32>(gid.xy)+vec2<f32>(0.5))/vec2<f32>(p.size_op.xy);
    let w = p.extra.z*uv.x+p.extra.w*uv.y+1.0;
    // Pixels looking at the plane behind the camera stay empty (size_op.w selects the side).
    if abs(w)<0.000001 || (p.size_op.w==1u && w<0.0) || (p.size_op.w==2u && w>0.0) {
        result[index(gid)]=vec4<f32>(0.0);return;
    }
    let q = vec2<f32>(p.values.x*uv.x+p.values.y*uv.y+p.values.z,
                     p.values.w*uv.x+p.extra.x*uv.y+p.extra.y)/w;
    result[index(gid)] = bilinear(q*vec2<f32>(p.size_op.xy)-vec2<f32>(0.5));
}

// Surface integration keeps the clean plate untouched outside the inserted layer.
fn surface_rgb(pos:vec2<i32>)->vec3<f32> {
    let q=clamp(pos,vec2<i32>(0),vec2<i32>(p.size_op.xy)-vec2<i32>(1));
    let c=source[u32(q.y)*p.size_op.x+u32(q.x)];
    if c.a<=0.000001 {return vec3<f32>(0.5);}
    return max(c.rgb/c.a,vec3<f32>(0.0));
}
fn surface_luma(pos:vec2<i32>)->f32 {
    return dot(surface_rgb(pos),vec3<f32>(0.2126,0.7152,0.0722));
}
fn surface_foreground_pixel(pos:vec2<i32>)->vec4<f32> {
    if any(pos<vec2<i32>(0)) || any(pos>=vec2<i32>(p.size_op.xy)) {return vec4<f32>(0.0);}
    return second[u32(pos.y)*p.size_op.x+u32(pos.x)];
}
fn surface_foreground(pos:vec2<f32>)->vec4<f32> {
    if any(pos<vec2<f32>(-1.0)) || any(pos>vec2<f32>(p.size_op.xy)) {return vec4<f32>(0.0);}
    let q=vec2<i32>(floor(pos));let f=fract(pos);
    return mix(mix(surface_foreground_pixel(q),surface_foreground_pixel(q+vec2<i32>(1,0)),f.x),
               mix(surface_foreground_pixel(q+vec2<i32>(0,1)),surface_foreground_pixel(q+vec2<i32>(1,1)),f.x),f.y);
}
@compute @workgroup_size(8,8)
fn surface_blend(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let bg=source[i];
    if p.values.x==0.0 {result[i]=bg;return;}
    let q=vec2<i32>(gid.xy);let pos=vec2<f32>(gid.xy);
    var mean=vec3<f32>(0.5);var local=0.5;
    if p.values.y>0.0 || p.values.z>0.0 || p.values.w>0.0 || p.extra.y>0.0 {
        // Scale the local texture window with composition dimensions.
        let radius=max(2,i32(min(p.size_op.x,p.size_op.y)/180u));
        mean=vec3<f32>(0.0);
        for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {mean+=surface_rgb(q+vec2<i32>(x,y)*radius);}}
        mean/=9.0;local=surface_luma(q);
    }
    let average=max(dot(mean,vec3<f32>(0.2126,0.7152,0.0722)),0.02);
    var delta=vec2<f32>(0.0);
    if p.extra.y>0.0 {
        delta=clamp(vec2<f32>(surface_luma(q+vec2<i32>(1,0))-surface_luma(q-vec2<i32>(1,0)),
            surface_luma(q+vec2<i32>(0,1))-surface_luma(q-vec2<i32>(0,1)))/(2.0*average),vec2<f32>(-1.0),vec2<f32>(1.0))*p.extra.y*bg.a;
    }
    var fg=surface_foreground(pos+delta);
    if p.extra.z>0.0 {
        fg=vec4<f32>(0.0);
        for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {
            let weight=select(1.0,2.0,x==0)*select(1.0,2.0,y==0);
            fg+=surface_foreground(pos+delta+vec2<f32>(f32(x),f32(y))*p.extra.z)*weight/16.0;
        }}
    }
    if fg.a<=0.0 {result[i]=bg;return;}
    let illumination=mix(vec3<f32>(1.0),clamp(mean*2.0,vec3<f32>(0.0),vec3<f32>(4.0)),p.values.y*bg.a);
    let texture=mix(1.0,clamp(local/average,0.0,2.0),p.values.z*bg.a);
    let recess=clamp((average-local)/average,0.0,1.0);
    let worn=smoothstep(p.extra.x,min(p.extra.x+0.12,1.001),recess)*p.values.w*bg.a;
    fg=vec4<f32>(fg.rgb*illumination*texture,fg.a)*(p.values.x*(1.0-worn));
    if p.extra.w>0.0 {
        let n=f32(noise_hash(gid.x+gid.y*65537u+p.size_op.w*747796405u)&65535u)/65535.0-0.5;
        fg=vec4<f32>(max(vec3<f32>(0.0),fg.rgb+vec3<f32>(n*p.extra.w*fg.a)),fg.a);
    }
    result[i]=fg+bg*(1.0-fg.a);
}
