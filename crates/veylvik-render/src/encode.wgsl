struct Params { size: vec4<u32>, matte: vec4<f32>, options: vec4<u32> }
@group(0) @binding(0) var<storage, read> pixels: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> packed: array<u32>;
@group(0) @binding(2) var<uniform> p: Params;
fn pixel(x:i32,y:i32)->vec4<f32> { let q=vec2<u32>(clamp(vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(p.size.xy)-1)); return pixels[q.y*p.size.x+q.x]; }
fn sample_at(q:vec2<f32>)->vec4<f32> {
 let lo=vec2<i32>(floor(q)); let f=fract(q);
 return mix(mix(pixel(lo.x,lo.y),pixel(lo.x+1,lo.y),f.x),mix(pixel(lo.x,lo.y+1),pixel(lo.x+1,lo.y+1),f.x),f.y);
}
fn rgb(x:u32,y:u32)->vec3<f32> {
 let scale=min(f32(p.size.z)/f32(p.size.x),f32(p.size.w)/f32(p.size.y));
 let offset=(vec2<f32>(p.size.zw)-vec2<f32>(p.size.xy)*scale)*0.5;
 let centre=(vec2<f32>(f32(x)+0.5,f32(y)+0.5)-offset)/scale;
 var c=vec4<f32>(0.0);
 if all(centre>=vec2<f32>(0.0)) && all(centre<vec2<f32>(p.size.xy)) {
  if scale>=1.0 { c=sample_at(centre-0.5); }
  else {
   let low=max(centre-vec2<f32>(0.5/scale),vec2<f32>(0.0));
   let high=min(centre+vec2<f32>(0.5/scale),vec2<f32>(p.size.xy));
   var sum=vec4<f32>(0.0);var weight=0.0;
   for(var sy=i32(floor(low.y));sy<i32(ceil(high.y));sy++) {
    for(var sx=i32(floor(low.x));sx<i32(ceil(high.x));sx++) {
     let w=max(0.0,min(high.x,f32(sx+1))-max(low.x,f32(sx)))*max(0.0,min(high.y,f32(sy+1))-max(low.y,f32(sy)));
     sum+=pixel(sx,sy)*w;weight+=w;
    }
   }
   c=sum/max(weight,0.00001);
  }
 }
 let linear=clamp(c.rgb+p.matte.rgb*(1.0-clamp(c.a,0.0,1.0)),vec3<f32>(0.0),vec3<f32>(1.0));
 return select(1.099*pow(linear,vec3<f32>(0.45))-0.099,4.5*linear,linear<vec3<f32>(0.018));
}
fn channel(i:u32)->u32 {
 let area=p.size.z*p.size.w; var value=0.0;var chroma=false;
 if i<area { let c=rgb(i%p.size.z,i/p.size.z);value=dot(c,vec3<f32>(0.2126,0.7152,0.0722)); }
 else {
  chroma=true;let uv=i-area;let cx=(uv/2u)%(p.size.z/2u);let cy=(uv/2u)/(p.size.z/2u);
  let c=(rgb(cx*2u,cy*2u)+rgb(cx*2u+1u,cy*2u)+rgb(cx*2u,cy*2u+1u)+rgb(cx*2u+1u,cy*2u+1u))*0.25;
  let l=dot(c,vec3<f32>(0.2126,0.7152,0.0722));value=select((c.b-l)/1.8556,(c.r-l)/1.5748,(uv%2u)==1u);
 }
 if p.options.x==10u { return u32(round(clamp(select(64.0+876.0*value,512.0+896.0*value,chroma),0.0,1023.0)))<<6u; }
 return u32(round(clamp(select(16.0+219.0*value,128.0+224.0*value,chroma),0.0,255.0)));
}
// Optional linear DMA-BUF layout includes row padding and a page-aligned UV plane.
fn stored_channel(i:u32)->u32 {
 if p.options.y==0u { return channel(i); }
 let pitch=p.options.y; let uv=p.options.z;
 if i<uv {
  let x=i%pitch;let y=i/pitch;
  if x>=p.size.z || y>=p.size.w { return 0u; }
  return channel(y*p.size.z+x);
 }
 let x=(i-uv)%pitch;let y=(i-uv)/pitch;
 if x>=p.size.z || y>=p.size.w/2u { return 0u; }
 return channel(p.size.z*p.size.w+y*p.size.z+x);
}
@compute @workgroup_size(64) fn encode(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
 let index=id.x+id.y*groups.x*64u;
 let per=select(4u,2u,p.options.x==10u);let count=select(p.size.z*p.size.w*3u/2u,p.options.w,p.options.y!=0u);
 if index*per>=count { return; }
 var value=0u;
 for(var n=0u;n<per;n++) { let i=index*per+n; if i<count { value|=stored_channel(i)<<(n*(32u/per)); } }
 packed[index]=value;
}
