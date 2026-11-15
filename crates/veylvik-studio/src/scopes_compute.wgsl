// Measurement conventions follow Klipp: 10-bit levels, fractional splats,
// source-column coverage and separate histogram/vector measurements.
struct Params { size: vec4<u32>, view: vec4<f32>, grid:vec4<u32> }
@group(0) @binding(0) var<storage,read> pixels:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> bins:array<atomic<u32>>;
@group(0) @binding(2) var<uniform> params:Params;
fn srgb(v: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), 12.92 * v, v <= vec3<f32>(0.0031308));
}
// ACES 1.0 RRT + sRGB ODT, Stephen Hill's fit; mirrors veylvik_render::display::filmic.
fn filmic(rgb: vec3<f32>) -> vec3<f32> {
    let input = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777));
    let output = mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602));
    let v = input * max(rgb, vec3<f32>(0.0));
    let t = (v * (v + 0.0245786) - 0.000090537) / (v * (0.983729 * v + 0.4329510) + 0.238081);
    return clamp(output * t, vec3<f32>(0.0), vec3<f32>(1.0));
}


fn vector_base()->u32 {return 4096u+params.grid.x*4096u;}
fn histogram_base()->u32 {return vector_base()+4194304u;}
fn encode(p:vec4<f32>)->vec3<f32> {
 var rgb=p.rgb/max(p.a,0.000001)*exp2(params.view.x);
 if params.size.z!=0u {rgb=filmic(rgb);} else {rgb=clamp(rgb,vec3<f32>(0.),vec3<f32>(1.));}
 return srgb(rgb);
}
fn sample_at(x:i32,y:i32)->vec4<f32> {return pixels[u32(clamp(y,0,i32(params.size.y)-1))*params.size.x+u32(clamp(x,0,i32(params.size.x)-1))];}
fn filtered(x:u32,y:u32)->vec3<f32> {
 if params.grid.z==0u {return encode(sample_at(i32(x),i32(y)));}
 let sigma=max(0.35,f32(max(params.size.x,params.size.y))/3840.);
 let radius=min(6,i32(ceil(2.*sigma)));var sum=vec3<f32>(0.);var total=0.;
 for(var dy=-radius;dy<=radius;dy++){for(var dx=-radius;dx<=radius;dx++){
  let p=sample_at(i32(x)+dx,i32(y)+dy);
  if p.a>0.000001 {let w=exp(-f32(dx*dx+dy*dy)/(2.*sigma*sigma));sum+=encode(p)*w;total+=w;}
 }}
 return sum/max(total,0.000001);
}
fn splat(base:u32,value:f32,weight:f32) {
 let level=clamp(value,0.,1.)*1023.;let lo=u32(floor(level));let f=fract(level);
 atomicAdd(&bins[base+lo],u32(round((1.-f)*weight)));
 atomicAdd(&bins[base+min(lo+1u,1023u)],u32(round(f*weight)));
}
@compute @workgroup_size(8,8)
fn collect(@builtin(global_invocation_id) id:vec3<u32>) {
 let xy=id.xy*params.grid.y;if any(xy>=params.size.xy) {return;}
 let p=sample_at(i32(xy.x),i32(xy.y));
 if !(p.a>0.000001) || any(p.rgb!=p.rgb) || any(abs(p.rgb)>vec3<f32>(1e30)) {return;}
 let rgb=encode(p);let lum=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
 splat(0u,rgb.r,256.);splat(1024u,rgb.g,256.);splat(2048u,rgb.b,256.);splat(3072u,lum,256.);
 let e=filtered(xy.x,xy.y);let values=vec4<f32>(e,dot(e,vec3<f32>(0.2126,0.7152,0.0722)));
 let scale=f32(params.grid.x)/f32(params.size.x);
 let lo=f32(xy.x)*scale;let hi=min(f32(xy.x+params.grid.y)*scale,f32(params.grid.x));
 for(var c=u32(floor(lo));c<min(u32(ceil(hi)),params.grid.x);c++) {
  let weight=256.*(min(f32(c+1u),hi)-max(f32(c),lo))/(hi-lo);
  for(var ch=0u;ch<4u;ch++) {splat(4096u+(ch*params.grid.x+c)*1024u,values[ch],weight);}
 }
 let v=clamp(vec2<f32>((rgb.b-lum)/1.8556+0.5,(rgb.r-lum)/1.5748+0.5),vec2<f32>(0.),vec2<f32>(1.))*2047.;
 let q=vec2<u32>(floor(v));let f=fract(v);
 for(var dy=0u;dy<2u;dy++){for(var dx=0u;dx<2u;dx++){
  let w=select(1.-f.x,f.x,dx==1u)*select(1.-f.y,f.y,dy==1u);
  atomicAdd(&bins[vector_base()+min(q.y+dy,2047u)*2048u+min(q.x+dx,2047u)],u32(round(w*256.)));
 }}
}
@compute @workgroup_size(1)
fn histogram() {
 let base=histogram_base();
 for(var ch=0u;ch<4u;ch++) {
  var inner=0u;var whole=0u;
  for(var level=0u;level<1024u;level++) {
   var value=f32(atomicLoad(&bins[ch*1024u+level]));
   for(var d=1u;d<4u;d++) {
    let w=1.-f32(d)*0.25;
    if level>=d {value+=f32(atomicLoad(&bins[ch*1024u+level-d]))*w;}
    if level+d<1024u {value+=f32(atomicLoad(&bins[ch*1024u+level+d]))*w;}
   }
   let count=u32(round(value));atomicStore(&bins[base+ch*1024u+level],count);
   whole=max(whole,count);if level>=10u && level<1014u {inner=max(inner,count);}
  }
  atomicStore(&bins[base+4096u+ch],inner);atomicStore(&bins[base+4100u+ch],whole);
 }
}
