//! Direct area alignment after feature matching: Levenberg-Marquardt over every sampled pixel
//! of the tracking area on the GPU. Only small normal-equation sums return to the host.
use crate::{Result, error};
use veylvik_document::planar::{Homography, Quad};
use veylvik_render::{GpuFrame, Renderer};

const TERMS: usize = 67;
const THREADS: u32 = 2048;

pub(crate) struct AreaAligner {
    pipeline: wgpu::ComputePipeline,
}
struct Sums {
    normal: [[f64; 10]; 10],
    gradient: [f64; 10],
    error: f64,
    count: f64,
}
impl AreaAligner {
    pub fn new(renderer: &Renderer) -> Self {
        let module = renderer
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("area alignment"),
                source: wgpu::ShaderSource::Wgsl(include_str!("refine.wgsl").into()),
            });
        let pipeline =
            renderer
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("area alignment"),
                    layout: None,
                    module: &module,
                    entry_point: Some("accumulate"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        Self { pipeline }
    }
    /// Refines `initial` (reference → current, normalized coordinates) by aligning the pixels of
    /// `area` on the reference frame. Returns `None` when alignment does not clearly improve
    /// the match or moves far from the feature solution, which then stands.
    #[allow(clippy::too_many_arguments)]
    pub fn align(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        occlusion: Option<&GpuFrame>,
        area: Quad,
        initial: Homography,
        perspective: bool,
        max_shift_pixels: f64,
    ) -> Result<Option<Homography>> {
        let (w, h) = (f64::from(reference.width()), f64::from(reference.height()));
        let (cx, cy, scale) = (w * 0.5, h * 0.5, w.max(h) * 0.5);
        // Normalized [0,1] coordinates to centred pixel coordinates divided by `scale`.
        let to_centred = Homography {
            matrix: [
                w / scale,
                0.,
                (-0.5 - cx) / scale,
                0.,
                h / scale,
                (-0.5 - cy) / scale,
                0.,
                0.,
                1.,
            ],
        };
        let from_centred = to_centred
            .inverse()
            .ok_or_else(|| error("invalid frame size"))?;
        let mut motion = to_centred.compose(initial).compose(from_centred);
        let m8 = motion.matrix[8];
        if !m8.is_finite() || m8.abs() < 1e-12 {
            return Ok(None);
        }
        motion.matrix = motion.matrix.map(|v| v / m8);
        if !perspective {
            motion.matrix[6] = 0.;
            motion.matrix[7] = 0.;
        }
        let pixels: Quad = area.map(|[x, y]| [x * w - 0.5, y * h - 0.5]);
        // Whole-pixel sampling grid: the kernel reads reference pixels at exactly these positions.
        let lo = [0, 1].map(|axis| {
            pixels
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
                .max(1.)
                .ceil()
        });
        let hi = [0, 1].map(|axis| {
            pixels
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
                .min([w, h][axis] - 3.)
                .floor()
        });
        if hi[0] <= lo[0] || hi[1] <= lo[1] {
            return Ok(None);
        }
        // Sample about a quarter of a million pixels whatever the area's size.
        let span = (hi[0] - lo[0]) * (hi[1] - lo[1]);
        let stride = ((span / 250_000.).sqrt().ceil() as u32).max(1);
        let cols = ((hi[0] - lo[0]) / f64::from(stride)).floor() as u32 + 1;
        let rows = ((hi[1] - lo[1]) / f64::from(stride)).floor() as u32 + 1;
        let mut photometric = [1.0_f64, 0.0];
        let unknowns: &[usize] = if perspective {
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        } else {
            &[0, 1, 2, 3, 4, 5, 8, 9]
        };
        let run = |motion: &Homography, photometric: [f64; 2]| -> Result<Sums> {
            let m = motion.matrix;
            let mut params = [0_u32; 32];
            params[..4].copy_from_slice(&[reference.width(), reference.height(), THREADS, stride]);
            let floats = [cx as f32, cy as f32, scale as f32];
            for (i, v) in floats.iter().enumerate() {
                params[4 + i] = v.to_bits();
            }
            params[7] = cols;
            for i in 0..8 {
                params[8 + i] = (m[i] as f32).to_bits();
            }
            let photo = [
                photometric[0] as f32,
                photometric[1] as f32,
                0.04,
                (cols * rows) as f32,
            ];
            for i in 0..4 {
                params[16 + i] = photo[i].to_bits();
            }
            for (i, p) in pixels.iter().enumerate() {
                params[20 + i * 2] = (p[0] as f32).to_bits();
                params[21 + i * 2] = (p[1] as f32).to_bits();
            }
            for (i, v) in [lo[0], lo[1], hi[0], hi[1]].iter().enumerate() {
                params[28 + i] = (*v as f32).to_bits();
            }
            let values: Vec<f32> = renderer.compute(|scratch| {
                let bytes = u64::from(THREADS) * TERMS as u64 * 4;
                let uniform = scratch.buffer(
                    128,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    "area params",
                )?;
                let output = scratch.buffer(
                    bytes,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    "area sums",
                )?;
                let readback = scratch.buffer(
                    bytes,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    "area sums readback",
                )?;
                let empty =
                    scratch.buffer(16, wgpu::BufferUsages::STORAGE, "area empty occlusion")?;
                scratch
                    .queue()
                    .write_buffer(&uniform, 0, bytemuck::cast_slice(&params));
                let bind = scratch
                    .device()
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("area alignment"),
                        layout: &self.pipeline.get_bind_group_layout(0),
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: uniform.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: reference.buffer().as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: current.buffer().as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: occlusion.map_or(empty.as_entire_binding(), |m| {
                                    m.buffer().as_entire_binding()
                                }),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: output.as_entire_binding(),
                            },
                        ],
                    });
                let mut encoder = scratch.device().create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(THREADS.div_ceil(64), 1, 1);
                }
                encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
                scratch.read(encoder, &readback)
            })?;
            let mut total = [0.0_f64; TERMS];
            for chunk in values.as_chunks::<TERMS>().0 {
                for (t, v) in total.iter_mut().zip(chunk) {
                    *t += f64::from(*v);
                }
            }
            // Unpack the upper triangle, row by row, into a symmetric matrix; both indices
            // address the matrix, so the index loop is the clearest form.
            let mut normal = [[0.; 10]; 10];
            let mut terms = total.iter();
            #[allow(clippy::needless_range_loop)]
            for a in 0..10 {
                for b in a..10 {
                    let value = *terms.next().expect("55 upper-triangle terms");
                    (normal[a][b], normal[b][a]) = (value, value);
                }
            }
            Ok(Sums {
                normal,
                gradient: std::array::from_fn(|i| total[55 + i]),
                error: total[65],
                count: total[66],
            })
        };
        let mean = |s: &Sums| {
            if s.count > 0. {
                s.error / s.count
            } else {
                f64::INFINITY
            }
        };
        let mut sums = run(&motion, photometric)?;
        // Too few pixels to say anything reliable: keep the feature solution.
        if sums.count < 400. {
            return Ok(None);
        }
        let before = mean(&sums);
        let mut lambda = 1e-3;
        for _ in 0..10 {
            let Some(step) = solve(&sums, unknowns, lambda) else {
                break;
            };
            let mut candidate = motion;
            for (k, &u) in unknowns.iter().enumerate() {
                if u < 8 {
                    candidate.matrix[u] -= step[k];
                }
            }
            let mut photo = photometric;
            for (k, &u) in unknowns.iter().enumerate() {
                if u >= 8 {
                    photo[u - 8] -= step[k];
                }
            }
            let next = run(&candidate, photo)?;
            if next.count >= sums.count * 0.8 && mean(&next) < mean(&sums) {
                let small = step.iter().map(|v| v.abs()).fold(0., f64::max) < 1e-6;
                (motion, photometric, sums) = (candidate, photo, next);
                lambda = (lambda * 0.3).max(1e-7);
                if small {
                    break;
                }
            } else {
                lambda *= 10.;
                if lambda > 1e4 {
                    break;
                }
            }
        }
        let after = mean(&sums);
        // Require a clear improvement (this also rejects NaN) and a plausible exposure change.
        let improved = after < before * 0.98;
        if !improved || !(0.5..=2.).contains(&photometric[0]) {
            return Ok(None);
        }
        let refined = from_centred.compose(motion).compose(to_centred);
        // Stay near the feature solution: alignment polishes it, never replaces it.
        let shift = area
            .iter()
            .filter_map(|&p| Some((refined.project(p)?, initial.project(p)?)))
            .map(|(a, b)| (((a[0] - b[0]) * w).powi(2) + ((a[1] - b[1]) * h).powi(2)).sqrt())
            .fold(0., f64::max);
        if shift > max_shift_pixels || refined.transform_quad(area).is_err() {
            return Ok(None);
        }
        Ok(Some(refined))
    }
}
/// Damped Gauss-Newton step for the selected unknowns, by Gaussian elimination.
fn solve(sums: &Sums, unknowns: &[usize], lambda: f64) -> Option<Vec<f64>> {
    let n = unknowns.len();
    let mut a: Vec<Vec<f64>> = unknowns
        .iter()
        .map(|&r| {
            let mut row: Vec<f64> = unknowns.iter().map(|&c| sums.normal[r][c]).collect();
            row.push(sums.gradient[r]);
            row
        })
        .collect();
    for (i, row) in a.iter_mut().enumerate() {
        row[i] += lambda * row[i].abs().max(1e-12);
    }
    for col in 0..n {
        let pivot = (col..n).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))?;
        if a[pivot][col].abs() < 1e-15 {
            return None;
        }
        a.swap(col, pivot);
        let (upper, lower) = a.split_at_mut(col + 1);
        let pivot_row = &upper[col];
        for row in lower.iter_mut() {
            let f = row[col] / pivot_row[col];
            for (value, pivot_value) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *value -= f * pivot_value;
            }
        }
    }
    let mut x = vec![0.; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (a[row][n] - s) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}
