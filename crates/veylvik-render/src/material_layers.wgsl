// Layered material: Over on Under through the mask, worn away at Under's edges.
// fourth.a is Under's alpha blurred with sigma = wear width: 1 deep inside, about 0.5
// on a straight edge and lower at convex corners, so corners wear first.
@compute @workgroup_size(8,8)
fn material_layers(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid);
    var shown=clamp(p.values.x,0.0,1.0);
    if p.size_op.w==1u {shown*=clamp(third[i].a,0.0,1.0);}
    var under=source[i];
    let amount=clamp(p.values.z,0.0,1.0);
    if amount>0.0 && p.values.y>0.0 {
        // 1 on a straight edge, up to 1.5 at sharp corners, falling to 0 inside.
        let edge=clamp((1.0-fourth[i].a)*2.0,0.0,1.5);
        let q=vec3<f32>(vec2<f32>(gid.xy)/max(p.values.w,1.0),0.0);
        let n=fire_fbm(q,p.size_op.z)-0.5;
        // More wear pushes the worn zone inward; breakup lets noise carry chips
        // further in and roughens their borders.
        let field=edge*amount*0.95+n*clamp(p.extra.x,0.0,1.0)*1.8;
        let worn=smoothstep(0.5,0.53,field);
        // The paint's thickness shades the bare surface just inside a chip.
        let lip=smoothstep(0.5,0.53,field)*(1.0-smoothstep(0.53,0.6,field));
        under=vec4<f32>(under.rgb*(1.0-0.45*lip*shown),under.a);
        shown*=1.0-worn;
    }
    result[i]=mix(under,second[i],shown);
}
