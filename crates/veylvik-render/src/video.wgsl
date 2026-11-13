struct Params {
    size:vec4<u32>, // source width/height, format (0 rgba64, 1 nv12, 2 p010), rotation
    yuv:vec4<f32>, // kr, kb, full range, unused
    row0:vec4<f32>, row1:vec4<f32>, row2:vec4<f32>,
    chroma:vec4<f32>, // sample location in source luma coordinates
    output_size:vec4<u32>, // output width/height after rotation; equal to the source for 1:1
}
@group(0) @binding(0) var<storage,read> raw:array<u32>;
@group(0) @binding(1) var<storage,read> lut:array<f32>;
@group(0) @binding(2) var<storage,read_write> output:array<vec4<f32>>;
@group(0) @binding(3) var<uniform> p:Params;
fn byte_at(i:u32)->u32 {return (raw[i/4u]>>((i%4u)*8u))&255u;}
fn word_at(i:u32)->u32 {return byte_at(i)|byte_at(i+1u)<<8u;}
fn chroma_at(q:vec2<i32>)->vec2<f32> {
    let c=vec2<u32>(clamp(q,vec2<i32>(0),vec2<i32>(p.size.xy/2u)-vec2<i32>(1)));
    let i=p.size.x*p.size.y+c.y*p.size.x+c.x*2u;
    if p.size.z==1u {return vec2<f32>(f32(byte_at(i)),f32(byte_at(i+1u)));}
    return vec2<f32>(f32(word_at(i*2u)>>6u),f32(word_at(i*2u+2u)>>6u));
}
fn linear(v:f32)->f32 {return lut[u32(round(clamp(v,0.0,1.0)*65535.0))];}
// One rotated output pixel converted to scene-linear premultiplied RGBA.
fn pixel(o:vec2<u32>)->vec4<f32> {
    var q=o;
    if p.size.w==90u {q=vec2<u32>(p.size.x-1u-o.y,o.x);}
    if p.size.w==180u {q=p.size.xy-vec2<u32>(1u)-o;}
    if p.size.w==270u {q=vec2<u32>(o.y,p.size.y-1u-o.x);}
    let i=q.y*p.size.x+q.x;
    var rgb=vec3<f32>(0.0); var alpha=1.0;
    if p.size.z==0u {
        rgb=vec3<f32>(lut[word_at(i*8u)],lut[word_at(i*8u+2u)],lut[word_at(i*8u+4u)]);
        alpha=f32(word_at(i*8u+6u))/65535.0;
    } else {
        var y=f32(byte_at(i)); var depth=1.0;
        if p.size.z==2u {y=f32(word_at(i*2u)>>6u);depth=4.0;}
        let c=(vec2<f32>(q)-p.chroma.xy)*0.5;
        let base=vec2<i32>(floor(c)); let f=fract(c);
        var uv=mix(mix(chroma_at(base),chroma_at(base+vec2<i32>(1,0)),f.x),mix(chroma_at(base+vec2<i32>(0,1)),chroma_at(base+vec2<i32>(1,1)),f.x),f.y);
        if p.yuv.z==1.0 {y/=256.0*depth-1.0;uv=(uv-vec2<f32>(128.0*depth))/(256.0*depth-1.0);}
        else {y=(y-16.0*depth)/(219.0*depth);uv=(uv-vec2<f32>(128.0*depth))/(224.0*depth);}
        let kr=p.yuv.x; let kb=p.yuv.y; let kg=1.0-kr-kb;
        let r=y+2.0*(1.0-kr)*uv.y;
        let b=y+2.0*(1.0-kb)*uv.x;
        let g=y-2.0*kb*(1.0-kb)/kg*uv.x-2.0*kr*(1.0-kr)/kg*uv.y;
        rgb=vec3<f32>(linear(r),linear(g),linear(b));
    }
    return vec4<f32>(vec3<f32>(dot(p.row0.xyz,rgb),dot(p.row1.xyz,rgb),dot(p.row2.xyz,rgb))*alpha,alpha);
}
@compute @workgroup_size(8,8)
fn convert(@builtin(global_invocation_id) id:vec3<u32>) {
    var dims=p.size.xy;
    if p.size.w==90u || p.size.w==270u {dims=dims.yx;}
    let sized=p.output_size.xy;
    if any(id.xy>=sized) {return;}
    if all(sized==dims) {
        output[id.y*sized.x+id.x]=pixel(id.xy);
        return;
    }
    // Reduced output: area-average the source footprint in scene-linear light, which avoids
    // the aliasing of shrinking later with point or bilinear sampling.
    let ratio=vec2<f32>(dims)/vec2<f32>(sized);
    let lo=vec2<f32>(id.xy)*ratio; let hi=lo+ratio;
    var sum=vec4<f32>(0.0); var weight=0.0;
    for (var y=u32(floor(lo.y)); y<min(u32(ceil(hi.y)),dims.y); y++) {
        for (var x=u32(floor(lo.x)); x<min(u32(ceil(hi.x)),dims.x); x++) {
            let w=max(0.0,min(hi.x,f32(x+1u))-max(lo.x,f32(x)))*max(0.0,min(hi.y,f32(y+1u))-max(lo.y,f32(y)));
            sum+=pixel(vec2<u32>(x,y))*w; weight+=w;
        }
    }
    output[id.y*sized.x+id.x]=sum/weight;
}
