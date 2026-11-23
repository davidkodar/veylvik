// Gaussian splats: preprocess (each splat's place on screen, its ellipse as a conic
// in pixels, its colour seen from the camera, and a key that sorts it back to front),
// a bitonic sort of the keys, then each splat drawn as a soft quad.
struct Frame {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    model: mat4x4<f32>,
    // width, height, focal length in pixels, splats
    viewport: vec4<f32>,
    // camera's place (xyz), spherical harmonics degree (w)
    eye: vec4<f32>,
    // brightness, padded count, coefficients a colour, unused
    params: vec4<f32>,
}
struct Projected {
    centre: vec4<f32>,
    // conic (xyz) and radius in pixels (w; 0: not drawn)
    conic: vec4<f32>,
    colour: vec4<f32>,
}
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> splats: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> rest: array<f32>;
@group(0) @binding(3) var<storage, read_write> keys: array<u32>;
@group(0) @binding(4) var<storage, read_write> values: array<u32>;
@group(0) @binding(5) var<storage, read_write> projected: array<Projected>;

fn turn(q: vec4<f32>) -> mat3x3<f32> {
    let x = q.x; let y = q.y; let z = q.z; let w = q.w;
    // Columns.
    return mat3x3<f32>(
        vec3<f32>(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)),
        vec3<f32>(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)),
        vec3<f32>(2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)));
}
fn linear(m: mat4x4<f32>) -> mat3x3<f32> { return mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz); }
// A splat's colour seen along `d` (in its own space): the base and the view-dependent
// bands it has.
fn seen_colour(i: u32, base: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let degree = u32(frame.eye.w);
    var c = base;
    if degree == 0u { return c; }
    let k = u32(frame.params.z);
    let at = i * k * 3u;
    let x = d.x; let y = d.y; let z = d.z;
    var basis: array<f32, 15>;
    basis[0] = -0.4886025 * y; basis[1] = 0.4886025 * z; basis[2] = -0.4886025 * x;
    if degree >= 2u {
        let xx = x * x; let yy = y * y; let zz = z * z;
        basis[3] = 1.0925484 * x * y; basis[4] = -1.0925484 * y * z;
        basis[5] = 0.3153916 * (2.0 * zz - xx - yy); basis[6] = -1.0925484 * x * z;
        basis[7] = 0.5462742 * (xx - yy);
        if degree >= 3u {
            basis[8] = -0.5900436 * y * (3.0 * xx - yy); basis[9] = 2.8906114 * x * y * z;
            basis[10] = -0.4570458 * y * (4.0 * zz - xx - yy);
            basis[11] = 0.3731763 * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
            basis[12] = -0.4570458 * x * (4.0 * zz - xx - yy);
            basis[13] = 1.4453057 * z * (xx - yy); basis[14] = -0.5900436 * x * (xx - 3.0 * yy);
        }
    }
    for (var b = 0u; b < k; b++) {
        c += basis[b] * vec3<f32>(rest[at + b], rest[at + k + b], rest[at + 2u * k + b]);
    }
    return c;
}
@compute @workgroup_size(256)
fn preprocess(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let padded = u32(frame.params.y);
    if i >= padded { return; }
    values[i] = i;
    keys[i] = 0xffffffffu;
    let count = u32(frame.viewport.w);
    if i >= count { return; }
    projected[i].conic = vec4<f32>(0.0);
    let a = splats[i * 4u]; let s = splats[i * 4u + 1u]; let q = splats[i * 4u + 2u]; let c = splats[i * 4u + 3u];
    let world = frame.model * vec4<f32>(a.xyz, 1.0);
    let v = frame.view * world;
    let depth = -v.z;
    if depth <= 0.2 { return; }
    // Its shape in the camera's space, then on the screen (pixels, y down).
    let m = linear(frame.view) * linear(frame.model) * turn(q) * mat3x3<f32>(
        vec3<f32>(s.x, 0.0, 0.0), vec3<f32>(0.0, s.y, 0.0), vec3<f32>(0.0, 0.0, s.z));
    let shape = m * transpose(m);
    let f = frame.viewport.z;
    // Its look along the view, held in to just off the screen's edges.
    let reach = 1.3 * vec2<f32>(frame.viewport.x, frame.viewport.y) * 0.5 / f;
    let t = vec2<f32>(clamp(v.x / depth, -reach.x, reach.x), clamp(v.y / depth, -reach.y, reach.y)) * depth;
    let j = mat3x3<f32>(
        vec3<f32>(f / depth, 0.0, 0.0),
        vec3<f32>(0.0, f / depth, 0.0),
        vec3<f32>(f * t.x / (depth * depth), f * t.y / (depth * depth), 0.0));
    let flat = j * shape * transpose(j);
    let ca = flat[0][0] + 0.3; let cb = flat[0][1]; let cc = flat[1][1] + 0.3;
    let det = ca * cc - cb * cb;
    if det <= 0.0 { return; }
    let middle = 0.5 * (ca + cc);
    let largest = middle + sqrt(max(middle * middle - det, 0.1));
    let radius = ceil(3.0 * sqrt(largest));
    // Its colour along the view, in its own space.
    let look = normalize(transpose(linear(frame.model)) * (world.xyz - frame.eye.xyz));
    // Splat colours are fitted to photographs, as displayed: into linear light.
    let shown = max(seen_colour(i, c.rgb, look), vec3<f32>(0.0));
    let linear_light = select(pow((shown + 0.055) / 1.055, vec3<f32>(2.4)), shown / 12.92, shown <= vec3<f32>(0.04045));
    let colour = linear_light * frame.params.x;
    projected[i].centre = frame.projection * v;
    projected[i].conic = vec4<f32>(cc / det, -cb / det, ca / det, radius);
    projected[i].colour = vec4<f32>(colour, a.w);
    // Farther first: the larger the depth, the smaller the key.
    keys[i] = 0xfffffffeu - bitcast<u32>(depth);
}
// ---- bitonic sort: one pass of the network, `step` and `block` from the uniform ----
struct Pass { step: u32, block: u32 }
@group(0) @binding(6) var<uniform> sorting: Pass;
@compute @workgroup_size(256)
fn sort(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let padded = u32(frame.params.y);
    let l = i ^ sorting.step;
    if i >= padded || l <= i { return; }
    let up = (i & sorting.block) == 0u;
    let a = keys[i]; let b = keys[l];
    if (a > b) == up {
        keys[i] = b; keys[l] = a;
        let va = values[i]; values[i] = values[l]; values[l] = va;
    }
}
// ---- draw: each splat a quad its radius wide, back to front ----
struct Varying {
    @builtin(position) clip: vec4<f32>,
    @location(0) offset: vec2<f32>,
    @location(1) @interpolate(flat) conic: vec3<f32>,
    @location(2) @interpolate(flat) colour: vec4<f32>,
}
@group(0) @binding(7) var<storage, read> order: array<u32>;
@group(0) @binding(8) var<storage, read> drawn: array<Projected>;
@vertex
fn vertex(@builtin(vertex_index) corner: u32, @builtin(instance_index) n: u32) -> Varying {
    var out: Varying;
    let p = drawn[order[n]];
    if p.conic.w <= 0.0 {
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        return out;
    }
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let offset = corners[corner] * p.conic.w;
    out.clip = p.centre + vec4<f32>(offset.x * 2.0 / frame.viewport.x, -offset.y * 2.0 / frame.viewport.y, 0.0, 0.0) * p.centre.w;
    out.offset = offset;
    out.conic = p.conic.xyz;
    out.colour = p.colour;
    return out;
}
@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    let d = in.offset;
    let power = -0.5 * (in.conic.x * d.x * d.x + in.conic.z * d.y * d.y) - in.conic.y * d.x * d.y;
    if power > 0.0 { discard; }
    let alpha = min(0.99, in.colour.a * exp(power));
    if alpha < 1.0 / 255.0 { discard; }
    return vec4<f32>(in.colour.rgb * alpha, alpha);
}
