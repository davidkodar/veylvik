//! Frames exchanged with a host on the same device, buffer to buffer: the host's pixels (straight or premultiplied, linear, in primaries of its own) become a frame of this renderer (premultiplied, linear Rec.709), and a frame goes back the same way. The host states its own colour matrix, so the two cannot disagree about it.
use crate::{GpuFrame, Renderer, Result, error};

/// How a host stores a pixel: RGBA float32, linear light, rows top first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostPixels {
    /// Alpha is not multiplied into the colour.
    pub straight: bool,
    /// Rows of the matrix from the host's linear RGB to linear Rec.709 (D65).
    pub to_rec709: [[f32; 3]; 3],
}
type Matrix = [[f64; 3]; 3];
fn inverse(m: Matrix) -> Option<Matrix> {
    let cofactor = |r: usize, c: usize| {
        let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let determinant = (0..3).map(|c| m[0][c] * cofactor(0, c)).sum::<f64>();
    (determinant.abs() > 1e-12)
        .then(|| std::array::from_fn(|r| std::array::from_fn(|c| cofactor(c, r) / determinant)))
}
fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
/// Linear RGB with these chromaticities (x, y) to CIE XYZ, white at the space's own white.
fn to_xyz(red: [f64; 2], green: [f64; 2], blue: [f64; 2], white: [f64; 2]) -> Option<Matrix> {
    let column = |c: [f64; 2]| [c[0] / c[1], 1.0, (1.0 - c[0] - c[1]) / c[1]];
    let (r, g, b, w) = (column(red), column(green), column(blue), column(white));
    let primaries: Matrix = std::array::from_fn(|row| [r[row], g[row], b[row]]);
    let back = inverse(primaries)?;
    let scale: [f64; 3] = std::array::from_fn(|row| (0..3).map(|k| back[row][k] * w[k]).sum());
    Some(std::array::from_fn(|row| {
        std::array::from_fn(|c| primaries[row][c] * scale[c])
    }))
}
const REC709: [[f64; 2]; 4] = [
    [0.640, 0.330],
    [0.300, 0.600],
    [0.150, 0.060],
    [0.3127, 0.3290],
];
impl HostPixels {
    /// Linear Rec.709, as this renderer's own frames are.
    pub fn rec709(straight: bool) -> Self {
        Self {
            straight,
            to_rec709: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }
    /// Primaries given by their chromaticities, each space keeping its own white: no
    /// chromatic adaptation is applied, so a host whose white is not D65 states so by
    /// its white point and gets the same colours back.
    pub fn from_chromaticities(
        straight: bool,
        red: [f64; 2],
        green: [f64; 2],
        blue: [f64; 2],
        white: [f64; 2],
    ) -> Result<Self> {
        let bad = || error("these chromaticities do not make a colour space");
        let theirs = to_xyz(red, green, blue, white).ok_or_else(bad)?;
        let ours = to_xyz(REC709[0], REC709[1], REC709[2], REC709[3]).ok_or_else(bad)?;
        let m = product(inverse(ours).ok_or_else(bad)?, theirs);
        Ok(Self {
            straight,
            to_rec709: m.map(|row| row.map(|v| v as f32)),
        })
    }
    /// ACEScg (AP1) by the ACES reference matrix, which adapts its D60 white to D65 with
    /// CAT02: `AP1_2_REC709_PRI_MAT` of aces-dev, as OCIO's ACES configs use.
    pub fn acescg(straight: bool) -> Self {
        Self {
            straight,
            to_rec709: [
                [1.7050523622, -0.6217957415, -0.0832566207],
                [-0.1302600472, 1.1408029888, -0.0105430932],
                [-0.0239638781, -0.1289693959, 1.1529332720_f64],
            ]
            .map(|row| row.map(|v| v as f32)),
        }
    }
    /// Linear Rec.2020 (D65).
    pub fn rec2020(straight: bool) -> Result<Self> {
        Self::from_chromaticities(
            straight,
            [0.708, 0.292],
            [0.170, 0.797],
            [0.131, 0.046],
            [0.3127, 0.3290],
        )
    }
    fn back(&self) -> Result<[[f32; 3]; 3]> {
        inverse(self.to_rec709.map(|row| row.map(f64::from)))
            .map(|m| m.map(|row| row.map(|v| v as f32)))
            .ok_or_else(|| error("the host's colour matrix cannot be reversed"))
    }
}
const WGSL: &str = "
struct Exchange { size: vec4<u32>, row0: vec4<f32>, row1: vec4<f32>, row2: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Exchange;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> result: array<vec4<f32>>;
@compute @workgroup_size(8, 8)
fn exchange(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    let c = source[i];
    // The matrix is linear, so it means the same on straight and premultiplied colour.
    var rgb = vec3<f32>(dot(p.row0.xyz, c.rgb), dot(p.row1.xyz, c.rgb), dot(p.row2.xyz, c.rgb));
    if p.size.w == 1u {
        if p.size.z == 0u {
            rgb *= c.a;
        } else if c.a > 1e-6 {
            rgb /= c.a;
        }
        // Light where alpha is 0 has no straight form: it is passed on as it is.
    }
    result[i] = vec4<f32>(rgb, c.a);
}
";
pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("host frame exchange"),
        source: wgpu::ShaderSource::Wgsl(WGSL.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("host frame exchange"),
        layout: None,
        module: &module,
        entry_point: Some("exchange"),
        compilation_options: Default::default(),
        cache: None,
    })
}
impl Renderer {
    fn exchange(
        &self,
        scratch: &mut crate::compute::Scratch<'_, '_>,
        source: &wgpu::Buffer,
        result: &wgpu::Buffer,
        [width, height]: [u32; 2],
        (outwards, straight, matrix): (bool, bool, [[f32; 3]; 3]),
    ) -> Result<()> {
        let bytes = u64::from(width) * u64::from(height) * 16;
        if width == 0 || height == 0 || source.size() < bytes || result.size() < bytes {
            return Err(error("a frame buffer is smaller than its picture"));
        }
        let storage = wgpu::BufferUsages::STORAGE;
        if !source.usage().contains(storage) || !result.usage().contains(storage) {
            return Err(error(
                "frame buffers exchanged with a host must be storage buffers",
            ));
        }
        let mut words = Vec::with_capacity(64);
        for v in [width, height, u32::from(outwards), u32::from(straight)] {
            words.extend_from_slice(&v.to_le_bytes());
        }
        for row in matrix {
            for v in [row[0], row[1], row[2], 0.0] {
                words.extend_from_slice(&v.to_le_bytes());
            }
        }
        let device = scratch.device();
        let settings = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("host frame exchange"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        scratch.queue().write_buffer(&settings, 0, &words);
        fn whole(buffer: &wgpu::Buffer, bytes: u64) -> wgpu::BindingResource<'_> {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: std::num::NonZeroU64::new(bytes),
            })
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("host frame exchange"),
            layout: &self.host_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: settings.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: whole(source, bytes),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: whole(result, bytes),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.host_pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        scratch.run(encoder)
    }
    /// A frame of this renderer from a host's buffer on the same device: `width` by
    /// `height` pixels of RGBA float32, rows top first. The host keeps its buffer; the
    /// frame is a copy, finished when this returns.
    pub fn frame_from_host(
        &self,
        source: &wgpu::Buffer,
        width: u32,
        height: u32,
        pixels: &HostPixels,
    ) -> Result<GpuFrame> {
        self.compute(|scratch| {
            let frame = scratch.buffer(
                u64::from(width) * u64::from(height) * 16,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                "frame from the host",
            )?;
            self.exchange(
                scratch,
                source,
                &frame,
                [width, height],
                (false, pixels.straight, pixels.to_rec709),
            )?;
            scratch.frame(&frame, width, height)
        })
    }
    /// Write a whole frame of this renderer into a host's storage buffer on the same
    /// device, as the host stores pixels; finished when this returns.
    pub fn frame_to_host(
        &self,
        frame: &GpuFrame,
        target: &wgpu::Buffer,
        pixels: &HostPixels,
    ) -> Result<()> {
        if !self.owns(frame) {
            return Err(error("GPU frame belongs to another renderer"));
        }
        let matrix = pixels.back()?;
        self.compute(|scratch| {
            self.exchange(
                scratch,
                frame.buffer(),
                target,
                [frame.width(), frame.height()],
                (true, pixels.straight, matrix),
            )
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_host_states_its_colour_space_by_its_own_white() {
        // ACEScg is the ACES reference matrix (CAT02 from D60 to D65), to the last digit a
        // float holds.
        let ap1 = HostPixels::acescg(true).to_rec709;
        let want: [[f64; 3]; 3] = [
            [1.7050523622, -0.6217957415, -0.0832566207],
            [-0.1302600472, 1.1408029888, -0.0105430932],
            [-0.0239638781, -0.1289693959, 1.1529332720],
        ];
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (f64::from(ap1[r][c]) - want[r][c]).abs() < 1e-7,
                    "AP1 [{r}][{c}] = {}",
                    ap1[r][c]
                );
            }
        }
        // Adapted, so its white is ours.
        assert!(
            ap1.iter()
                .all(|row| (row.iter().sum::<f32>() - 1.0).abs() < 1e-6)
        );
        let wide = HostPixels::rec2020(false).unwrap().to_rec709;
        let want: [[f64; 3]; 3] = [
            [1.660491002, -0.5876411388, -0.07284986332],
            [-0.1245504745, 1.132899897, -0.008349422604],
            [-0.01815076335, -0.100578898, 1.118729661],
        ];
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (f64::from(wide[r][c]) - want[r][c]).abs() < 2e-6,
                    "Rec.2020 [{r}][{c}] = {}",
                    wide[r][c]
                );
            }
        }
        // Rec.2020 shares our white, so its white is ours.
        assert!(
            wide.iter()
                .all(|row| (row.iter().sum::<f32>() - 1.0).abs() < 1e-5)
        );
        // There and back is where it started.
        let host = HostPixels::acescg(true);
        let back = product(
            host.back().unwrap().map(|r| r.map(f64::from)),
            host.to_rec709.map(|r| r.map(f64::from)),
        );
        for (r, row) in back.iter().enumerate() {
            for (c, value) in row.iter().enumerate() {
                assert!((value - f64::from(u8::from(r == c))).abs() < 1e-5);
            }
        }
        assert!(
            HostPixels::from_chromaticities(true, [0.3, 0.3], [0.3, 0.3], [0.3, 0.3], [0.3, 0.3])
                .is_err()
        );
    }
}
