// Lens distortion: radial Brown-Conrady about the frame centre, radii normalised by half
// the diagonal; ideal radius r is seen at r (1 + k1 r^2 + k2 r^4). values = (k1, k2,
// redistort, zoom). The picture is read from zoom times the frame about its centre
// (1 = as is; less enlarges it so the bend leaves no empty rim). Removing samples the source where each ideal pixel was seen; putting
// it on inverts the radius by Newton. Pixels past the lens's fold stay transparent.
fn lens_factor(r2:f32)->f32 {return 1.0+p.values.x*r2+p.values.y*r2*r2;}
fn lens_slope(r2:f32)->f32 {return 1.0+3.0*p.values.x*r2+5.0*p.values.y*r2*r2;}
@compute @workgroup_size(8,8)
fn lens_distortion(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let size=vec2<f32>(p.size_op.xy);
    let radius=0.5*length(size);
    let offset=(vec2<f32>(gid.xy)+vec2<f32>(0.5)-0.5*size)*p.values.w;
    let r=length(offset)/radius;
    var scale=1.0;
    if p.values.z<0.5 {
        if lens_slope(r*r)<=0.0 {result[index(gid)]=vec4<f32>(0.0);return;}
        scale=lens_factor(r*r);
    } else if r>1e-6 {
        var ru=r;
        for(var k=0;k<12;k++) {
            let slope=lens_slope(ru*ru);
            if slope<=0.0 {result[index(gid)]=vec4<f32>(0.0);return;}
            ru-=(ru*lens_factor(ru*ru)-r)/slope;
        }
        if ru<0.0 || abs(ru*lens_factor(ru*ru)-r)>1e-4 {result[index(gid)]=vec4<f32>(0.0);return;}
        scale=ru/r;
    }
    result[index(gid)]=bilinear(0.5*size+offset*scale-vec2<f32>(0.5));
}
