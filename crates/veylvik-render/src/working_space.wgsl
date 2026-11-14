// OCIO's linear Rec.709 D65 <-> ACEScg AP1 D60 matrices, including adaptation.
// Linear transforms apply directly to premultiplied RGB and retain HDR/negatives.
@compute @workgroup_size(8,8)
fn to_acescg(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) { return; }
    let c=source[index(gid)];
    let m=mat3x3<f32>(vec3<f32>(0.6130974024,0.07019372247,0.02061559288),vec3<f32>(0.3395231462,0.9163538791,0.1095697729),vec3<f32>(0.04737945141,0.01345239847,0.8698146342));
    result[index(gid)]=vec4<f32>(m*c.rgb,c.a);
}
@compute @workgroup_size(8,8)
fn from_acescg(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) { return; }
    let c=source[index(gid)];
    let m=mat3x3<f32>(vec3<f32>(1.705050993,-0.1302564175,-0.02400335680),vec3<f32>(-0.6217921207,1.140804737,-0.1289689761),vec3<f32>(-0.08325887200,-0.01054831907,1.152972333));
    result[index(gid)]=vec4<f32>(m*c.rgb,c.a);
}
