// Deterministic stratified emitter sampling. Flat object/receiver approximation;
// no readback, random temporal noise, or invented light from environment highlights.
@compute @workgroup_size(8,8)
fn shadow(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) { return; }
    let point=vec2<f32>(gid.xy)-p.values.xy;
    var alpha=0.0;
    var weight=0.0;
    let count=select(1u,min(u32(studio.environment.y),8u),p.extra.y>0.5);
    for(var light_index=0u;light_index<count;light_index++) {
        var power=1.0;
        var light=studio.lights[light_index];
        if p.extra.y>0.5 {
            if p.extra.z>0.0 && light_index+1u!=u32(p.extra.z) {continue;}
            if light.center.w<=0.0 || light.center.z<=0.01 {continue;}
            power=light.center.w*dot(light.colour.rgb,vec3<f32>(0.2126,0.7152,0.0722));
            if power<=0.0 {continue;}
        }
        var coverage=0.0;
        for(var j=0u;j<32u;j++) {
            let u=(f32(j%8u)+0.5)/8.0*2.0-1.0;
            let v=(f32(j/8u)+0.5)/4.0*2.0-1.0;
            var offset=vec2<f32>(0.0);
            if p.extra.y>0.5 {
                let emitter=light.center.xyz+light.u.xyz*u+light.v.xyz*v;
                // Positive light X throws a shadow towards negative X.
                offset=-emitter.xy/max(emitter.z,0.1)*p.extra.x;
            }
            let theta=f32(j)*2.39996323;
            let radius=sqrt((f32(j)+0.5)/32.0)*p.values.z;
            let soft=vec2<f32>(cos(theta),sin(theta))*radius;
            coverage+=bilinear(point-offset+soft).a/32.0;
        }
        alpha+=coverage*power;
        weight+=power;
    }
    alpha=clamp(alpha/max(weight,0.000001)*p.values.w,0.0,1.0);
    result[index(gid)]=vec4<f32>(0.0,0.0,0.0,alpha);
}

// Contact shadow: occlusion where a layer meets the surface behind it. A ring of taps
// on the sharp alpha gives the dense contact; the blurred alpha (second) the soft
// falloff over the reach. Both are read from a point shifted towards the light, so
// the occlusion leans away from it.
@compute @workgroup_size(8,8)
fn contact_shadow(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    var direction=vec2<f32>(cos(p.extra.y),sin(p.extra.y));
    if p.extra.z>0.5 {
        // Lights' screen-plane positions weighted by power; shadows fall away from them.
        var toward=vec2<f32>(0.0);
        for(var i=0u;i<min(u32(studio.environment.y),8u);i++) {
            let light=studio.lights[i];
            let power=light.center.w*dot(light.colour.rgb,vec3<f32>(0.2126,0.7152,0.0722));
            if power>0.0 && length(light.center.xy)>0.0001 {toward+=normalize(light.center.xy)*power;}
        }
        if length(toward)>0.0001 {direction=-normalize(toward);}
    }
    let at=vec2<f32>(gid.xy)-direction*max(p.extra.x,0.0);
    let reach=max(p.values.z,0.0);
    var near=bilinear(at).a;
    let ring=max(reach*0.12,0.75);
    for(var j=0u;j<8u;j++) {
        let a=f32(j)*0.7853982;
        near+=bilinear(at+vec2<f32>(cos(a),sin(a))*ring).a;
    }
    near/=9.0;
    var far=near;
    if reach>0.5 {far=second_bilinear(at).a;}
    // Contact dominates at the edge; the falloff stays dense close in (a square root
    // lifts the gaussian's thin tail) and fades to nothing at the reach.
    let soft=sqrt(clamp(far*1.6,0.0,1.0));
    let occlusion=1.0-(1.0-clamp(near,0.0,1.0)*0.9)*(1.0-soft*0.7);
    result[index(gid)]=vec4<f32>(0.0,0.0,0.0,clamp(occlusion*p.values.w,0.0,1.0));
}
