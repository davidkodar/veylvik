struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> second: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
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
// A 64-pixel row plus a maximum 32-pixel halo at either end (2 KiB).
var<workgroup> blur_row:array<vec4<f32>,128>;
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
@compute @workgroup_size(8,8)
fn image(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var color=vec4<f32>(0.0);
    if gid.x<u32(p.values.x) && gid.y<u32(p.values.y) {color=source[(gid.y+u32(p.extra.w))*u32(p.extra.x)+gid.x+u32(p.extra.z)];}
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
