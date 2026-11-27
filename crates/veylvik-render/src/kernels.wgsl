@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> second: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> third: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read> fourth: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> ltc: array<vec4<f32>>;
@group(0) @binding(6) var<uniform> studio:StudioLighting;
@group(0) @binding(7) var<storage,read> environment_map:array<vec4<f32>>;
fn inside(gid:vec3<u32>)->bool { return gid.x<p.size_op.x && gid.y<p.size_op.y; }
fn index(gid:vec3<u32>)->u32 { return gid.y*p.size_op.x+gid.x; }
fn sample_source(x:i32,y:i32)->vec4<f32> {
    if x<0 || y<0 || x>=i32(p.size_op.x) || y>=i32(p.size_op.y) {return vec4<f32>(0.0);}
    return source[u32(y)*p.size_op.x+u32(x)];
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
// Copies of the source in a grid: values = columns, rows, step x, step y; extra.x =
// fade per copy. Later copies lie over earlier ones.
@compute @workgroup_size(8,8)
fn repeat(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let columns=i32(clamp(p.values.x,1.0,32.0)); let rows=i32(clamp(p.values.y,1.0,32.0));
    let at=vec2<f32>(gid.xy);
    var colour=vec4<f32>(0.0);
    var n=0;
    for(var j=0;j<32;j++) {
        if j>=rows {break;}
        for(var i=0;i<32;i++) {
            if i>=columns || n>=256 {break;}
            let copy=bilinear(at-vec2<f32>(f32(i),f32(j))*p.values.zw)*pow(1.0-p.extra.x,f32(n));
            colour=copy+colour*(1.0-copy.a);
            n+=1;
        }
    }
    result[index(gid)]=colour;
}
// Rounded rectangle: values = centre, half size; extra = corner radius, feather, fill
// red, fill green; size_op.zw = fill blue and opacity as float bits.
@compute @workgroup_size(8,8)
fn rectangle(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var alpha=0.0;
    if p.values.z>0.0 && p.values.w>0.0 {
        let corner=min(p.extra.x,min(p.values.z,p.values.w));
        let q=abs(vec2<f32>(gid.xy)+vec2<f32>(0.5)-p.values.xy)-(p.values.zw-vec2<f32>(corner));
        let distance=length(max(q,vec2<f32>(0.0)))+min(max(q.x,q.y),0.0)-corner;
        alpha=1.0-smoothstep(-p.extra.y*0.5,p.extra.y*0.5,distance);
    }
    let fill=vec4<f32>(p.extra.z,p.extra.w,bitcast<f32>(p.size_op.z),bitcast<f32>(p.size_op.w));
    let a=alpha*fill.a;
    result[index(gid)]=vec4<f32>(fill.rgb*a,a);
}
@compute @workgroup_size(8,8)
fn mask(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);result[i]=source[i]*clamp(second[i].a,0.0,1.0);
}
// Box weights are all 1; gaussian (values.y = 1) uses sigma = radius / 3, so the
// radius stays the visible reach of the blur for both filters.
fn blur_weight(offset:i32,radius:i32,gaussian:bool)->f32 {
    if !gaussian {return 1.0;}
    let sigma=max(f32(radius)/3.0,0.5);
    return exp(-f32(offset*offset)/(2.0*sigma*sigma));
}
// Blur treats the outside as transparent; frost extends the edge pixels instead,
// so a full-frame background stays opaque up to the frame edge.
fn blur_tap(x:i32,y:i32,extend:bool)->vec4<f32> {
    if !extend {return sample_source(x,y);}
    let q=clamp(vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(p.size_op.xy)-vec2<i32>(1));
    return source[u32(q.y)*p.size_op.x+u32(q.x)];
}
fn blur_axis(gid:vec3<u32>,axis:vec2<i32>,radius:i32,gaussian:bool,extend:bool)->vec4<f32> {
    var total=vec4<f32>(0.0);var weights=0.0;
    for (var offset=-radius;offset<=radius;offset++) {
        let pos=vec2<i32>(gid.xy)+axis*offset;let w=blur_weight(offset,radius,gaussian);
        total+=blur_tap(pos.x,pos.y,extend)*w;weights+=w;
    }
    return total/weights;
}
// A 64-pixel row plus a 256-pixel halo at either end (9 KiB).
var<workgroup> blur_row:array<vec4<f32>,576>;
@compute @workgroup_size(64,1)
fn blur_horizontal(@builtin(global_invocation_id) gid:vec3<u32>,@builtin(local_invocation_id) lid:vec3<u32>) {
    blur_row_pass(gid,lid,u32(round(p.values.x)),p.values.y>=0.5,false);
}
// Frosted glass blurs its background with a gaussian whose reach is the roughness.
@compute @workgroup_size(64,1)
fn frost_horizontal(@builtin(global_invocation_id) gid:vec3<u32>,@builtin(local_invocation_id) lid:vec3<u32>) {
    blur_row_pass(gid,lid,u32(round(clamp(p.values.z,0.0,256.0))),true,true);
}
@compute @workgroup_size(8,8)
fn frost_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[index(gid)]=blur_axis(gid,vec2<i32>(0,1),i32(round(clamp(p.values.z,0.0,256.0))),true,true);
}
fn blur_row_pass(gid:vec3<u32>,lid:vec3<u32>,radius:u32,gaussian:bool,extend:bool) {
    if radius<=2u {
        if inside(gid) {result[index(gid)]=blur_axis(gid,vec2<i32>(1,0),i32(radius),gaussian,extend);}
        return;
    }
    let origin=i32(gid.x-lid.x)-i32(radius);
    for(var item=lid.x;item<64u+2u*radius;item+=64u) {
        blur_row[item]=blur_tap(origin+i32(item),i32(gid.y),extend);
    }
    workgroupBarrier();
    if !inside(gid) {return;}
    var total=vec4<f32>(0.0);var weights=0.0;
    for(var tap=0u;tap<=2u*radius;tap++) {
        let w=blur_weight(i32(tap)-i32(radius),i32(radius),gaussian);
        total+=blur_row[lid.x+tap]*w;weights+=w;
    }
    result[index(gid)]=total/weights;
}
// Lens blur in three passes sharing values (radius, highlight boost, threshold).
// N samples cover the disc on a golden-angle spiral; when N is below the disc area the
// image is first box-filtered to the sample spacing, so even a one-pixel light spreads
// into a solid, evenly lit disc instead of a speckled one.
fn lens_samples(radius:f32)->u32 {return u32(clamp(ceil(3.14159265*radius*radius),1.0,400.0));}
fn lens_prefilter(radius:f32)->i32 {
    let area=3.14159265*radius*radius;let count=f32(lens_samples(radius));
    if count>=area {return 0;}
    // Spiral gaps reach about 1.5x the mean spacing; the box must bridge the widest.
    return i32(ceil(sqrt(area/count)*0.75));
}
// Highlights above threshold (scene-linear luminance) gain energy, so lights bloom.
fn lens_boosted(c:vec4<f32>)->vec4<f32> {
    let l=dot(max(c.rgb,vec3<f32>(0.0)),vec3<f32>(0.2126,0.7152,0.0722));
    return vec4<f32>(c.rgb*(1.0+p.values.y*max(l-p.values.z,0.0)),c.a);
}
@compute @workgroup_size(8,8)
fn lens_prefilter_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let s=lens_prefilter(max(p.values.x,0.0));var total=vec4<f32>(0.0);
    for(var o=-s;o<=s;o++) {total+=lens_boosted(sample_source(i32(gid.x)+o,i32(gid.y)));}
    result[index(gid)]=total/f32(2*s+1);
}
@compute @workgroup_size(8,8)
fn lens_prefilter_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let s=lens_prefilter(max(p.values.x,0.0));var total=vec4<f32>(0.0);
    for(var o=-s;o<=s;o++) {total+=sample_source(i32(gid.x),i32(gid.y)+o);}
    result[index(gid)]=total/f32(2*s+1);
}
@compute @workgroup_size(8,8)
fn lens_blur(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let radius=max(p.values.x,0.0);
    let count=lens_samples(radius);
    var total=vec4<f32>(0.0);
    if count<=1u {result[index(gid)]=sample_source(i32(gid.x),i32(gid.y));return;}
    if f32(count)>=3.14159265*radius*radius {
        // Small discs: every pixel inside the disc, exactly once.
        let r=i32(ceil(radius));var n=0.0;
        for(var dy=-r;dy<=r;dy++) {for(var dx=-r;dx<=r;dx++) {
            if f32(dx*dx+dy*dy)<=radius*radius {total+=sample_source(i32(gid.x)+dx,i32(gid.y)+dy);n+=1.0;}
        }}
        result[index(gid)]=total/n;return;
    }
    for(var n=0u;n<count;n++) {
        let r=radius*sqrt((f32(n)+0.5)/f32(count));
        let a=f32(n)*2.39996323;
        let q=vec2<f32>(gid.xy)+vec2<f32>(cos(a),sin(a))*r;
        total+=sample_source(i32(round(q.x)),i32(round(q.y)));
    }
    result[index(gid)]=total/f32(count);
}
@compute @workgroup_size(8,8)
fn blur_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=blur_axis(gid,vec2<i32>(0,1),i32(round(p.values.x)),p.values.y>=0.5,false);
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
        // values.zw: where the picture's top left corner sits in the frame (a drawn
        // shape is kept over its own box only).
        let ox=u32(p.values.z);let oy=u32(p.values.w);
        if gid.x>=ox && gid.y>=oy && gid.x-ox<u32(p.values.x) && gid.y-oy<u32(p.values.y) {color=source[(gid.y-oy+u32(p.extra.w))*u32(p.extra.x)+gid.x-ox+u32(p.extra.z)];}
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
    // Centred on whole pixels, rounded down as editors do: a centre half a pixel off
    // would make the whole picture half a pixel soft where the bars are an odd width.
    if p.size_op.w!=0u {offset=floor((canvas-native*scale)*0.5);}
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
            case 7u, 8u: {
                // Overlay and soft light work on perceptual (square-root) values of the
                // straight colours, 0..1, as the eye judges dark and bright.
                let x=sqrt(clamp(b.rgb/max(b.a,1e-6),vec3<f32>(0.0),vec3<f32>(1.0)));
                let y=sqrt(clamp(f.rgb/max(f.a,1e-6),vec3<f32>(0.0),vec3<f32>(1.0)));
                var o=(1.0-2.0*y)*x*x+2.0*y*x;
                if p.size_op.w==7u {
                    o=select(1.0-2.0*(1.0-x)*(1.0-y),2.0*x*y,x<vec3<f32>(0.5));
                }
                overlap=o*o*f.a*b.a;
            }
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

@compute @workgroup_size(8,8)
fn fire(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let uv=vec2<f32>((f32(gid.x)-p.values.x)/p.values.z+0.5,(p.values.y-f32(gid.y))/p.values.w);
    if uv.x<=0.0 || uv.x>=1.0 || uv.y<0.0 || uv.y>=1.15 || p.extra.y==0.0 {
        result[index(gid)]=vec4<f32>(0.0);return;
    }
    // Flames shorten and thin over the outer fifth of each side, so the volume ends
    // like a burning line instead of a cut-off box even where it is dense.
    let edge=smoothstep(0.0,0.2,min(uv.x,1.0-uv.x));
    let time=fract(p.extra.x/4.0)*4.0;
    let fuel_patch=fire_noise(vec3<f32>(uv.x*9.0,0.0,0.3),p.size_op.w+193u);
    let base=smoothstep(0.0,0.07,uv.y);
    let taper=1.0-smoothstep(0.85,1.12,uv.y);
    var rgb=vec3<f32>(0.0);var transmittance=1.0;
    // Emission/absorption through eight turbulent depth slices. Each slice
    // contributes a different thin sheet, avoiding a solid silhouette fill.
    for(var slice=0u;slice<8u;slice++) {
        let z=(f32(slice)+0.5)/8.0*2.0-1.0;
        let depth=1.0-z*z;
        // Buoyant advection stretches structures vertically. Large eddies bend
        // the tongues; small scales tear their edges rather than filling a cloud.
        let rise=uv.y*3.2-time*2.0;
        let bend=fire_noise(vec3<f32>(uv.x*5.0,uv.y*1.7-time*2.0,z),p.size_op.w+83u)-0.5;
        let pos=vec3<f32>(uv.x*15.0+bend*p.extra.z*(1.0+uv.y*4.0),rise,z*1.4);
        let coarse=fire_noise(pos,p.size_op.w);
        let detail=fire_fbm(pos*2.0,p.size_op.w+17u);
        let fuel=(0.69+fuel_patch*0.13)*mix(0.3,1.0,edge)-uv.y*0.64;
        let body=max(fuel-coarse*0.72-detail*0.28,0.0);
        let density=body*sqrt(edge)*base*depth*taper;
        let alpha=1.0-exp(-density*3.2);
        let heat=clamp(body*2.4+(1.0-uv.y)*0.18,0.0,1.0);
        let warm=mix(vec3<f32>(0.55,0.009,0.0002),vec3<f32>(1.8,0.24,0.008),smoothstep(0.08,0.48,heat));
        let color=mix(warm,vec3<f32>(2.5,1.5,0.45),smoothstep(0.48,0.9,heat));
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
fn detailed_surface(uv:vec2<f32>, n:vec3<f32>, kind:u32)->vec3<f32> {
    let b=bumped(uv,n,kind);
    return studio_surface(uv,b.normal,b.albedo);
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
// The environment contains nine GGX-filtered levels and a diffuse irradiance/pi layer.
// The shot as an environment: six box-filtered levels from half resolution down,
// packed in one canvas-sized buffer. Roughness picks the level, so a rough surface
// reflects a blurred shot and a polished one a sharp, half-resolution one.
fn pyramid_size(level:u32)->vec2<u32> {
    let s=1u<<level;
    return max(vec2<u32>(1u),(p.size_op.xy+vec2<u32>(s-1u))/s);
}
fn pyramid_offset(level:u32)->u32 {
    var offset=0u;
    for(var k=1u;k<level;k++) {let z=pyramid_size(k);offset+=z.x*z.y;}
    return offset;
}
@compute @workgroup_size(8,8)
fn environment_pyramid(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var i=index(gid);
    for(var k=1u;k<=6u;k++) {
        let z=pyramid_size(k);
        if i<z.x*z.y {
            let s=1u<<k;
            let lo=vec2<u32>(i%z.x,i/z.x)*s;
            let hi=min(lo+vec2<u32>(s),p.size_op.xy);
            var total=vec4<f32>(0.0);
            for(var y=lo.y;y<hi.y;y++) {for(var x=lo.x;x<hi.x;x++) {total+=source[y*p.size_op.x+x];}}
            result[index(gid)]=total/f32((hi.x-lo.x)*(hi.y-lo.y));
            return;
        }
        i-=z.x*z.y;
    }
}
fn shot_level(uv:vec2<f32>,level:u32)->vec3<f32> {
    let z=pyramid_size(level);let offset=pyramid_offset(level);
    let q=clamp(uv*vec2<f32>(z)-vec2<f32>(0.5),vec2<f32>(0.0),vec2<f32>(z)-vec2<f32>(1.0));
    let a=vec2<u32>(floor(q));let b=min(a+vec2<u32>(1u),z-vec2<u32>(1u));let f=fract(q);
    return mix(mix(third[offset+a.y*z.x+a.x].rgb,third[offset+a.y*z.x+b.x].rgb,f.x),
        mix(third[offset+b.y*z.x+a.x].rgb,third[offset+b.y*z.x+b.x].rgb,f.x),f.y);
}
fn mirror_repeat(t:vec2<f32>)->vec2<f32> {
    let f=fract(t*0.5)*2.0;
    return select(f,vec2<f32>(2.0)-f,f>vec2<f32>(1.0));
}
// The shot stands in for the world behind the camera: mirrored about the surface's
// own screen position and pushed along the reflected direction, so bevels and
// curved faces pick up the surrounding footage. Reflection motion slides it.
fn shot_environment(screen:vec2<f32>,direction:vec3<f32>,rough:f32)->vec3<f32> {
    let uv=mirror_repeat(vec2<f32>(1.0-screen.x+p.values.w,screen.y)+vec2<f32>(-direction.x,direction.y)*0.35);
    let level=1.0+clamp(rough,0.0,1.0)*5.0;
    let lo=u32(floor(level));
    return mix(shot_level(uv,lo),shot_level(uv,min(lo+1u,6u)),fract(level));
}
// A connected Roughness map sets this per pixel in `material` (negative: none).
// Stored map data: images are linearised on import, so undo the sRGB curve to
// recover the 8/16-bit values the map was authored with.
fn map_data(c:f32)->f32 {
    let v=clamp(c,0.0,1.0);
    return select(1.055*pow(v,1.0/2.4)-0.055,v*12.92,v<=0.0031308);
}
fn map_value(c:vec4<f32>)->vec3<f32> {
    let s=c.rgb/max(c.a,0.0001);
    return vec3<f32>(map_data(s.r),map_data(s.g),map_data(s.b));
}
// PBR maps packed into one image: normal xy in the shading frame (x right, y down),
// roughness and height. Unconnected maps are never read.
@compute @workgroup_size(8,8)
fn surface_maps(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let flags=map_flags();
    var packed=vec4<f32>(0.0,0.0,-1.0,0.0);
    if (flags&1u)!=0u {
        let t=map_value(source[i])*2.0-vec3<f32>(1.0);
        // OpenGL maps point green up; the shading frame points y down.
        packed.x=t.x;packed.y=select(-t.y,t.y,(flags&8u)!=0u);
    }
    if (flags&2u)!=0u {packed.z=dot(map_value(second[i]),vec3<f32>(1.0/3.0));}
    if (flags&4u)!=0u {packed.w=dot(map_value(third[i]),vec3<f32>(1.0/3.0));}
    result[i]=packed;
}
fn map_height(x:i32,y:i32)->f32 {
    let q=clamp(vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(p.size_op.xy)-vec2<i32>(1));
    return fourth[u32(q.y)*p.size_op.x+u32(q.x)].w;
}
// Tilts the bevel normal by the Normal map and the Height map's slope, blended so
// the bevel and the detail both survive (whiteout blending).
fn surface_detail(pos:vec2<f32>,n:vec3<f32>)->vec3<f32> {
    let flags=map_flags();
    if (flags&5u)==0u {return n;}
    let q=clamp(vec2<i32>(floor(pos)),vec2<i32>(0),vec2<i32>(p.size_op.xy)-vec2<i32>(1));
    var detail=vec3<f32>(0.0,0.0,1.0);
    if (flags&1u)!=0u {
        let m=fourth[u32(q.y)*p.size_op.x+u32(q.x)].xy;
        let z=sqrt(max(1.0-dot(m,m),0.0));
        detail=normalize(vec3<f32>(m*studio.maps.x,max(z,0.001)));
    }
    if (flags&4u)!=0u {
        let depth=studio.maps.y*0.5;
        let dx=(map_height(q.x+1,q.y)-map_height(q.x-1,q.y))*depth;
        let dy=(map_height(q.x,q.y+1)-map_height(q.x,q.y-1))*depth;
        let bump=normalize(vec3<f32>(-dx,-dy,1.0));
        detail=normalize(vec3<f32>(detail.xy+bump.xy,detail.z*bump.z));
    }
    return normalize(vec3<f32>(n.xy+detail.xy,n.z*detail.z));
}
fn studio_surface(uv:vec2<f32>, normal:vec3<f32>,albedo:vec3<f32>)->vec3<f32> {
    // Lighting coordinates follow canvas proportions, independent of texture scale.
    var position=vec3<f32>((uv*p.values.x-vec2<f32>(p.size_op.xy)*0.5)/(f32(p.size_op.y)*0.5),0.0);
    if p.extra.z>0.5 {position=normal;}
    var v=normalize(vec3<f32>(0.0,0.0,6.0)-position);
    // The swatch is an orthographic sphere; a perspective view vector would
    // shade part of its back-facing hemisphere and exaggerate the bright rim.
    if p.extra.z>0.5 {v=vec3<f32>(0.0,0.0,1.0);}
    let point=surface_point(uv,normal,albedo);
    var n=point.normal;
    let grain_turn=point.turn;
    let grain_across=point.across;
    let plate_gain=point.gain;
    if p.extra.w>0.5 {
        // Real-time anisotropic IBL approximation (Filament bent reflection
        // normal): brush direction shapes the lobe instead of blurring it
        // equally in every direction. Use the same frame for area lights.
        var along=cross(grain_across,n);
        if dot(along,along)<0.000001 {along=vec3<f32>(grain_turn.x,grain_turn.y,0.0);}
        along=normalize(along);
        let projected_view=cross(cross(along,v),along);
        let anisotropic_normal=normalize(projected_view+n*0.00001);
        n=normalize(mix(n,anisotropic_normal,clamp(p.extra.x*1.1,0.0,0.85)));
    }
    let rough=point.rough;
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
    // Shot mixes the studio's reflections with the connected footage (0 = studio only).
    let shot=select(0.0,studio.palette[0].w,p.size_op.z==1u);
    let studio_part=radiance;let studio_diffuse=diffuse;
    if studio.environment.z>0.0 {
        let ec=cos(studio.environment.w);let es=sin(studio.environment.w);
        let environment_rotation=mat3x3<f32>(vec3<f32>(ec,0.0,es),vec3<f32>(0.0,1.0,0.0),vec3<f32>(-es,0.0,ec));
        radiance+=environment_specular(environment_rotation*rotation*reflect(-v,n),rough)*studio.environment.z;
        if studio.optics.w<0.5 {diffuse+=environment_layer(environment_rotation*rotation*n,9u)*studio.environment.z;}
    }
    if shot>0.0 && studio.environment.z>0.0 {
        // Environment strength 0.65 (the studio default) shows the shot at its own brightness.
        let gain=studio.environment.z/0.65;
        let screen=uv*p.values.x/vec2<f32>(p.size_op.xy);
        let reflected_shot=studio_part+shot_environment(screen,reflect(-v,n),rough)*gain;
        radiance=mix(radiance,reflected_shot,shot);
        if studio.optics.w<0.5 {diffuse=mix(diffuse,studio_diffuse+shot_environment(screen,n,1.0)*gain,shot);}
    }
    let diffuse_basis=frame*transpose(rotation);
    for(var i=0u;i<min(u32(studio.environment.y),8u);i++) {
        let light=studio.lights[i];
        if light.center.w>0.0 {
            radiance+=light.colour.rgb*light.center.w*ltc_rect(origin,basis,light.center.xyz,light.u.xyz,light.v.xyz);
            if studio.optics.w<0.5 {diffuse+=light.colour.rgb*light.center.w*ltc_rect(origin,diffuse_basis,light.center.xyz,light.u.xyz,light.v.xyz);}
        }
    }
    return reflected*radiance*plate_gain+(1.0-studio.optics.w)*(vec3<f32>(1.0)-reflected)*albedo*diffuse;

}
// Glass: the shot behind, bent by the bevel and tinted, plus studio reflections.
// The full-resolution shot sits in the packed bevel buffer (rgb) and its roughness
// pyramid in the third input, so clear glass is sharp and frosted glass is smooth.
// Needs the Environment input; without it only the reflections remain.
fn glass_refracted(pixel:vec2<f32>,rough:f32)->vec3<f32> {
    let screen=pixel/vec2<f32>(p.size_op.xy);
    let level=clamp(rough,0.0,1.0)*6.0;
    if level<1.0 {return mix(second_bilinear(pixel).rgb,shot_level(screen,1u),level);}
    let lo=u32(floor(level));
    return mix(shot_level(screen,lo),shot_level(screen,min(lo+1u,6u)),fract(level));
}
fn glass_surface(uv:vec2<f32>,n:vec3<f32>)->vec3<f32> {
    let reflections=studio_surface(uv,n,vec3<f32>(0.0));
    if p.size_op.z!=1u {return reflections;}
    let f0=studio.optics.g;
    let ior=(1.0+sqrt(f0))/max(1.0-sqrt(f0),0.0001);
    let pixel=uv*p.values.x;
    // Stronger bevels and denser materials bend the background further.
    let bend=-n.xy*p.values.y*6.0*(1.0-1.0/ior);
    let rough=surface_roughness();
    var through=glass_refracted(pixel+bend,rough);
    // Dense stones split colour at the edges.
    let spread=max(ior-1.6,0.0)*0.6;
    if spread>0.0 {
        through.r=glass_refracted(pixel+bend*(1.0+spread),rough).r;
        through.b=glass_refracted(pixel+bend*(1.0-spread),rough).b;
    }
    let screen=pixel/vec2<f32>(p.size_op.xy);
    // Frosting scatters light: a milky lift towards the shot's average colour.
    through=mix(through,shot_level(screen,6u)*1.05+vec3<f32>(0.05),clamp(rough,0.0,1.0)*0.45);
    // Thick, steep edges absorb and reflect more: a darker band that defines the shape.
    let edge=1.0-clamp(n.z,0.0,1.0);
    through*=mix(1.0,0.45,pow(edge,1.3));
    let fresnel=f0+(1.0-f0)*pow(edge,5.0);
    // The rims also mirror the scene around them, not only the studio.
    let scene=shot_environment(screen,reflect(vec3<f32>(0.0,0.0,-1.0),n),rough)*(studio.environment.z/0.65);
    return through*studio.palette[0].rgb*(1.0-fresnel)+reflections+scene*(fresnel+edge*0.35);
}
// A 3D renderer's neon core (0 to 1, where the tube faces the viewer); below 0 the
// core follows the 2D bevel height.
var<private> tube_core:f32=-1.0;
fn material_colour(uv:vec2<f32>,n:vec3<f32>)->vec3<f32> {
    if p.size_op.w<=3u {return studio_surface(uv,n,vec3<f32>(0.0));}
    let rim=pow(max(dot(n,normalize(vec3<f32>(-0.6,-0.8,0.6))),0.0),8.0)*2.4;
    var color=vec3<f32>(0.0);
    if p.size_op.w==4u {
        let phosphor=0.8+p.extra.x*0.45*(fire_noise(vec3<f32>(uv*150.0,0.0),431u)-0.5);
        let pulse=0.92+0.08*cos(p.values.w*6.2831853);
        // A tube glows white-hot along its core and saturated towards its edges.
        let glow=studio.palette[0].rgb*1.9;
        var core=smoothstep(0.55,1.0,second_bilinear(uv*p.values.x).a);
        if tube_core>=0.0 {core=tube_core;}
        let hot=vec3<f32>(max(glow.r,max(glow.g,glow.b)));
        // Keep the tube saturated: the core only warms towards white, and the level
        // stays near display white so the colour survives; the bloom carries the glow.
        color=mix(glow,hot,0.18*core)*phosphor*p.extra.y*pulse*(0.45+0.25*core)+vec3<f32>(rim*0.08);
        // The original green phosphor keeps its flat glow; only recoloured neon
        // tubes use the saturated core shading.
        if studio.palette[2].w<0.5 {color=glow*phosphor*p.extra.y*pulse+vec3<f32>(rim*0.08);}
    }
    if p.size_op.w==5u {
        color=wood_colour(uv);
        color=studio_surface(uv,n,color);
    }
    if p.size_op.w==24u {return glass_surface(uv,n);}
    if p.size_op.w==25u {return detailed_surface(uv,n,25u);}
    if p.size_op.w==28u {
        // Fabric catches light at grazing angles, strongest on velvet's pile.
        let sheen=pow(1.0-clamp(n.z,0.0,1.0),2.0);
        return detailed_surface(uv,n,28u)+studio.palette[0].rgb*sheen*0.6;
    }
    if p.size_op.w==26u {
        // Pearl: a pale dielectric whose sheen shifts colour with the viewing angle.
        let base=detailed_surface(uv,n,26u);
        let angle=1.0-clamp(n.z,0.0,1.0);
        // The film's thickness swirls across the face, so the colours flow over it.
        let swirl=fire_fbm(vec3<f32>(uv*2.5,0.3),733u)*2.0;
        let film=vec3<f32>(0.6)+0.4*cos(6.2831853*(vec3<f32>(angle*1.6+swirl)+vec3<f32>(0.0,0.33,0.67)));
        // The film also adds its own colour, so a dark body (oil slick) still shows it.
        let sheen=film*studio.palette[1].rgb;
        return base*mix(vec3<f32>(1.0),sheen*1.4,0.25*angle+0.18)+sheen*(0.1+0.35*angle*angle);
    }
    if p.size_op.w==23u {
        // Image texture: colour from the packed texture at this pixel; its own
        // detail tilts the normal a little, scaled by Grain.
        let pixel=uv*p.values.x;
        let albedo=second_bilinear(pixel).rgb;
        let l=vec3<f32>(0.2126,0.7152,0.0722);
        let dx=dot(second_bilinear(pixel+vec2<f32>(1.0,0.0)).rgb-second_bilinear(pixel-vec2<f32>(1.0,0.0)).rgb,l);
        let dy=dot(second_bilinear(pixel+vec2<f32>(0.0,1.0)).rgb-second_bilinear(pixel-vec2<f32>(0.0,1.0)).rgb,l);
        return studio_surface(uv,normalize(n+vec3<f32>(-dx,-dy,0.0)*p.extra.x*0.5),albedo);
    }
    if p.size_op.w==22u {
        // Holographic foil: a white mirror tinted by a rainbow that shifts with the
        // surface angle and position, and drifts with Reflection motion.
        let hue=fract(dot(n.xy,vec2<f32>(0.9,0.6))*1.5+(uv.x+uv.y)*0.15+p.values.w);
        let rainbow=vec3<f32>(0.55)+0.45*cos(6.2831853*(vec3<f32>(hue)+vec3<f32>(0.0,0.33,0.67)));
        return studio_surface(uv,n,vec3<f32>(0.0))*rainbow;
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
    if (map_flags()&2u)!=0u {
        // Roughness 0.5 reads the map as authored; lower is glossier, higher rougher.
        map_roughness=clamp(fourth[i].z*p.values.z*2.0,0.02,1.0);
    }
    let here=vec2<f32>(gid.xy);
    let n=surface_detail(here,normalize(vec3<f32>(slope*1.6,1.0)));
    var color=material_colour(uv,n);
    if p.extra.z>=0.0 && p.values.y>0.0 {
        let position=vec2<f32>(gid.xy);
        let filtered_normal=surface_detail(position,smooth_bevel_normal(position,p.values.y));
        color=material_colour(uv,filtered_normal);
        // Shade the subpixel footprint where the bevel bends. Keep the original
        // coverage, so antialiasing never expands the silhouette or adds a halo.
        if filtered_normal.z<0.9999 {
            color=vec3<f32>(0.0);
            var total=0.0;
            for(var y=0;y<2;y++) {for(var x=0;x<2;x++) {
                let offset=vec2<f32>(f32(x),f32(y))*0.5-vec2<f32>(0.25);
                let weight=bilinear(position+offset).a;
                color+=material_colour((position+offset)/p.values.x,surface_detail(position+offset,smooth_bevel_normal(position+offset,p.values.y)))*weight;
                total+=weight;
            }}
            color/=max(total,0.000001);
        }
    }
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
    // The background arrives already frosted (gaussian, reach = roughness).
    let center=second_bilinear(pos+displacement);
    var color=center.rgb;
    if p.extra.z>0.0 {
        color.r=second_bilinear(pos+displacement*(1.0+p.extra.z)).r;
        color.b=second_bilinear(pos+displacement*(1.0-p.extra.z)).b;
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
// One cooperative group per cell visits every source pixel, so narrow antialiased
// reflections are not lost to a random point test. Only this compact cell grid is read.
var<workgroup> glint_candidates:array<vec4<f32>,64>;
@compute @workgroup_size(64)
fn glint_seeds(@builtin(workgroup_id) group:vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let side=u32(ceil(max(8.,p.values.x*2.)));
    let origin=group.xy*side;
    let extent=min(vec2<u32>(side),p.size_op.xy-origin);
    var best=vec4<f32>(0.,0.,-1.,0.);
    let enabled=p.values.y>0. && glint_random(vec2<i32>(group.xy),31u)<p.extra.x;
    if enabled {
        for(var j=lane;j<extent.x*extent.y;j+=64u) {
            let pixel=origin+vec2<u32>(j%extent.x,j/extent.x);
            let hit=source[pixel.y*p.size_op.x+pixel.x];
            let luminance=dot(hit.rgb,vec3<f32>(0.2126,0.7152,0.0722));
            // Seed only breaks near ties; brightness determines the highlight.
            let score=luminance*(0.995+0.005*glint_random(vec2<i32>(pixel),79u));
            if hit.a>=0.1 && luminance>=p.values.z && score>best.z {
                best=vec4<f32>(vec2<f32>(pixel),score,hit.a);
            }
        }
    }
    glint_candidates[lane]=best;
    workgroupBarrier();
    for(var stride=32u;stride>0u;stride/=2u) {
        if lane<stride && glint_candidates[lane+stride].z>glint_candidates[lane].z {
            glint_candidates[lane]=glint_candidates[lane+stride];
        }
        workgroupBarrier();
    }
    if lane==0u {result[group.y*p.size_op.x+group.x]=glint_candidates[0];}
}
@compute @workgroup_size(8,8)
fn sun_glints(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let original=source[i];
    if p.values.y==0.0 || p.extra.x==0.0 {result[i]=original;return;}
    let cell_size=ceil(max(8.0,p.values.x*2.0));
    let cell=vec2<i32>(floor(vec2<f32>(gid.xy)/cell_size));
    var light=0.0;
    for(var y=-1;y<=1;y++) { for(var x=-1;x<=1;x++) {
        let candidate=cell+vec2<i32>(x,y);
        let cells=vec2<i32>(ceil(vec2<f32>(p.size_op.xy)/cell_size));
        if any(candidate<vec2<i32>(0)) || any(candidate>=cells) {continue;}
        let highlight=second[u32(candidate.y)*p.size_op.x+u32(candidate.x)];
        if highlight.z<0. {continue;}
        let point=highlight.xy;
        let pulse=pow(max(0.0,sin(p.values.w*1.570796327+glint_random(candidate,257u)*6.2831853)),8.0);
        let delta=(vec2<f32>(gid.xy)-point)/p.values.x;
        let c=cos(p.extra.y);let s=sin(p.extra.y);
        let q=abs(vec2<f32>(c*delta.x-s*delta.y,s*delta.x+c*delta.y));
        let taper=pow(max(0.0,1.0-length(q)),2.0);
        let spikes=(exp(-q.x*80.0)+exp(-q.y*80.0))*taper;
        let core=exp(-dot(q,q)*220.0);
        light+=(spikes+core)*pulse*highlight.w*p.values.y;
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
 // The coverage is kept over the text's own box: extra = (width, height, left, top).
 let w=u32(p.extra.x);let h=u32(p.extra.y);let ox=u32(p.extra.z);let oy=u32(p.extra.w);
 var coverage=0.0;
 if gid.x>=ox && gid.y>=oy && gid.x-ox<w && gid.y-oy<h {coverage=source[(gid.y-oy)*w+gid.x-ox].a;}
 let a=coverage*p.values.a;
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

// Separable Gaussian derivative evaluated continuously in two dimensions.
// Unlike integer cross taps, this does not quantize the bevel into pixel rings.
// The bevel is the gradient of a smooth height field: coverage blurred with a gaussian
// (sigma = 0.35 x bevel) in the bevel_height passes, read from `second`. A dense field
// keeps corners and acute vertices round instead of terraced or saw-toothed, with the
// same depth and strength as the former 5x5 binomial derivative at that spacing.
fn smooth_bevel_normal(pos:vec2<f32>,radius:f32)->vec3<f32> {
    let step=max(radius*0.35,0.5);
    let dx=second_bilinear(pos+vec2<f32>(1.0,0.0)).a-second_bilinear(pos-vec2<f32>(1.0,0.0)).a;
    let dy=second_bilinear(pos+vec2<f32>(0.0,1.0)).a-second_bilinear(pos-vec2<f32>(0.0,1.0)).a;
    let slope=-vec2<f32>(dx,dy)*2.0*step;
    return normalize(vec3<f32>(slope*1.6,1.0));
}
fn bevel_height_axis(gid:vec3<u32>,axis:vec2<i32>)->vec4<f32> {
    let sigma=max(p.values.y*0.35,0.5);let reach=i32(ceil(sigma*3.0));
    var total=0.0;var weights=0.0;
    for(var o=-reach;o<=reach;o++) {
        let q=vec2<i32>(gid.xy)+axis*o;let w=exp(-f32(o*o)/(2.0*sigma*sigma));
        total+=clamp(sample_source(q.x,q.y).a,0.0,1.0)*w;weights+=w;
    }
    return vec4<f32>(total/weights);
}
// Image texture: the texture's straight colour beside the bevel height (alpha), so a
// material reads both from one input.
@compute @workgroup_size(8,8)
fn texture_pack(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let t=second[i];
    result[i]=vec4<f32>(max(t.rgb/max(t.a,0.0001),vec3<f32>(0.0))*step(0.0001,t.a),source[i].a);
}
@compute @workgroup_size(8,8)
fn bevel_height_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bevel_height_axis(gid,vec2<i32>(1,0));
}
@compute @workgroup_size(8,8)
fn bevel_height_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bevel_height_axis(gid,vec2<i32>(0,1));
}
