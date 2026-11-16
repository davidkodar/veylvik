// Prefiltered HDR studio lookups. Needs `environment_map: array<vec4<f32>>`
// (701096 texels, nine roughness levels). Directions use the scene frame: x right,
// y down, z towards the viewer. Shared by compute kernels and the 3D renderer.
fn environment_layer(direction:vec3<f32>,layer:u32)->vec3<f32> {
    let d=normalize(vec3<f32>(direction.x,-direction.y,direction.z));
    let uv=vec2<f32>(atan2(d.z,d.x)/6.2831853+0.5,acos(clamp(d.y,-1.0,1.0))/3.14159265);
    var size=vec2<u32>(1024u,512u)>>vec2<u32>(layer);
    if layer==9u {size=vec2<u32>(64u,32u);}
    var offset=0u;
    for(var i=0u;i<layer;i++) {let extent=vec2<u32>(1024u,512u)>>vec2<u32>(i);offset+=extent.x*extent.y;}
    let q=uv*vec2<f32>(size)-vec2<f32>(0.5);let lo=vec2<i32>(floor(q));let f=fract(q);
    let x0=u32((lo.x+i32(size.x))%i32(size.x));let x1=(x0+1u)%size.x;
    let y0=u32(clamp(lo.y,0,i32(size.y)-1));let y1=u32(clamp(lo.y+1,0,i32(size.y)-1));
    return mix(mix(environment_map[offset+y0*size.x+x0].rgb,environment_map[offset+y0*size.x+x1].rgb,f.x),
        mix(environment_map[offset+y1*size.x+x0].rgb,environment_map[offset+y1*size.x+x1].rgb,f.x),f.y);
}
fn environment_specular(direction:vec3<f32>,rough:f32)->vec3<f32> {
    let level=clamp(rough,0.0,1.0)*8.0;let lo=u32(floor(level));
    return mix(environment_layer(direction,lo),environment_layer(direction,min(lo+1u,8u)),fract(level));
}
