// Copies of 3D objects at particles (see instances.rs): three values a particle for the
// 3D renderer, where it is and its size (w; 0 when it is not alive), its turn as a
// quaternion, and its tint. first = a flock's state (or nothing), third = the settings
// as the fields pass lays them out; values.z = the copies' scale, values.w = tumble.
fn quaternion(axis: vec3<f32>, angle: f32) -> vec4<f32> {
    return vec4<f32>(axis * sin(angle * 0.5), cos(angle * 0.5));
}
fn quaternion_times(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(a.w * b.xyz + b.w * a.xyz + cross(a.xyz, b.xyz), a.w * b.w - dot(a.xyz, b.xyz));
}
// A random direction for a particle, `purpose` and the next one used.
fn random_direction(thread: u32, generation: u32, purpose: u32) -> vec3<f32> {
    let z = random(thread, generation, purpose) * 2.0 - 1.0;
    let a = random(thread, generation, purpose + 1u) * 6.2831853;
    let r = sqrt(max(1.0 - z * z, 0.0));
    return vec3<f32>(r * cos(a), r * sin(a), z);
}
@compute @workgroup_size(64)
fn particle_instances(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread = gid.y * u32(p.values.y) + gid.x;
    if gid.x >= u32(p.values.y) || thread >= u32(p.values.x) { return; }
    let now = bitcast<f32>(p.size_op.z);
    var where_scale = vec4<f32>(0.0); var turn = vec4<f32>(0.0, 0.0, 0.0, 1.0); var tint = vec4<f32>(0.0);
    let b = born(thread, 0u, now);
    let age = now - b.birth;
    if b.ok && age >= 0.0 && age < b.own_life && (!simulated() || simulated_at(thread, b.generation)) {
        let at = place(b.origin, b.launch, b.birth, age, b.own_life, b.goal, b.homing, b.deep);
        let share = age / b.own_life;
        var growth = mix(1.0, setting(GROW, b.birth), share);
        if has_curve(1u, b.birth) { growth = life_curve(SIZE_CURVE, b.birth, share); }
        let variation = setting(VARIATION, b.birth);
        let size = growth * (1.0 - 0.6 * variation * random(thread, b.generation, 7u)) * p.values.z;
        where_scale = vec4<f32>(at, max(size, 0.0));
        if p.values.w > 0.5 {
            turn = quaternion(random_direction(thread, b.generation, 26u), random(thread, b.generation, 28u) * 6.2831853);
        }
        let spin = radians(setting(SPIN, b.birth));
        if spin != 0.0 {
            let way = sign(random(thread, b.generation, 11u) - 0.5);
            let spun = spin * age * (1.0 - variation * random(thread, b.generation, 10u)) * way;
            turn = quaternion_times(quaternion(random_direction(thread, b.generation, 29u), spun), turn);
        }
        let first_colour = vec4<f32>(setting(COLOUR, b.birth), setting(COLOUR + 1u, b.birth), setting(COLOUR + 2u, b.birth), setting(COLOUR_A, b.birth));
        let last_colour = vec4<f32>(setting(END_COLOUR, b.birth), setting(END_COLOUR + 1u, b.birth), setting(END_COLOUR + 2u, b.birth), setting(END_A, b.birth));
        var colour = mix(first_colour, last_colour, share);
        if has_curve(8u, b.birth) { colour = life_colour(b.birth, share); }
        let hues = setting(HUES, b.birth);
        if hues > 0.0 { colour = vec4<f32>(turned(colour.rgb, (random(thread, b.generation, 12u) - 0.5) * 6.2831853 * hues), colour.a); }
        tint = colour;
    }
    store(3u * thread, where_scale);
    store(3u * thread + 1u, turn);
    store(3u * thread + 2u, tint);
}
