struct Params { mode:u32, srgb:u32, intensity:f32, cols:u32, width:u32,height:u32,samples:u32,pad:u32 }
@group(0) @binding(0) var<storage,read> bins:array<u32>;
@group(0) @binding(1) var<uniform> params:Params;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) i:u32)->Vertex {let uv=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));return Vertex(vec4<f32>(uv*vec2<f32>(2.,-2.)+vec2<f32>(-1.,1.),0.,1.),uv);}
fn vector_base()->u32 {return 4096u+params.cols*4096u;}
fn tint(ch:u32)->vec3<f32> {
 if ch==0u {return vec3<f32>(1.,0.16,0.12);}if ch==1u {return vec3<f32>(0.18,1.,0.22);}if ch==2u {return vec3<f32>(0.26,0.44,1.);}return vec3<f32>(0.86,0.93,0.90);
}
fn histogram(ch:u32,level:f32)->f32 {
 let base=vector_base()+4194304u;let inner=bins[base+4096u+ch];
 let peak=max(1u,select(bins[base+4100u+ch],inner,inner>0u));
 let x=clamp(level,0.,1023.);let a=u32(floor(x));let value=mix(f32(bins[base+ch*1024u+a]),f32(bins[base+ch*1024u+min(a+1u,1023u)]),fract(x));
 return min(1.,0.9*log(1.+value)/log(1.+f32(peak)));
}
@fragment fn fragment(v:Vertex)->@location(0) vec4<f32> {
 let uv=clamp(v.uv,vec2<f32>(0.),vec2<f32>(0.999999));var color=vec3<f32>(0.);
 let gain=pow(6.,params.intensity*2.-1.);
 if params.mode==0u {
  for(var ch=0u;ch<3u;ch++) {
   let h=histogram(ch,uv.x*1023.);let y=1.-uv.y;
   let edge=1.-smoothstep(0.,1.5/f32(params.height),abs(y-h));
   let fill=select(0.,0.16,y<h);color+=tint(ch)*max(edge,fill);
  }
 } else if params.mode==1u || params.mode==2u {
  let channel=select(min(2u,u32(uv.x*3.)),3u,params.mode==1u);
  let local=select(fract(uv.x*3.),uv.x,params.mode==1u);
  let panel_width=max(1.,f32(params.width)/select(3.,1.,params.mode==1u));
  let cx=local*f32(params.cols);let dx=f32(params.cols)/panel_width;
  let cy=(1.-uv.y)*1024.;let dy=1024./f32(params.height);
  let x0=u32(max(0.,floor(cx-dx*0.5)));let x1=min(params.cols,max(x0+1u,u32(ceil(cx+dx*0.5))));
  let y0=u32(max(0.,floor(cy-dy*0.5)));let y1=min(1024u,max(y0+1u,u32(ceil(cy+dy*0.5))));
  var sum=0.;
  for(var x=x0;x<x1;x++){for(var y=y0;y<y1;y++){sum+=f32(bins[4096u+(channel*params.cols+x)*1024u+y]);}}
  let area=max(1.,f32((x1-x0)*(y1-y0)));
  let norm=1024.*f32(params.cols)/f32(max(1u,params.samples));
  let density=sum/256.*norm/area;
  let value=1.-exp(-density*0.25*gain);
  color=mix(tint(channel),vec3<f32>(1.),value*value*0.6)*value;
 } else {
  let lo=vec2<f32>(uv.x,1.-uv.y)*2048.-vec2<f32>(1024./f32(params.width),1024./f32(params.height));
  let hi=lo+vec2<f32>(2048./f32(params.width),2048./f32(params.height));
  let tent=8.;let bin_start=vec2<u32>(max(floor(lo-tent),vec2<f32>(0.)));let to=vec2<u32>(min(ceil(hi+tent),vec2<f32>(2048.)));
  var sum=0.;
  for(var y=bin_start.y;y<to.y;y++){for(var x=bin_start.x;x<to.x;x++){
   let d=max(max(lo-(vec2<f32>(f32(x),f32(y))+0.5),(vec2<f32>(f32(x),f32(y))+0.5)-hi),vec2<f32>(0.));
   let weight=max(vec2<f32>(0.),vec2<f32>(1.)-d/tent);
   sum+=f32(bins[vector_base()+y*2048u+x])*weight.x*weight.y;
  }}
  let norm=4194304./f32(max(params.samples,1u));let area=(hi.x-lo.x+tent)*(hi.y-lo.y+tent);
  let value=clamp(log2(1.+sum/256.*norm/area*1.5*gain)/8.,0.,1.);
  color=mix(vec3<f32>(0.80,0.95,0.85),vec3<f32>(1.),value*value*0.5)*value;
 }
 color=clamp(color,vec3<f32>(0.),vec3<f32>(1.));
 if params.srgb!=0u {color=select(pow((color+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),color/12.92,color<=vec3<f32>(0.04045));}
 return vec4<f32>(color,1.);
}
