// Packs one earlier frame of an image into a tile of an atlas: grid x grid tiles, each
// the frame averaged down by `grid`.
struct Pack { width: u32, height: u32, grid: u32, tile: u32 }
@group(0) @binding(0) var<storage, read> sample: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> atlas: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> pack: Pack;
@compute @workgroup_size(16, 16)
fn pack_tile(@builtin(global_invocation_id) id: vec3<u32>) {
    let across = pack.width / pack.grid; let down = pack.height / pack.grid;
    if id.x >= across || id.y >= down { return; }
    var sum = vec4<f32>(0.0);
    for (var j = 0u; j < pack.grid; j++) {
        for (var i = 0u; i < pack.grid; i++) {
            sum += sample[(id.y * pack.grid + j) * pack.width + id.x * pack.grid + i];
        }
    }
    let x = (pack.tile % pack.grid) * across + id.x;
    let y = (pack.tile / pack.grid) * down + id.y;
    atlas[y * pack.width + x] = sum / f32(pack.grid * pack.grid);
}
