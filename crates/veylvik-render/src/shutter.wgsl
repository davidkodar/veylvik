struct Sampling { count:u32, width:u32, first:u32, weight:f32 }
@group(0) @binding(0) var<storage,read> sample:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> accumulation:array<vec4<f32>>;
@group(0) @binding(2) var<uniform> sampling:Sampling;
@compute @workgroup_size(16,16)
fn accumulate(@builtin(global_invocation_id) id:vec3<u32>) {
    let index=id.y*sampling.width+id.x;
    if id.x>=sampling.width || index>=sampling.count {return;}
    let value=sample[index]*sampling.weight;
    if sampling.first!=0u {accumulation[index]=value;}
    else {accumulation[index]+=value;}
}
