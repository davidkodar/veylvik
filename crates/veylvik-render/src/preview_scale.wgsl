struct Size { width:u32, height:u32, out_width:u32, divisor:u32 }
@group(0) @binding(0) var<uniform> size:Size;
@group(0) @binding(1) var<storage,read> source:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read_write> pixels_out:array<vec4<f32>>;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    let out_height=(size.height+size.divisor-1u)/size.divisor;
    if id.x>=size.out_width || id.y>=out_height {return;}
    let first=id.xy*size.divisor;
    var sum=vec4<f32>(0.);var count=0u;
    for(var y=first.y;y<min(first.y+size.divisor,size.height);y++) {
        for(var x=first.x;x<min(first.x+size.divisor,size.width);x++) {
            sum+=source[y*size.width+x];count++;
        }
    }
    pixels_out[id.y*size.out_width+id.x]=sum/f32(count);
}
