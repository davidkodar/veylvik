// Offline Vulkan GGX radiance prefilter and cosine-weighted diffuse convolution.
// Coordinates are conventional Y-up; the material evaluator converts its image Y-down normal.
@group(0) @binding(0) var env:texture_2d<f32>;
@group(0) @binding(1) var filtering:sampler;
@group(0) @binding(2) var<storage,read_write> output:array<vec4<f32>>;
@group(0) @binding(3) var<uniform> options:vec4<u32>;
fn finish_radiance(rgb:vec3<f32>)->vec4<f32> {
    if options.x!=0u {return vec4<f32>(vec3<f32>(dot(rgb,vec3<f32>(0.2126,0.7152,0.0722))),1.0);}
    return vec4<f32>(rgb,1.0);
}
const PI=3.14159265359;
fn layer_size(layer:u32)->vec2<u32> {
    if layer==9u {return vec2<u32>(64u,32u);}
    return vec2<u32>(1024u,512u)>>vec2<u32>(layer);
}
fn layer_offset(layer:u32)->u32 {
    var offset=0u;for(var i=0u;i<layer;i++){let size=layer_size(i);offset+=size.x*size.y;}return offset;
}
const SAMPLES=1024u;
fn sample_radiance(direction:vec3<f32>,pdf:f32)->vec3<f32> {
    let uv=vec2<f32>(atan2(direction.z,direction.x)/(2.0*PI)+0.5,acos(clamp(direction.y,-1.0,1.0))/PI);
    let size=vec2<f32>(textureDimensions(env));
    let texel_angle=2.0*PI*PI*max(0.001,sqrt(max(0.0,1.0-direction.y*direction.y)))/(size.x*size.y);
    let sample_angle=1.0/(f32(SAMPLES)*max(pdf,1e-6));
    let lod=max(0.0,0.5*log2(sample_angle/texel_angle));
    return textureSampleLevel(env,filtering,uv,lod).rgb;
}
@compute @workgroup_size(8,8)
fn prefilter(@builtin(global_invocation_id) gid:vec3<u32>) {
    if gid.z>=10u {return;}
    let size=layer_size(gid.z);
    if any(gid.xy>=size) {return;}
    let uv=(vec2<f32>(gid.xy)+vec2<f32>(0.5))/vec2<f32>(size);
    let theta=uv.y*PI;let phi=(uv.x-0.5)*2.0*PI;
    let n=vec3<f32>(sin(theta)*cos(phi),cos(theta),sin(theta)*sin(phi));
    let index=layer_offset(gid.z)+gid.y*size.x+gid.x;
    if gid.z==0u {output[index]=finish_radiance(textureSampleLevel(env,filtering,uv,0.0).rgb);return;}
    let helper=select(vec3<f32>(0.0,1.0,0.0),vec3<f32>(1.0,0.0,0.0),abs(n.y)>0.99);
    let t=normalize(cross(helper,n));let b=cross(n,t);
    let rough=f32(min(gid.z,8u))/8.0;let a=rough*rough;let a2=a*a;
    var colour=vec3<f32>(0.0);var weight=0.0;
    for(var i=0u;i<SAMPLES;i++) {
        let u=(f32(i)+0.5)/f32(SAMPLES);let v=f32(reverseBits(i))*2.3283064365386963e-10;
        let angle=2.0*PI*v;
        if gid.z==9u {
            let z=sqrt(1.0-u);let r=sqrt(u);
            let l=t*(cos(angle)*r)+b*(sin(angle)*r)+n*z;
            colour+=sample_radiance(l,z/PI);weight+=1.0;
        } else {
            let nh=sqrt((1.0-u)/(1.0+(a2-1.0)*u));let r=sqrt(max(0.0,1.0-nh*nh));
            let h=t*(cos(angle)*r)+b*(sin(angle)*r)+n*nh;
            let l=normalize(2.0*dot(n,h)*h-n);let nl=dot(n,l);
            if nl>0.0 {
                let denominator=nh*nh*(a2-1.0)+1.0;
                let pdf=a2/(4.0*PI*denominator*denominator);
                colour+=sample_radiance(l,pdf)*nl;weight+=nl;
            }
        }
    }
    output[index]=finish_radiance(colour/max(weight,1e-6));
}
