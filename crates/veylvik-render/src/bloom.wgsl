// Bloom: light brighter than a threshold spreads as glow. values = (radius, intensity,
// threshold, knee), extra = (tint, streak). The bright part is blurred twice (a wide gaussian of
// the radius and a tight one of a fifth of it) and both are added to the image, so
// lights get a hot core and a long soft falloff. Alpha is left alone: the glow is
// added light, and shows over whatever the image is merged onto.
fn bloom_blur(gid:vec3<u32>,axis:vec2<i32>,radius:f32)->vec4<f32> {
    return blur_axis(gid,axis,i32(clamp(round(radius),1.0,256.0)),true,false);
}
@compute @workgroup_size(8,8)
fn bloom_bright(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let rgb=max(source[index(gid)].rgb,vec3<f32>(0.0));
    let l=dot(rgb,vec3<f32>(0.2126,0.7152,0.0722));
    // Soft knee: light starts to count a little below the threshold, without a hard edge.
    let k=max(p.values.w*p.values.z,1e-5);
    let soft=clamp(l-p.values.z+k,0.0,2.0*k);
    let amount=max(soft*soft/(4.0*k),l-p.values.z)/max(l,1e-5);
    result[index(gid)]=vec4<f32>(rgb*amount,0.0);
}
@compute @workgroup_size(8,8)
fn bloom_wide_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(1,0),p.values.x);
}
@compute @workgroup_size(8,8)
fn bloom_wide_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(0,1),p.values.x);
}
@compute @workgroup_size(8,8)
fn bloom_tight_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(1,0),p.values.x*0.2);
}
@compute @workgroup_size(8,8)
fn bloom_tight_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;} result[index(gid)]=bloom_blur(gid,vec2<i32>(0,1),p.values.x*0.2);
}
// The wide glow and the streak are smooth over many pixels, so they are worked out on a
// smaller grid, `bloom_step` pixels to a cell, and read back between cells. A wide
// gaussian of sigma s over cells of d pixels is the same gaussian widened by about d
// over sqrt(12) (the cell average) plus the reading between cells, so the step is kept
// to a twelfth of the radius or less: under one percent wider than the gaussian asked
// for. Small radii keep a step of one and are worked out pixel by pixel as before.
// The cells sit in the top left corner of the frame-sized buffers, a frame's row apart.
fn bloom_step(radius:f32)->u32 {return u32(clamp(floor(clamp(radius,1.0,256.0)/12.0),1.0,8.0));}
fn bloom_cells(d:u32)->vec2<u32> {return (p.size_op.xy+vec2<u32>(d-1u))/d;}
fn bloom_cell(x:u32,y:u32)->vec4<f32> {return source[y*p.size_op.x+x];}
// The gaussian of the wide blur, in cells: the same sigma (a third of the radius) as the
// pixel blur, divided by the step, over taps out to three sigma.
fn bloom_cell_blur(gid:vec3<u32>,axis:vec2<i32>,cells:vec2<u32>,sigma:f32)->vec4<f32> {
    let reach=i32(ceil(3.0*sigma));var total=vec4<f32>(0.0);var weights=0.0;
    for (var offset=-reach;offset<=reach;offset++) {
        let q=vec2<i32>(gid.xy)+axis*offset;
        let w=exp(-f32(offset*offset)/(2.0*sigma*sigma));weights+=w;
        // The outside of the frame is transparent, as for the pixel blur.
        if q.x>=0 && q.y>=0 && q.x<i32(cells.x) && q.y<i32(cells.y) {total+=bloom_cell(u32(q.x),u32(q.y))*w;}
    }
    return total/weights;
}
fn bloom_wide_sigma()->f32 {
    let radius=clamp(round(p.values.x),1.0,256.0);
    return max(radius/3.0,0.5)/f32(bloom_step(p.values.x));
}
// How much of a pixel goes to a cell along one axis: each pixel is shared between the
// two nearest cell centres by its distance to them (a tent a cell wide either side), so
// a light keeps its exact place, however it sits in its cell, and nothing is lost.
fn bloom_share(pixel:i32,cell:i32,d:u32)->f32 {
    let at=(f32(pixel)+0.5)/f32(d)-0.5;
    return max(1.0-abs(at-f32(cell)),0.0);
}
// The bright part shared out onto the cells (outside the frame counts as nothing).
@compute @workgroup_size(8,8)
fn bloom_wide_down(@builtin(global_invocation_id) gid:vec3<u32>) {
    let d=bloom_step(p.values.x);let cells=bloom_cells(d);
    if gid.x>=cells.x || gid.y>=cells.y {return;}
    // The pixels a cell shares in: its own and half a cell's worth either side.
    let first=vec2<i32>(gid.xy)*i32(d)-vec2<i32>(i32(d/2u));
    var total=vec4<f32>(0.0);
    for (var y=0;y<=2*i32(d);y++) {
        let wy=bloom_share(first.y+y,i32(gid.y),d);
        if wy==0.0 {continue;}
        for (var x=0;x<=2*i32(d);x++) {
            let w=wy*bloom_share(first.x+x,i32(gid.x),d);
            if w>0.0 {total+=sample_source(first.x+x,first.y+y)*w;}
        }
    }
    result[gid.y*p.size_op.x+gid.x]=total/f32(d*d);
}
@compute @workgroup_size(8,8)
fn bloom_wide_cells_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    let cells=bloom_cells(bloom_step(p.values.x));
    if gid.x>=cells.x || gid.y>=cells.y {return;}
    result[gid.y*p.size_op.x+gid.x]=bloom_cell_blur(gid,vec2<i32>(1,0),cells,bloom_wide_sigma());
}
@compute @workgroup_size(8,8)
fn bloom_wide_cells_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    let cells=bloom_cells(bloom_step(p.values.x));
    if gid.x>=cells.x || gid.y>=cells.y {return;}
    result[gid.y*p.size_op.x+gid.x]=bloom_cell_blur(gid,vec2<i32>(0,1),cells,bloom_wide_sigma());
}
// The streak is blurred sideways only, so its cells are a step wide and one pixel tall:
// its height (a twentieth of the radius) stays exact.
@compute @workgroup_size(8,8)
fn bloom_streak_down(@builtin(global_invocation_id) gid:vec3<u32>) {
    let d=bloom_step(p.values.x);let cells=vec2<u32>(bloom_cells(d).x,p.size_op.y);
    if gid.x>=cells.x || gid.y>=cells.y {return;}
    let first=i32(gid.x*d)-i32(d/2u);
    var total=vec4<f32>(0.0);
    for (var x=0;x<=2*i32(d);x++) {
        let w=bloom_share(first+x,i32(gid.x),d);
        if w>0.0 {total+=sample_source(first+x,i32(gid.y))*w;}
    }
    result[gid.y*p.size_op.x+gid.x]=total/f32(d);
}
@compute @workgroup_size(8,8)
fn bloom_streak_cells_horizontal(@builtin(global_invocation_id) gid:vec3<u32>) {
    let d=bloom_step(p.values.x);let cells=vec2<u32>(bloom_cells(d).x,p.size_op.y);
    if gid.x>=cells.x || gid.y>=cells.y {return;}
    result[gid.y*p.size_op.x+gid.x]=bloom_cell_blur(gid,vec2<i32>(1,0),cells,bloom_wide_sigma());
}
// The streak's height: the sideways blur given a twentieth of the radius, pixel by
// pixel. A lens streak is a soft band; a blur one row tall draws hard lines along
// bright horizontal edges and stripes where rows differ.
@compute @workgroup_size(8,8)
fn bloom_streak_cells_vertical(@builtin(global_invocation_id) gid:vec3<u32>) {
    let d=bloom_step(p.values.x);let cells=vec2<u32>(bloom_cells(d).x,p.size_op.y);
    if gid.x>=cells.x || gid.y>=cells.y {return;}
    result[gid.y*p.size_op.x+gid.x]=bloom_blur(gid,vec2<i32>(0,1),p.values.x*0.05);
}
// A cell grid read at a pixel: between the four nearest cell centres (two for the
// streak's, which are one pixel tall).
fn bloom_between(cells_of:u32,x:u32,y:u32,d:vec2<u32>,cells:vec2<u32>)->vec3<f32> {
    let at=(vec2<f32>(f32(x),f32(y))+0.5)/vec2<f32>(d)-0.5;
    let lo=clamp(vec2<i32>(floor(at)),vec2<i32>(0),vec2<i32>(cells)-vec2<i32>(1));
    let hi=min(lo+vec2<i32>(1),vec2<i32>(cells)-vec2<i32>(1));
    let f=clamp(at-vec2<f32>(lo),vec2<f32>(0.0),vec2<f32>(1.0));
    let w=p.size_op.x;
    var a:vec3<f32>;var b:vec3<f32>;var c:vec3<f32>;var e:vec3<f32>;
    if cells_of==0u {
        a=second[u32(lo.y)*w+u32(lo.x)].rgb;b=second[u32(lo.y)*w+u32(hi.x)].rgb;
        c=second[u32(hi.y)*w+u32(lo.x)].rgb;e=second[u32(hi.y)*w+u32(hi.x)].rgb;
    } else {
        a=fourth[u32(lo.y)*w+u32(lo.x)].rgb;b=fourth[u32(lo.y)*w+u32(hi.x)].rgb;
        c=fourth[u32(hi.y)*w+u32(lo.x)].rgb;e=fourth[u32(hi.y)*w+u32(hi.x)].rgb;
    }
    return mix(mix(a,b,f.x),mix(c,e,f.x),f.y);
}
// source = the image, second = the wide glow's cells, third = the tight glow (pixels),
// fourth = the streak's cells.
@compute @workgroup_size(8,8)
fn bloom_combine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let c=source[i];
    let d=bloom_step(p.values.x);let cells=bloom_cells(d);
    let wide=bloom_between(0u,gid.x,gid.y,vec2<u32>(d,d),cells);
    let streak=bloom_between(1u,gid.x,gid.y,vec2<u32>(d,1u),vec2<u32>(cells.x,p.size_op.y));
    let glow=(0.55*wide+0.45*third[i].rgb+p.extra.w*streak)*p.extra.rgb*p.values.y;
    result[i]=vec4<f32>(c.rgb+glow,c.a);
}
// Light wrap: source = the foreground, second = the background blurred by the radius,
// third = the foreground blurred by the radius. values = (radius, amount). The
// background's light is added just inside the foreground's edges, where its blurred
// alpha has fallen below its own; alpha and everything outside it stay.
@compute @workgroup_size(8,8)
fn light_wrap_combine(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);let c=source[i];
    let edge=clamp(c.a-third[i].a,0.0,1.0)*2.0;
    let light=max(second[i].rgb,vec3<f32>(0.0));
    result[i]=vec4<f32>(c.rgb+light*min(edge,c.a)*p.values.y,c.a);
}
// Zoom blur: values = (amount, x, y). Each pixel is the average of 32 samples along
// the line towards the point, over `amount` of its distance from it.
@compute @workgroup_size(8,8)
fn zoom_blur(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let at=vec2<f32>(gid.xy)+vec2<f32>(0.5);
    let away=at-p.values.yz;
    var sum=vec4<f32>(0.0);
    for(var k=0;k<32;k++) {
        let along=1.0-clamp(p.values.x,0.0,1.0)*(f32(k)+0.5)/32.0;
        sum+=bilinear(p.values.yz+away*along-vec2<f32>(0.5));
    }
    result[index(gid)]=sum/32.0;
}

// An Own size picture placed in the frame: source = the picture, values = (its width,
// its height, fit: 0 actual, 1 fit, 2 fill, 3 stretch). `scale` is frame pixels for
// each picture pixel; scaling down averages the picture pixels an output pixel covers
// (up to 8 by 8 samples).
fn own_texel(x:i32,y:i32)->vec4<f32> {
    let w=i32(p.values.x); let h=i32(p.values.y);
    return source[u32(clamp(y,0,h-1)*w+clamp(x,0,w-1))];
}
fn own_sample(q:vec2<f32>)->vec4<f32> {
    let b=q-vec2<f32>(0.5); let i=vec2<i32>(floor(b)); let f=fract(b);
    return mix(mix(own_texel(i.x,i.y),own_texel(i.x+1,i.y),f.x),
               mix(own_texel(i.x,i.y+1),own_texel(i.x+1,i.y+1),f.x),f.y);
}
fn own_placed(gid:vec3<u32>,scale:vec2<f32>)->vec4<f32> {
    let frame=vec2<f32>(p.size_op.xy);
    let q=(vec2<f32>(gid.xy)+vec2<f32>(0.5)-frame*0.5)/scale+p.values.xy*0.5;
    let span=1.0/scale;
    if any(q+span*0.5<=vec2<f32>(0.0)) || any(q-span*0.5>=p.values.xy) {return vec4<f32>(0.0);}
    let n=vec2<i32>(clamp(ceil(span),vec2<f32>(1.0),vec2<f32>(8.0)));
    var sum=vec4<f32>(0.0);
    for(var j=0;j<n.y;j++) { for(var i=0;i<n.x;i++) {
        let at=q+(vec2<f32>(f32(i),f32(j))+vec2<f32>(0.5))/vec2<f32>(n)*span-span*0.5;
        // Beyond the picture is empty, so its edge stays an edge.
        if all(at>=vec2<f32>(0.0)) && all(at<=p.values.xy) { sum+=own_sample(at); }
    } }
    return sum/f32(n.x*n.y);
}
@compute @workgroup_size(8,8)
fn own_size_fit(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let ratio=vec2<f32>(p.size_op.xy)/p.values.xy;
    var scale=vec2<f32>(1.0);
    if p.values.z>2.5 { scale=ratio; }
    else if p.values.z>1.5 { scale=vec2<f32>(max(ratio.x,ratio.y)); }
    else if p.values.z>0.5 { scale=vec2<f32>(min(ratio.x,ratio.y)); }
    result[gid.y*p.size_op.x+gid.x]=own_placed(gid,scale);
}
@compute @workgroup_size(8,8)
fn own_size_stretch(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    result[gid.y*p.size_op.x+gid.x]=own_placed(gid,vec2<f32>(p.size_op.xy)/p.values.xy);
}
