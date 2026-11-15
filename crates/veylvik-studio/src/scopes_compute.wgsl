struct Params { size: vec4<u32>, view: vec4<f32> }
@group(0) @binding(0) var<storage,read> pixels: array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> bins: array<atomic<u32>>;
@group(0) @binding(2) var<uniform> params: Params;
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

fn add(index:u32, peak:u32) { let count=atomicAdd(&bins[index],1u)+1u; atomicMax(&bins[82944u+peak],count); }
@compute @workgroup_size(8,8)
fn collect(@builtin(global_invocation_id) id:vec3<u32>) {
 let size=params.size.xy; let grid=min(size,vec2<u32>(512u));
 if any(id.xy>=grid) {return;}
 let xy=min(vec2<u32>((vec2<f32>(id.xy)+0.5)*vec2<f32>(size)/vec2<f32>(grid)),size-1u);
 let p=pixels[xy.y*size.x+xy.x];
 if !(p.a>0.000001) {return;}
 var rgb=p.rgb/p.a*exp2(params.view.x);
 if any(rgb!=rgb) || any(abs(rgb)>vec3<f32>(1e30)) {return;}
 if params.size.z!=0u {rgb=filmic(rgb);} else {rgb=clamp(rgb,vec3<f32>(0.),vec3<f32>(1.));}
 rgb=srgb(rgb);
 let lum=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
 let h=vec3<u32>(round(rgb*255.));
 add(h.r,0u); add(256u+h.g,0u); add(512u+h.b,0u); add(768u+u32(round(lum*255.)),1u);
 let x=min(127u,u32(f32(id.x)*128./f32(grid.x)));
 let y=u32(round(lum*127.)); add(1024u+y*128u+x,2u);
 let channels=vec3<u32>(round(rgb*127.));
 add(17408u+channels.r*128u+x,3u);
 add(33792u+channels.g*128u+x,3u);
 add(50176u+channels.b*128u+x,3u);
 let cb=(rgb.b-lum)/1.8556; let cr=(rgb.r-lum)/1.5748;
 let v=vec2<u32>(round(clamp(vec2<f32>(cb+0.5,cr+0.5),vec2<f32>(0.),vec2<f32>(1.))*127.));
 add(66560u+v.y*128u+v.x,4u);
}
