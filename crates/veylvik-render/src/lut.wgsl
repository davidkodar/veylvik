// LUT node (appended to kernels.wgsl). values: LUT width and height (set by the host from the
// asset, N² and N + 1), mix, and the packed input curve, input gamut, output curve and output
// gamut (4 bits each, ids in media::Curve / media::Gamut order). extra.x: the LUT's storage
// width. Auto (0) means Rec.709. Colour is encoded into the LUT's input space, looked up with
// trilinear interpolation, and decoded from its output space back to scene-linear Rec.709.
fn gamut_to_709(gamut:u32)->mat3x3<f32> {
    switch gamut {
        case 2u: { return mat3x3<f32>(vec3<f32>(1.660491002,-0.124550475,-0.018150763),vec3<f32>(-0.587641139,1.132899897,-0.100578898),vec3<f32>(-0.072849863,-0.008349423,1.118729661)); }
        case 3u: { return mat3x3<f32>(vec3<f32>(1.224940176,-0.042056955,-0.019637555),vec3<f32>(-0.224940176,1.042056955,-0.078636046),vec3<f32>(-0.000000000,0.000000000,1.098273600)); }
        case 4u: { return mat3x3<f32>(vec3<f32>(1.877915128,-0.176806981,-0.026201126),vec3<f32>(-0.794168761,1.350999621,-0.148422262),vec3<f32>(-0.083746367,-0.174192640,1.174623389)); }
        case 5u: { return mat3x3<f32>(vec3<f32>(1.626947410,-0.178515527,-0.044436115),vec3<f32>(-0.540138539,1.417940927,-0.195919966),vec3<f32>(-0.086808871,-0.239425400,1.240356081)); }
        case 6u: { return mat3x3<f32>(vec3<f32>(1.617523436,-0.070572741,-0.021101728),vec3<f32>(-0.537286622,1.334613062,-0.226953875),vec3<f32>(-0.080236814,-0.264040321,1.248055603)); }
        case 7u: { return mat3x3<f32>(vec3<f32>(1.893123443,-0.205700358,-0.012705743),vec3<f32>(-0.780881504,1.340257489,-0.152184876),vec3<f32>(-0.112241939,-0.134557131,1.164890619)); }
        case 8u: { return mat3x3<f32>(vec3<f32>(1.806575884,-0.170090343,-0.025205784),vec3<f32>(-0.695697274,1.305955216,-0.154468329),vec3<f32>(-0.110878610,-0.135864873,1.179674113)); }
        default: { return mat3x3<f32>(vec3<f32>(1.0,0.0,0.0),vec3<f32>(0.0,1.0,0.0),vec3<f32>(0.0,0.0,1.0)); }
    }
}
fn gamut_from_709(gamut:u32)->mat3x3<f32> {
    switch gamut {
        case 2u: { return mat3x3<f32>(vec3<f32>(0.627403896,0.069097289,0.016391439),vec3<f32>(0.329283038,0.919540395,0.088013308),vec3<f32>(0.043313066,0.011362316,0.895595253)); }
        case 3u: { return mat3x3<f32>(vec3<f32>(0.822461969,0.033194199,0.017082631),vec3<f32>(0.177538031,0.966805801,0.072397441),vec3<f32>(0.000000000,-0.000000000,0.910519929)); }
        case 4u: { return mat3x3<f32>(vec3<f32>(0.566049080,0.076961379,0.022350913),vec3<f32>(0.342763087,0.799054500,0.108612051),vec3<f32>(0.091187833,0.123984121,0.869037036)); }
        case 5u: { return mat3x3<f32>(vec3<f32>(0.645679478,0.087529992,0.036957420),vec3<f32>(0.259114547,0.759699563,0.129280905),vec3<f32>(0.095205975,0.152770446,0.833761675)); }
        case 6u: { return mat3x3<f32>(vec3<f32>(0.631321023,0.036819934,0.017369732),vec3<f32>(0.270800727,0.793036966,0.148789185),vec3<f32>(0.097878250,0.170143100,0.833841083)); }
        case 7u: { return mat3x3<f32>(vec3<f32>(0.565836540,0.088625637,0.017750040),vec3<f32>(0.340330999,0.809346842,0.109447621),vec3<f32>(0.093832461,0.102027521,0.872802339)); }
        case 8u: { return mat3x3<f32>(vec3<f32>(0.585196147,0.078588567,0.022794238),vec3<f32>(0.322641622,0.819627115,0.114217024),vec3<f32>(0.092162231,0.101784318,0.862988738)); }
        default: { return mat3x3<f32>(vec3<f32>(1.0,0.0,0.0),vec3<f32>(0.0,1.0,0.0),vec3<f32>(0.0,0.0,1.0)); }
    }
}
const HLG_SCALE:f32=7.6614598;
// Scene-linear to code value, the inverse of the decoder's curve for each id.
fn encode_curve(l:f32,curve:u32)->f32 {
    switch curve {
        case 2u: { if l<=0.0031308 {return 12.92*l;} return 1.055*pow(l,1.0/2.4)-0.055; }
        case 3u: { return l; }
        case 4u: {
            let y=pow(max(l/100.0,0.0),0.1593017578125);
            return pow((0.8359375+18.8515625*y)/(1.0+18.6875*y),78.84375);
        }
        case 5u: {
            let e=max(l,0.0)/HLG_SCALE;
            if e<=1.0/12.0 {return sqrt(3.0*e);}
            return 0.17883277*log(12.0*e-0.28466892)+0.55991073;
        }
        case 6u: {
            if l>=0.01 {return 0.08550479*log2(l+0.00964052)+0.69336945;}
            let d=max(l+0.05641088,0.0); return 47.28711236*d*d;
        }
        case 7u: {
            if l>=0.01125 {return (420.0+log((l+0.01)/0.19)/log(10.0)*261.5)/1023.0;}
            return (l*(171.2102946929-95.0)/0.01125+95.0)/1023.0;
        }
        case 8u: {
            if l>0.010591 {return 0.247190*log(5.555556*l+0.052272)/log(10.0)+0.385537;}
            return 5.367655*l+0.092809;
        }
        case 9u: {
            let a=(262144.0-16.0)/117.45; let b=(1023.0-95.0)/1023.0; let c=95.0/1023.0;
            let s=(7.0*log(2.0)*pow(2.0,7.0-14.0*c/b))/(a*b);
            let t=(pow(2.0,14.0*(-c/b)+6.0)-64.0)/a;
            if l>=t {return (log2(a*l+64.0)-6.0)/14.0*b+c;}
            return (l-t)/s;
        }
        case 10u: {
            if l<0.01 {return 5.6*l+0.125;}
            return 0.241514*log(l+0.00873)/log(10.0)+0.598206;
        }
        default: { if l<0.018 {return 4.5*l;} return 1.099*pow(l,0.45)-0.099; }
    }
}
// Code value to scene-linear for each id (the LUT's output side).
fn decode_curve(v:f32,curve:u32)->f32 {
    switch curve {
        case 2u: { if v<=0.04045 {return v/12.92;} return pow((v+0.055)/1.055,2.4); }
        case 3u: { return v; }
        case 4u: {
            let e=pow(max(v,0.0),1.0/78.84375);
            return pow(max(e-0.8359375,0.0)/(18.8515625-18.6875*e),1.0/0.1593017578125)*100.0;
        }
        case 5u: {
            if v<=0.5 {return v*v/3.0*HLG_SCALE;}
            return (exp((v-0.55991073)/0.17883277)+0.28466892)/12.0*HLG_SCALE;
        }
        case 6u: {
            let pt=47.28711236*(0.01+0.05641088)*(0.01+0.05641088);
            if v>=pt {return exp2((v-0.69336945)/0.08550479)-0.00964052;}
            return sqrt(max(v,0.0)/47.28711236)-0.05641088;
        }
        case 7u: {
            let code=v*1023.0;
            if code>=171.2102946929 {return pow(10.0,(code-420.0)/261.5)*0.19-0.01;}
            return (code-95.0)*0.01125/(171.2102946929-95.0);
        }
        case 8u: {
            if v>5.367655*0.010591+0.092809 {return (pow(10.0,(v-0.385537)/0.247190)-0.052272)/5.555556;}
            return (v-0.092809)/5.367655;
        }
        case 9u: {
            let a=(262144.0-16.0)/117.45; let b=(1023.0-95.0)/1023.0; let c=95.0/1023.0;
            return (pow(2.0,14.0*(v-c)/b+6.0)-64.0)/a;
        }
        case 10u: {
            if v<0.181 {return (v-0.125)/5.6;}
            return pow(10.0,(v-0.598206)/0.241514)-0.00873;
        }
        default: { if v<0.081 {return v/4.5;} return pow((v+0.099)/1.099,1.0/0.45); }
    }
}
fn lut_cell(x:u32,y:u32)->vec3<f32> {
    return second[y*u32(p.extra.x)+x].rgb;
}
@compute @workgroup_size(8,8)
fn lut(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !inside(gid) {return;}
    let i=index(gid); let c=source[i];
    if c.a<=0.0 {result[i]=c;return;}
    let n=u32(p.values.y)-1u;
    let packed=u32(p.values.w);
    let ic=packed&15u; let ig=(packed>>4u)&15u; let oc=(packed>>8u)&15u; let og=(packed>>12u)&15u;
    let linear=c.rgb/c.a;
    let encoded_gamut=gamut_from_709(ig)*linear;
    let code=vec3<f32>(encode_curve(encoded_gamut.x,ic),encode_curve(encoded_gamut.y,ic),encode_curve(encoded_gamut.z,ic));
    let low=lut_cell(0u,n); let high=lut_cell(1u,n);
    let at=clamp((code-low)/(high-low),vec3<f32>(0.0),vec3<f32>(1.0))*f32(n-1u);
    let i0=vec3<u32>(floor(at)); let i1=min(i0+vec3<u32>(1u),vec3<u32>(n-1u)); let f=fract(at);
    var looked=vec3<f32>(0.0);
    for (var k=0u;k<8u;k++) {
        let r=select(i0.x,i1.x,(k&1u)!=0u); let g=select(i0.y,i1.y,(k&2u)!=0u); let b=select(i0.z,i1.z,(k&4u)!=0u);
        let w=select(1.0-f.x,f.x,(k&1u)!=0u)*select(1.0-f.y,f.y,(k&2u)!=0u)*select(1.0-f.z,f.z,(k&4u)!=0u);
        looked+=lut_cell(r+n*g,b)*w;
    }
    let decoded=vec3<f32>(decode_curve(looked.x,oc),decode_curve(looked.y,oc),decode_curve(looked.z,oc));
    let back=gamut_to_709(og)*decoded;
    let rgb=mix(linear,back,p.values.z);
    result[i]=vec4<f32>(rgb*c.a,c.a);
}
