struct Params { size:vec4<u32>, other:vec4<u32>, values:vec4<f32> }
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var<storage,read> first:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> second:array<vec4<f32>>;
@group(0) @binding(3) var<storage,read> guess:array<vec4<f32>>;
@group(0) @binding(4) var<storage,read_write> output:array<vec4<f32>>;
@group(0) @binding(5) var<storage,read> backward:array<vec4<f32>>;
@group(0) @binding(6) var<storage,read_write> stats:array<atomic<u32>>;
fn fetch(kind:u32,q:vec2<i32>,size:vec2<u32>)->vec4<f32> {
    let v=vec2<u32>(clamp(q,vec2<i32>(0),vec2<i32>(size)-vec2<i32>(1)));
    let i=v.y*size.x+v.x;
    switch kind {case 0u:{return first[i];}case 1u:{return second[i];}case 2u:{return guess[i];}default:{return backward[i];}}
}
fn sample_image(kind:u32,q:vec2<f32>,size:vec2<u32>)->vec4<f32> {
    let a=vec2<i32>(floor(q));let f=fract(q);
    return mix(mix(fetch(kind,a,size),fetch(kind,a+vec2<i32>(1,0),size),f.x),mix(fetch(kind,a+vec2<i32>(0,1),size),fetch(kind,a+vec2<i32>(1,1),size),f.x),f.y);
}
fn colour(kind:u32,q:vec2<f32>)->vec4<f32> {
    if any(q<vec2<f32>(0.)) || any(q>vec2<f32>(p.size.xy)-vec2<f32>(1.)) {return vec4<f32>(0.);}
    return sample_image(kind,q,p.size.xy);
}
fn signal(c:vec4<f32>)->f32 {
    let y=dot(clamp(c.rgb,vec3<f32>(-1e6),vec3<f32>(1e6)),vec3<f32>(0.2126,0.7152,0.0722));
    return y/(1.+abs(y))+0.15*c.a;
}
fn intensity(kind:u32,q:vec2<f32>)->f32 {return signal(sample_image(kind,q,p.size.zw));}
@compute @workgroup_size(8,8)
fn pyramid(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=p.size.xy) {return;}
    let q=(vec2<f32>(id.xy)+0.5)*vec2<f32>(p.size.zw)/vec2<f32>(p.size.xy)-0.5;
    let weights=array<f32,5>(1.,4.,6.,4.,1.);var c=vec4<f32>(0.);
    for(var y=-2;y<=2;y++){for(var x=-2;x<=2;x++){c+=sample_image(0u,q+vec2<f32>(f32(x),f32(y)),p.size.zw)*weights[u32(x+2)]*weights[u32(y+2)];}}
    output[id.y*p.size.x+id.x]=c/256.;
}
@compute @workgroup_size(8,8)
fn refine(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=p.size.xy) {return;}
    let q=vec2<f32>(id.xy);
    let ratio=vec2<f32>(p.size.xy)/vec2<f32>(p.other.zw);
    let initial=sample_image(2u,(q+0.5)/ratio-0.5,p.other.zw);
    var flow=initial.xy*ratio;
    var xx=0.;var xy=0.;var yy=0.;var bx=0.;var by=0.;var residual=0.;var weight=0.;
    for(var y=-2;y<=2;y++){for(var x=-2;x<=2;x++){
        let v=q+vec2<f32>(f32(x),f32(y));
        let a=intensity(0u,v);let b=intensity(1u,v+flow);
        let g=0.5*vec2<f32>(intensity(0u,v+vec2<f32>(1.,0.))-intensity(0u,v-vec2<f32>(1.,0.)),intensity(0u,v+vec2<f32>(0.,1.))-intensity(0u,v-vec2<f32>(0.,1.)));
        let e=a-b;let w=1./(1.+8.*abs(e));
        xx+=w*g.x*g.x;xy+=w*g.x*g.y;yy+=w*g.y*g.y;bx+=w*g.x*e;by+=w*g.y*e;residual+=w*abs(e);weight+=w;
    }}
    let determinant=xx*yy-xy*xy;
    let trace=xx+yy;
    if determinant>1e-8 {
        let delta=vec2<f32>(yy*bx-xy*by,xx*by-xy*bx)/determinant;
        flow+=clamp(delta,vec2<f32>(-2.),vec2<f32>(2.));
    }
    let eigen=0.5*(trace-sqrt(max(0.,trace*trace-4.*determinant)));
    let texture=smoothstep(0.0002,0.008,eigen);
    let error=residual/max(weight,1e-6);
    var confidence=max(texture,initial.z*0.98)*(1.-smoothstep(0.025,0.16,error));
    if any(q+flow<vec2<f32>(0.)) || any(q+flow>vec2<f32>(p.size.xy)-vec2<f32>(1.)) {confidence=0.;}
    output[id.y*p.size.x+id.x]=vec4<f32>(flow,confidence,error);
}
fn flow_at(kind:u32,q:vec2<f32>)->vec4<f32> {
    let ratio=vec2<f32>(p.size.xy)/vec2<f32>(p.other.zw);
    let f=sample_image(kind,(q+0.5)/ratio-0.5,p.other.zw);
    return vec4<f32>(f.xy*ratio,f.zw);
}
fn consistency(kind:u32,other:u32,q:vec2<f32>)->f32 {
    let f=flow_at(kind,q);let end=q+f.xy;
    if any(q<vec2<f32>(0.)) || any(q>=vec2<f32>(p.size.xy)) || any(end<vec2<f32>(0.)) || any(end>=vec2<f32>(p.size.xy)) {return 0.;}
    let reverse=flow_at(other,end);
    let tolerance=1.5+0.05*length(f.xy);
    return min(f.z,reverse.z)*(1.-smoothstep(tolerance,tolerance*2.,length(f.xy+reverse.xy)));
}
@compute @workgroup_size(8,8)
fn interpolate(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=p.size.xy) {return;}
    let q=vec2<f32>(id.xy);let t=p.values.x;
    var a=q;var b=q;
    for(var i=0;i<4;i++){a=q-t*flow_at(2u,a).xy;b=q-(1.-t)*flow_at(3u,b).xy;}
    let wa=consistency(2u,3u,a);let wb=consistency(3u,2u,b);
    let ca=colour(0u,a);let cb=colour(1u,b);
    let confidence=max(wa,wb);
    let nearest=select(first[id.y*p.size.x+id.x],second[id.y*p.size.x+id.x],t>=0.5);
    var c=nearest;
    let measure=(id.x&3u)==0u && (id.y&3u)==0u;
    if confidence>=p.values.y && p.values.w<0.5 {
        let aw=(1.-t)*wa;let bw=t*wb;
        c=(ca*aw+cb*bw)/max(aw+bw,1e-6);
        if measure {atomicAdd(&stats[0],1u);}
    } else {if measure {atomicAdd(&stats[1],1u);}}
    let difference=abs(signal(first[id.y*p.size.x+id.x])-signal(second[id.y*p.size.x+id.x]));
    if measure {atomicAdd(&stats[2],u32(clamp(difference,0.,1.)*16.));}
    if p.values.z>0.5 {c=vec4<f32>(1.-confidence,confidence,0.15,1.);}
    output[id.y*p.size.x+id.x]=c;
}
