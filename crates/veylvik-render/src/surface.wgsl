// What a material's surface is at a point (pattern, bump, grain, roughness), free of
// any binding: it reads only the material's `p` and `studio` blocks, which the 2D
// kernels bind as uniforms and the path tracer fills per hit. Shared by both, so
// a material looks the same lit either way.
struct Params { size_op: vec4<u32>, values: vec4<f32>, extra: vec4<f32> }
struct StudioLight { center:vec4<f32>, u:vec4<f32>, v:vec4<f32>, colour:vec4<f32> }
struct StudioLighting { optics:vec4<f32>, environment:vec4<f32>, lights:array<StudioLight,8>, palette:array<vec4<f32>,3>, maps:vec4<f32>, finish:vec4<f32> }
fn noise_hash(value:u32)->u32 {
    var v=value;v=(v^(v>>16u))*0x7feb352du;v=(v^(v>>15u))*0x846ca68bu;return v^(v>>16u);
}
fn lattice_noise(pos:vec3<i32>,seed:u32)->f32 {
    let bits=bitcast<vec3<u32>>(pos);
    let hash=noise_hash(bits.x ^ noise_hash(bits.y ^ noise_hash(bits.z ^ seed)));
    return f32(hash & 0x00ffffffu)/16777215.0;
}
// Periodic on the rising axis. Evolution can advance indefinitely without
// simulation history; four units move exactly one lattice period upwards.
fn fire_lattice(pos:vec3<i32>,seed:u32)->f32 {
    return lattice_noise(vec3<i32>(pos.x,((pos.y%8)+8)%8,pos.z),seed);
}
fn fire_noise(pos:vec3<f32>,seed:u32)->f32 {
    let b=vec3<i32>(floor(pos)); let f=fract(pos); let t=f*f*(3.0-2.0*f);
    let a=mix(fire_lattice(b,seed),fire_lattice(b+vec3<i32>(1,0,0),seed),t.x);
    let c=mix(fire_lattice(b+vec3<i32>(0,1,0),seed),fire_lattice(b+vec3<i32>(1,1,0),seed),t.x);
    let d=mix(fire_lattice(b+vec3<i32>(0,0,1),seed),fire_lattice(b+vec3<i32>(1,0,1),seed),t.x);
    let e=mix(fire_lattice(b+vec3<i32>(0,1,1),seed),fire_lattice(b+vec3<i32>(1,1,1),seed),t.x);
    return mix(mix(a,c,t.y),mix(d,e,t.y),t.z);
}
fn fire_fbm(pos:vec3<f32>,seed:u32)->f32 {
    return fire_noise(pos,seed)*0.48 + fire_noise(pos*2.0,seed+17u)*0.28
        + fire_noise(pos*4.0,seed+37u)*0.16 + fire_noise(pos*8.0,seed+53u)*0.08;
}
fn surface_sample(uv:vec2<f32>, kind:u32)->vec4<f32> {
    let coarse=fire_fbm(vec3<f32>(uv*3.0,0.7),719u);
    let fine=fire_noise(vec3<f32>(uv*110.0,1.2),311u);
    var color=vec3<f32>(0.5); var height=0.0;
    if kind==6u || kind==7u {
        let warp=fire_fbm(vec3<f32>(uv.x*1.2,uv.y*0.28,0.2),173u);
        let rings=0.5+0.5*sin(length(vec2<f32>(uv.x*0.7-1.9,uv.y*0.24+0.4))*65.0+warp*9.0);
        let fiber=fire_noise(vec3<f32>(uv.x*180.0,uv.y*18.0,0.1),137u);
        let pores=pow(max(0.0,1.0-fiber*1.8),3.0);
        let latewood=pow(rings,9.0);
        let tone=clamp(0.78-0.32*latewood-0.25*pores+(coarse-0.5)*0.5,0.0,1.0);
        color=mix(studio.palette[0].rgb,studio.palette[1].rgb,tone);
        height=0.12*latewood-0.25*pores+fiber*0.03;
    } else if kind==8u {
        let warp=vec2<f32>(coarse,fire_fbm(vec3<f32>(uv*2.2,5.1),193u));
        let veins=abs(sin((uv.x+uv.y*0.6+warp.x*1.7)*8.0+warp.y*7.0));
        let vein=1.0-smoothstep(0.015,0.14,veins);
        let branches=1.0-smoothstep(0.015,0.05,abs(sin((uv.y-uv.x+warp.y)*21.0)));
        color=mix(studio.palette[0].rgb,studio.palette[1].rgb,vein*0.72+branches*0.09);
        color*=0.88+coarse*0.18; height=fine*0.005;
    } else if kind==9u || kind==10u {
        let row=floor(uv.y*3.0); let q=vec2<f32>(uv.x*1.5+fract(row*0.5),uv.y*3.0);
        let cell=floor(q); let f=fract(q);
        let edge=min(min(f.x,1.0-f.x)*2.0,min(f.y,1.0-f.y));
        let brick=smoothstep(0.035,0.075,edge+(fine-0.5)*0.012);
        let variation=fire_noise(vec3<f32>(cell*7.0,0.5),173u);
        var base=mix(studio.palette[1].rgb,studio.palette[2].rgb,variation);
        base*=0.72+0.5*coarse+0.16*(fine-0.5);
        color=mix(studio.palette[0].rgb*(0.85+fine*0.25),base,brick);
        height=brick*0.5+brick*(coarse*0.12+fine*0.04);
    } else if kind==11u {
        let aggregate=fire_noise(vec3<f32>(uv*38.0,0.3),937u);
        let pits=1.0-smoothstep(0.19,0.30,aggregate);
        color=studio.palette[0].rgb*(0.7+coarse*0.5+fine*0.12-pits*0.38);
        height=coarse*0.45+aggregate*0.09-pits*0.18;
    } else if kind==12u || kind==13u {
        let fibers=fire_noise(vec3<f32>(uv.x*160.0,uv.y*35.0,0.3),811u);
        color=studio.palette[0].rgb*(0.96+fibers*0.06);
        height=fibers*0.018+fine*0.009;
        if kind==13u {
            let stain=smoothstep(0.43,0.72,coarse);
            let crease=exp(-pow(sin(uv.x*4.2+sin(uv.y*1.8)*0.45)*45.0,2.0));
            color=mix(studio.palette[0].rgb,studio.palette[1].rgb,stain*0.65);
            color*=0.94+fibers*0.1-crease*0.22;
            height+=coarse*0.12-crease*0.055;
        }
    } else if kind==19u {
        // Moulded plastic: one colour, a faint orange-peel texture under the gloss.
        color=studio.palette[0].rgb;
        height=fine*0.003;
    } else if kind==20u {
        // Car paint: base coat with metallic flakes that each tilt the clear coat a
        // little, so highlights break into sparkle.
        let q=uv*180.0;let cell=floor(q);
        let flake=fire_noise(vec3<f32>(cell*3.1,0.7),557u);
        let lit=smoothstep(0.78,0.86,flake);
        color=mix(studio.palette[0].rgb,studio.palette[1].rgb,lit*0.18);
        height=lit*(fire_noise(vec3<f32>(cell*7.3,2.1),941u)-0.5)*0.006;
    } else if kind==21u {
        // 2x2 twill: tows alternate direction, so the sheen colour follows the weave.
        let q=uv*10.0;let cell=floor(q);let f=fract(q);
        let warp=(i32(cell.x)+i32(cell.y))%2==0;
        let along=select(f.y,f.x,warp);
        let across=select(f.x,f.y,warp);
        let tow=pow(sin(across*3.14159265),0.6);
        let fibres=fire_noise(vec3<f32>(select(vec2<f32>(q.x*40.0,q.y*2.0),vec2<f32>(q.x*2.0,q.y*40.0),warp),0.4),313u);
        color=mix(studio.palette[0].rgb,studio.palette[1].rgb,tow*(0.55+0.45*fibres)*select(0.6,1.0,warp));
        height=tow*0.02+(fibres-0.5)*0.002+along*0.0;
    } else if kind==25u {
        // Liquid metal: broad, slow swells that ripple as Reflection motion animates.
        let t=p.values.w*6.2831853;
        // One smooth noise layer: broad molten blobs, no crinkle.
        let swell=fire_noise(vec3<f32>(uv*2.6+vec2<f32>(sin(t),cos(t))*0.25,t*0.3),881u);
        color=vec3<f32>(0.0);
        height=swell*0.9;
    } else if kind==26u {
        color=studio.palette[0].rgb*(0.97+fine*0.04);
        height=fine*0.002;
    } else if kind==27u {
        // Slate: thin cleft layers along one direction, broken by coarse noise.
        let layer=fire_fbm(vec3<f32>(uv.x*0.6,uv.y*9.0+coarse*2.0,0.9),421u);
        let flake=smoothstep(0.35,0.75,layer);
        color=mix(studio.palette[0].rgb,studio.palette[1].rgb,flake)*(0.9+fine*0.2);
        height=flake*0.08+coarse*0.05+fine*0.01;
    } else if kind==28u {
        // Plain weave: warp and weft threads pass over and under alternately.
        // 2/1 twill: each row shifts by one thread, so the weave runs diagonally.
        let q=uv*48.0;let cell=floor(q);let f=fract(q);
        let warp=((i32(cell.x)-i32(cell.y))%3+3)%3!=0;
        let across=select(f.y,f.x,warp);
        let thread=pow(sin(across*3.14159265),1.2);
        let fibres=fire_noise(vec3<f32>(select(vec2<f32>(q.x*12.0,q.y),vec2<f32>(q.x,q.y*12.0),warp),0.2),977u);
        color=select(studio.palette[1].rgb,studio.palette[0].rgb,warp)*(0.35+0.65*thread)*(0.8+0.4*fibres);
        height=thread*0.035+(fibres-0.5)*0.006;
    } else {
        // Jittered Voronoi cells produce irregular leather pebbles rather than a grid.
        let q=uv*78.0;let cell=floor(q);let f=fract(q);
        var first=10.0; var second=10.0;
        for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {
            let offset=vec2<f32>(f32(x),f32(y));let c=cell+offset;
            let jitter=vec2<f32>(fire_noise(vec3<f32>(c*17.0,0.4),19u),fire_noise(vec3<f32>(c*17.0,5.2),83u));
            let distance=length(offset+jitter-f);
            if distance<first {second=first;first=distance;} else {second=min(second,distance);}
        }}
        let groove=1.0-smoothstep(0.015,0.13,second-first);
        let base=studio.palette[0].rgb;
        color=base*(0.8+coarse*0.36-groove*0.22+fine*0.08);
        height=(1.0-groove)*0.018+coarse*0.035+fine*0.004;
    }
    return vec4<f32>(color,height);
}
var<private> map_roughness:f32=-1.0;
fn surface_roughness()->f32 {
    if map_roughness>=0.0 {return map_roughness;}
    return p.values.z;
}
fn map_flags()->u32 {return u32(studio.maps.z);}
// Smooth noise in 0..1 over the plate, for the finish.
fn plate_noise(q:vec2<f32>)->f32 {return fire_noise(vec3<f32>(q,0.0),907u);}
// What a material's surface is at a point, before any light: its normal tilted by
// the material's bump, brushing and finish, its roughness, colour and how wear
// brightens or dulls its reflections there. The 2D studio lights it in
// `studio_surface`; the path tracer lights the same point itself.
struct SurfacePoint { normal:vec3<f32>, rough:f32, albedo:vec3<f32>, gain:f32, across:vec3<f32>, turn:vec2<f32> }
fn surface_point(uv:vec2<f32>, normal:vec3<f32>,albedo:vec3<f32>)->SurfacePoint {
    let kind=p.size_op.w;
    let texture_position=vec3<f32>(uv,0.0);
    let fine=fire_noise(texture_position*140.0+vec3<f32>(0.4,1.3,2.7),71u)-0.5;
    let fine_y=fire_noise(texture_position*140.0+vec3<f32>(4.1,9.2,0.7),191u)-0.5;
    let texture=fire_fbm(texture_position*19.0+vec3<f32>(1.0),257u);
    var bump=vec3<f32>(fine,fine_y,0.0)*0.012*p.extra.x;
    let grain_turn=vec2<f32>(cos(studio.finish.z),sin(studio.finish.z));
    let grain_across=vec3<f32>(-grain_turn.y,grain_turn.x,0.0);
    if p.extra.w>0.5 {
        // The grain runs along x, turned by the finish's angle.
        let turned=vec2<f32>(uv.x*grain_turn.x+uv.y*grain_turn.y,uv.y*grain_turn.x-uv.x*grain_turn.y);
        let brush=fire_noise(vec3<f32>(turned.x*3.0,turned.y*min(500.0,p.values.x*0.65),0.0),71u)-0.5;
        // Brush grooves vary across the grain, not along it. Project the
        // slope onto the surface so the engraving wraps around the sphere.
        let across=normalize(grain_across-normal*dot(normal,grain_across)+vec3<f32>(0.0,0.0,0.00001));
        bump=across*brush*0.22*p.extra.x;
    }
    var n=normalize(normal+bump);
    // Finish: a worked plate undulates in long shallow ridges along the grain, which
    // tip parts of it towards the light and parts away (the soft bands on real metal);
    // wear leaves it not quite flat, unevenly polished and faintly scratched.
    let ridged=studio.finish.x;let worn=studio.finish.y;
    var plate_polish=0.0;var plate_gain=1.0;
    if ridged>0.0 || worn>0.0 {
        // Sized like the material's other textures: by its Scale, with the patterns
        // drawn for the default of 180 pixels (where uv x 180 is the pixel itself).
        let pixel=uv*180.0;
        let q=vec2<f32>(pixel.x*grain_turn.x+pixel.y*grain_turn.y,pixel.y*grain_turn.x-pixel.x*grain_turn.y);
        let over=normalize(grain_across-n*dot(n,grain_across)+vec3<f32>(0.0,0.0,0.00001));
        let run=normalize(cross(over,n));
        let ridge=(plate_noise(vec2<f32>(q.x*0.0026,q.y*0.0105))-0.5)+0.4*(plate_noise(vec2<f32>(q.x*0.006+13.0,q.y*0.028+5.0))-0.5);
        let wave=vec2<f32>(plate_noise(q*0.012+vec2<f32>(21.0,5.0))-0.5,plate_noise(q*0.012+vec2<f32>(4.0,31.0))-0.5);
        n=normalize(n+over*ridge*0.22*ridged+(run*wave.x+over*wave.y)*0.025*worn);
        if worn>0.0 {
            plate_polish=((plate_noise(q*0.006+vec2<f32>(3.0,9.0))-0.5)+(plate_noise(q*0.019+vec2<f32>(11.0,2.0))-0.5)*0.6
                +(plate_noise(q*0.07+vec2<f32>(7.0,4.0))-0.5)*0.3+(plate_noise(q*0.9+vec2<f32>(1.0,6.0))-0.5)*0.1)*1.1*worn;
            // Sparse fine scratches a little off the grain, some bright and some dull.
            var bright=0.0;var dull=0.0;
            for(var k=0;k<3;k++) {
                let turn=(f32(k)-1.0)*0.22;let tc=cos(turn);let ts=sin(turn);
                let r=vec2<f32>(q.x*tc-q.y*ts,q.x*ts+q.y*tc);let o=f32(k)*37.0;
                bright+=smoothstep(0.62,0.95,plate_noise(vec2<f32>(r.x*0.014+o,r.y*0.7+o)))*smoothstep(0.35,0.6,plate_noise(r*0.03+vec2<f32>(o,5.0)));
                dull+=smoothstep(0.62,0.95,plate_noise(vec2<f32>(r.x*0.012+o+50.0,r.y*0.55+o)))*smoothstep(0.35,0.6,plate_noise(r*0.025+vec2<f32>(9.0,o)));
            }
            plate_gain=max(1.0+worn*(0.3*bright-0.16*dull),0.0);
        }
    }
    let rough=clamp((surface_roughness()+(texture-0.5)*p.extra.x*select(select(0.06,0.14,kind==3u),0.03,p.extra.w>0.5))*(1.0+plate_polish),0.03,1.0);
    return SurfacePoint(n,rough,albedo,plate_gain,grain_across,grain_turn);
}
// Wood: rings warped round a knot, with dark pores along the grain.
fn wood_colour(uv:vec2<f32>)->vec3<f32> {
    var color=vec3<f32>(0.0);
        let warp=fire_fbm(vec3<f32>(uv.x*0.7,uv.y*2.0,0.5),617u);
        let knot=length((uv-vec2<f32>(2.7,1.5))*vec2<f32>(0.7,2.3));
        let rings=0.5+0.5*sin(uv.y*80.0+warp*50.0+exp(-knot*1.4)*25.0);
        let pores=fire_noise(vec3<f32>(uv.x*7.0,uv.y*220.0,0.0),313u);
        let grain=clamp(mix(0.5,0.75-0.55*pow(rings,6.0)+0.12*pores,p.extra.x),0.0,1.0);
        color=mix(studio.palette[0].rgb,studio.palette[1].rgb,grain);
        let fibers=fire_noise(vec3<f32>(uv.x*5.0,uv.y*180.0+warp*15.0,0.0),733u);
        let pores_dark=pow(max(0.0,1.0-fibers*2.0),3.0);
        color*=max(0.1,1.0+p.extra.x*((fibers-0.5)*0.45-pores_dark*0.6));
    return color;
}
// A patterned material's colour at a point and its normal tilted by the pattern's relief.
struct Bumped { normal:vec3<f32>, albedo:vec3<f32> }
fn bumped(uv:vec2<f32>, n:vec3<f32>, kind:u32)->Bumped {
    let sample=surface_sample(uv,kind);
    // Finite differences in pixel space avoid resolution-dependent bump strength.
    let e=max(0.0005,1.0/p.values.x);
    let dx=surface_sample(uv+vec2<f32>(e,0.0),kind).a-surface_sample(uv-vec2<f32>(e,0.0),kind).a;
    let dy=surface_sample(uv+vec2<f32>(0.0,e),kind).a-surface_sample(uv-vec2<f32>(0.0,e),kind).a;
    let normal=normalize(n+vec3<f32>(-dx,-dy,0.0)*p.extra.x/e*select(0.18,0.035,kind==6u || kind==7u));
    return Bumped(normal,sample.rgb);
}
// A library material at a point as light meets it: what `material_colour` lights in
// the 2D studio, for a renderer that lights it physically (the path tracer). `n` is
// in the material's shading frame (+z towards the viewer). Glass is flagged for the
// renderer to refract; neon glows by itself. An image texture keeps only its base here
// (its picture is not at hand): the renderer puts the picture's colour in palette[0].
struct MaterialPoint { normal:vec3<f32>, rough:f32, albedo:vec3<f32>, metal:f32, f0:vec3<f32>, gain:f32, emission:vec3<f32>, glass:f32, across:vec3<f32>, brushed:f32 }
fn material_point(uv:vec2<f32>, n:vec3<f32>)->MaterialPoint {
    let kind=p.size_op.w;
    var albedo=vec3<f32>(0.0);
    var normal=n;
    var emission=vec3<f32>(0.0);
    var f0=studio.optics.rgb;
    var glass=0.0;
    if kind==4u {
        // Neon or phosphor: its own glow, as `material_colour` makes it. A recoloured tube
        // is white-hot along its core, where it faces the viewer (`n` is in the
        // camera's frame), and saturated towards its edges.
        let phosphor=0.8+p.extra.x*0.45*(fire_noise(vec3<f32>(uv*150.0,0.0),431u)-0.5);
        let pulse=0.92+0.08*cos(p.values.w*6.2831853);
        let glow=studio.palette[0].rgb*1.9;
        let rim=pow(max(dot(n,normalize(vec3<f32>(-0.6,-0.8,0.6))),0.0),8.0)*2.4;
        emission=glow*phosphor*p.extra.y*pulse+vec3<f32>(rim*0.08);
        if studio.palette[2].w>=0.5 {
            let core=smoothstep(0.55,1.0,n.z);
            let hot=vec3<f32>(max(glow.r,max(glow.g,glow.b)));
            emission=mix(glow,hot,0.18*core)*phosphor*p.extra.y*pulse*(0.45+0.25*core)+vec3<f32>(rim*0.08);
        }
    } else if kind==5u {
        albedo=wood_colour(uv);
    } else if kind==24u {
        glass=1.0;
    } else if kind==22u {
        // Holographic foil: a mirror tinted by a rainbow that shifts with the angle.
        let hue=fract(dot(n.xy,vec2<f32>(0.9,0.6))*1.5+(uv.x+uv.y)*0.15+p.values.w);
        f0*=vec3<f32>(0.55)+0.45*cos(6.2831853*(vec3<f32>(hue)+vec3<f32>(0.0,0.33,0.67)));
    } else if kind==23u {
        albedo=studio.palette[0].rgb;
    } else if kind>=6u {
        let b=bumped(uv,n,kind);
        normal=b.normal;
        albedo=b.albedo;
        if kind==26u {
            // Pearl: a film whose colour shifts with the angle and swirls across the face.
            let angle=1.0-clamp(n.z,0.0,1.0);
            let swirl=fire_fbm(vec3<f32>(uv*2.5,0.3),733u)*2.0;
            let film=vec3<f32>(0.6)+0.4*cos(6.2831853*(vec3<f32>(angle*1.6+swirl)+vec3<f32>(0.0,0.33,0.67)));
            let sheen=film*studio.palette[1].rgb;
            albedo*=mix(vec3<f32>(1.0),sheen*1.4,0.25*angle+0.18);
            f0=max(f0,sheen*0.25);
        }
    }
    let point=surface_point(uv,normal,albedo);
    let brushed=select(0.0,p.extra.x,p.extra.w>0.5);
    return MaterialPoint(point.normal,point.rough,point.albedo,studio.optics.w,f0,point.gain,emission,glass,point.across,brushed);
}
