struct Settings {
    size: vec2<u32>, radius: u32, search: u32,
    h0: vec4<f32>, h1: vec4<f32>,
    quad0: vec4<f32>, quad1: vec4<f32>,
    // x: detection grid cells per side.
    grid: vec4<u32>,
}
@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var<storage, read> reference: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> current: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> scores: array<f32>;
@group(0) @binding(4) var<storage, read> features: array<vec4<f32>>;
// Occlusion matte for the frame being sampled (reference in detect, current in
// correlate). A one-element buffer means no matte; alpha above one half excludes.
@group(0) @binding(5) var<storage, read> occlusion: array<vec4<f32>>;
fn occluded(p: vec2<i32>) -> bool {
    if arrayLength(&occlusion) < settings.size.x*settings.size.y { return false; }
    let c = clamp(p, vec2<i32>(0), vec2<i32>(settings.size)-vec2<i32>(1));
    return occlusion[u32(c.y)*settings.size.x+u32(c.x)].a > 0.5;
}
fn luma(p: vec4<f32>) -> f32 { return dot(p.rgb,vec3<f32>(0.2126,0.7152,0.0722)); }
fn reference_luma(p: vec2<i32>) -> f32 { return luma(reference[u32(p.y)*settings.size.x+u32(p.x)]); }
fn cur(p: vec2<f32>) -> f32 {
    let a = vec2<u32>(floor(p)); let t = fract(p);
    return mix(mix(luma(current[a.y*settings.size.x+a.x]), luma(current[a.y*settings.size.x+a.x+1u]), t.x),
               mix(luma(current[(a.y+1u)*settings.size.x+a.x]), luma(current[(a.y+1u)*settings.size.x+a.x+1u]), t.x),t.y);
}
fn project(p: vec2<f32>) -> vec2<f32> {
    let a = settings.h0; let b = settings.h1;
    return vec2<f32>(a.x*p.x+a.y*p.y+a.z,a.w*p.x+b.x*p.y+b.y)/(b.z*p.x+b.w*p.y+1.0);
}
fn corner(i: u32) -> vec2<f32> {
    switch i { case 0u: { return settings.quad0.xy; } case 1u: { return settings.quad0.zw; }
    case 2u: { return settings.quad1.xy; } default: { return settings.quad1.zw; } }
}
fn inside_quad(p: vec2<f32>) -> bool {
    var positive = false; var negative = false;
    for (var i=0u; i<4u; i++) {
        let a = corner(i); let b = corner((i+1u)%4u);
        let d = (b.x-a.x)*(p.y-a.y)-(b.y-a.y)*(p.x-a.x);
        positive = positive || d > 0.0; negative = negative || d < 0.0;
    }
    return !(positive && negative);
}
// One strongest Shi-Tomasi corner per grid cell gives spatial coverage and
// bounds memory/work. No image readback and no dependence on text/materials.
@compute @workgroup_size(64)
fn detect(@builtin(global_invocation_id) id: vec3<u32>) {
    let cells = settings.grid.x;
    if id.x >= cells*cells { return; }
    var lo = min(min(corner(0u),corner(1u)),min(corner(2u),corner(3u)));
    var hi = max(max(corner(0u),corner(1u)),max(corner(2u),corner(3u)));
    let cell = (hi-lo)/f32(cells);
    let start = lo+vec2<f32>(f32(id.x%cells),f32(id.x/cells))*cell;
    let end = start+cell;
    let border = i32(settings.radius)+3;
    let lower = max(vec2<i32>(ceil(start)),vec2<i32>(border));
    let upper = min(vec2<i32>(floor(end)),vec2<i32>(settings.size)-vec2<i32>(border+1));
    var best = 0.0; var pos = vec2<i32>(0);
    for (var y=lower.y; y<=upper.y; y+=2) { for (var x=lower.x; x<=upper.x; x+=2) {
        let p=vec2<i32>(x,y);
        let pf=vec2<f32>(p); let r=f32(settings.radius);
        if !inside_quad(pf+vec2<f32>(-r,-r)) || !inside_quad(pf+vec2<f32>(r,-r)) ||
           !inside_quad(pf+vec2<f32>(r,r)) || !inside_quad(pf+vec2<f32>(-r,r)) { continue; }
        let ri=i32(settings.radius);
        if occluded(p) || occluded(p+vec2<i32>(-ri,-ri)) || occluded(p+vec2<i32>(ri,-ri)) ||
           occluded(p+vec2<i32>(ri,ri)) || occluded(p+vec2<i32>(-ri,ri)) { continue; }
        var xx=0.0; var yy=0.0; var xy=0.0;
        for (var dy=-2;dy<=2;dy++) { for (var dx=-2;dx<=2;dx++) {
            let q=p+vec2<i32>(dx,dy);
            let gx=(reference_luma(q+vec2<i32>(1,0))-reference_luma(q-vec2<i32>(1,0)))*0.5;
            let gy=(reference_luma(q+vec2<i32>(0,1))-reference_luma(q-vec2<i32>(0,1)))*0.5;
            xx+=gx*gx; yy+=gy*gy; xy+=gx*gy;
        }}
        let strength=0.5*(xx+yy-sqrt((xx-yy)*(xx-yy)+4.0*xy*xy));
        if strength>best { best=strength;pos=p; }
    }}
    scores[id.x*4u]=f32(pos.x); scores[id.x*4u+1u]=f32(pos.y);
    scores[id.x*4u+2u]=best; scores[id.x*4u+3u]=0.0;
}
// Warp each reference patch by the predicted plane before testing residual
// translations. This supports perspective/scale changes without replacing the
// reference texture each frame (which would accumulate template drift).
@compute @workgroup_size(64)
fn correlate(@builtin(global_invocation_id) id: vec3<u32>) {
    let side=settings.search*2u+1u;
    if id.x>=side*side || id.y>=arrayLength(&features) { return; }
    let output=id.y*side*side+id.x;
    let delta=vec2<f32>(f32(id.x%side),f32(id.x/side))-vec2<f32>(f32(settings.search));
    // zw: a per-feature offset (pixels) added to the plane's prediction.
    let origin=vec2<i32>(features[id.y].xy);
    var sa=0.0;var sb=0.0;var aa=0.0;var bb=0.0;var ab=0.0;
    let r=i32(settings.radius);
    for(var y=-r;y<=r;y++) { for(var x=-r;x<=r;x++) {
        let p=origin+vec2<i32>(x,y); let q=project(vec2<f32>(p))+delta+features[id.y].zw;
        if any(q<vec2<f32>(0.0)) || any(q>=vec2<f32>(settings.size)-vec2<f32>(1.0)) {
            scores[output]=-2.0;return;
        }
        // A patch touching the occlusion matte on the current frame never matches.
        if occluded(vec2<i32>(round(q))) { scores[output]=-2.0;return; }
        let a=reference_luma(p);let b=cur(q);sa+=a;sb+=b;aa+=a*a;bb+=b*b;ab+=a*b;
    }}
    let n=f32((2*r+1)*(2*r+1)); let va=max(0.0,aa-sa*sa/n);let vb=max(0.0,bb-sb*sb/n);
    if min(va,vb)<n*0.000001 { scores[output]=-2.0;return; }
    scores[output]=clamp((ab-sa*sb/n)/sqrt(va*vb),-1.0,1.0);
}
