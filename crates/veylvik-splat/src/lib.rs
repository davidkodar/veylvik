//! Gaussian splats: a scanned scene as up to millions of soft, coloured, see-through ellipsoids, read from the .ply files capture tools write, and drawn on the GPU sorted back to front over a 3D scene's solids.
mod gpu;
mod ply;
pub use gpu::{Draw, Prepared, Renderer, Uploaded};
pub use ply::write as write_ply;

/// Reads splats from a file's bytes: a .ply (its header says so) or a .splat (32 bytes
/// a splat: place and size as floats, colour and opacity as bytes, turn as bytes).
pub fn parse(bytes: &[u8]) -> Result<Splats, String> {
    if bytes.starts_with(b"ply") {
        ply::parse(bytes)
    } else {
        ply::parse_splat(bytes)
    }
}

/// One splat described for `write_ply`: place, colour, opacity, size, turn (x y z w).
pub type Described = ([f32; 3], [f32; 3], f32, [f32; 3], [f32; 4]);
/// Most splats in one file.
pub const MOST: usize = 4_000_000;
/// Floats each splat takes on the GPU: place and opacity, size (three axes), turn
/// (a quaternion), base colour.
pub const STRIDE: usize = 16;

/// Splats as read: for each, its place, opacity (0..1), size along its own three axes,
/// turn, base colour (linear-ish RGB as the capture fitted it) and, with `degree` above
/// 0, its view-dependent colour (spherical harmonics past the first, three a band).
#[derive(Debug, Clone)]
pub struct Splats {
    pub count: usize,
    /// STRIDE floats a splat.
    pub data: Vec<f32>,
    /// Spherical harmonics degree held (0 to 3), and `(degree + 1)^2 - 1` coefficients
    /// of three colours a splat in `rest`, colour after colour as the file has them.
    pub degree: u32,
    pub rest: Vec<f32>,
    /// The box most splats lie in (from the 2nd to the 98th percentile on each axis, so
    /// a few stray far splats do not set the size), as `(low, high)`.
    pub bounds: ([f32; 3], [f32; 3]),
}
impl Splats {
    /// Coefficients a splat has past the first, per colour.
    pub fn coefficients(&self) -> usize {
        ((self.degree + 1) * (self.degree + 1) - 1) as usize
    }
    /// The middle and largest side of the box most splats lie in.
    pub fn extent(&self) -> ([f32; 3], f32) {
        let (low, high) = self.bounds;
        let middle = [0, 1, 2].map(|k| (low[k] + high[k]) * 0.5);
        let side = (0..3).map(|k| high[k] - low[k]).fold(0.0f32, f32::max);
        (middle, side.max(1e-6))
    }
}
/// The box most of `positions` lie in, from the 2nd to the 98th percentile per axis.
pub(crate) fn bounds(data: &[f32], count: usize) -> ([f32; 3], [f32; 3]) {
    let mut low = [0.0; 3];
    let mut high = [0.0; 3];
    if count == 0 {
        return (low, high);
    }
    for k in 0..3 {
        let mut axis: Vec<f32> = (0..count).map(|i| data[i * STRIDE + k]).collect();
        axis.sort_by(f32::total_cmp);
        low[k] = axis[count * 2 / 100];
        high[k] = axis[(count * 98 / 100).min(count - 1)];
    }
    // Grown by twice a splat's typical radius, so a few splats still have a size.
    let mut radii: Vec<f32> = (0..count)
        .map(|i| {
            data[i * STRIDE + 4]
                .max(data[i * STRIDE + 5])
                .max(data[i * STRIDE + 6])
        })
        .collect();
    radii.sort_by(f32::total_cmp);
    let margin = 2.0 * radii[count / 2];
    for k in 0..3 {
        low[k] -= margin;
        high[k] += margin;
    }
    (low, high)
}
