struct Params { mode:u32, srgb:u32, intensity:f32, pad:u32 }
@group(0) @binding(0) var<storage,read> bins:array<u32>;
@group(0) @binding(1) var<uniform> params:Params;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) i:u32)->Vertex {
 let uv=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));
 return Vertex(vec4<f32>(uv*vec2<f32>(2.,-2.)+vec2<f32>(-1.,1.),0.,1.),uv);
}
fn density(index:u32,peak:u32)->f32 {
 let count=f32(bins[index]);let maximum=max(1.,f32(bins[82944u+peak]));
 if peak==0u {return clamp(sqrt(count/maximum)*params.intensity,0.,1.);}
 return clamp(pow(log(1.+count)/log(1.+maximum),0.4)*params.intensity,0.,1.);
}
@fragment fn fragment(v:Vertex)->@location(0) vec4<f32> {
 let uv=clamp(v.uv,vec2<f32>(0.),vec2<f32>(0.99999));
 let x=u32(uv.x*128.); let y=u32((1.-uv.y)*127.);
 var c=vec3<f32>(0.);
 if params.mode==0u {
  let h=u32(uv.x*256.); let height=1.-uv.y;
  c=vec3<f32>(select(0.,0.85,height<density(h,0u)),select(0.,0.85,height<density(256u+h,0u)),select(0.,0.85,height<density(512u+h,0u)));
 } else if params.mode==1u { c=vec3<f32>(0.64,0.91,0.83)*density(1024u+y*128u+x,2u); }
 else if params.mode==2u {
  let channel=min(2u,u32(uv.x*3.)); let px=u32(fract(uv.x*3.)*128.);
  let value=density(17408u+channel*16384u+y*128u+px,3u);
  c=select(select(vec3<f32>(0.3,0.95,0.4),vec3<f32>(0.4,0.6,1.),channel==2u),vec3<f32>(1.,0.35,0.35),channel==0u)*value;
 } else {
  var trace=0.;
  for(var dy=-1;dy<=1;dy++) {for(var dx=-1;dx<=1;dx++) {
   let px=u32(clamp(i32(x)+dx,0,127)); let py=u32(clamp(i32(y)+dy,0,127));
   trace=max(trace,density(66560u+py*128u+px,4u));
  }}
  c=vec3<f32>(0.72,0.87,1.)*trace;
 }
 c=max(c,vec3<f32>(0.025,0.035,0.045));
 if params.srgb!=0u { c=select(pow((c+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c/12.92,c<=vec3<f32>(0.04045)); }
 return vec4<f32>(c,1.);
}
