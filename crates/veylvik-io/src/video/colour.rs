//! Explicit encoded-RGB to scene-linear Rec.709 conversion. Alpha stays straight
//! until the final multiplication, avoiding implicit decoder alpha transforms.
use crate::Result;
use veylvik_document::media::{Curve, Gamut, InputColour};
pub struct Transform {
    pub(super) lut: Vec<f32>,
    pub(super) matrix: [[f32; 3]; 3],
}
fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [e * i - f * h, c * h - b * i, b * f - c * e],
        [f * g - d * i, a * i - c * g, c * d - a * f],
        [d * h - e * g, b * g - a * h, a * e - b * d],
    ]
    .map(|r| r.map(|v| v / determinant))
}
fn multiply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
fn xyz(primaries: [[f64; 2]; 3]) -> [[f64; 3]; 3] {
    let columns = primaries.map(|[x, y]| [x / y, 1.0, (1.0 - x - y) / y]);
    let matrix = std::array::from_fn(|r| std::array::from_fn(|c| columns[c][r]));
    let scale = multiply(
        inverse(matrix),
        [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290],
    );
    std::array::from_fn(|r| std::array::from_fn(|c| matrix[r][c] * scale[c]))
}
/// BT.2100 HLG inverse OETF: signal 0..1 to normalised scene light 0..1.
fn hlg(v: f64) -> f64 {
    const A: f64 = 0.178_832_77;
    const B: f64 = 1.0 - 4.0 * A;
    const C: f64 = 0.559_910_73;
    if v <= 0.5 {
        v * v / 3.0
    } else {
        (((v - C) / A).exp() + B) / 12.0
    }
}
/// Puts HLG reference white (75% signal) at 2.03, where PQ's 203-nit reference white lands on
/// Veylvik's 100-nit scale, so HLG and PQ footage match in one composition.
const HLG_SCALE: f64 = 2.03 / 0.264_962_56;
fn tagged_primaries(code: u32) -> Result<[[f64; 2]; 3]> {
    Ok(match code {
        1 => [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]],
        5 => [[0.64, 0.33], [0.29, 0.60], [0.15, 0.06]],
        6 | 7 => [[0.630, 0.340], [0.310, 0.595], [0.155, 0.070]],
        9 => [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        _ => return Err("Unsupported video colour primaries; convert to Rec.709 first".into()),
    })
}
/// Camera and display gamuts by their published CIE xy primaries (all D65).
fn gamut_primaries(gamut: Gamut) -> [[f64; 2]; 3] {
    match gamut {
        Gamut::Auto | Gamut::Rec709 => [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]],
        Gamut::Rec2020 => [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        Gamut::P3D65 => [[0.680, 0.320], [0.265, 0.690], [0.150, 0.060]],
        Gamut::SGamut3 => [[0.730, 0.280], [0.140, 0.855], [0.100, -0.050]],
        Gamut::SGamut3Cine => [[0.766, 0.275], [0.225, 0.800], [0.089, -0.087]],
        Gamut::ArriWideGamut3 => [[0.6840, 0.3130], [0.2210, 0.8480], [0.0861, -0.1020]],
        Gamut::ArriWideGamut4 => [[0.7347, 0.2653], [0.1424, 0.8576], [0.0991, -0.0308]],
        Gamut::VGamut => [[0.730, 0.280], [0.165, 0.840], [0.100, -0.030]],
    }
}
/// Decoding curves for tagged transfers (H.273 codes); unknown codes use BT.709.
fn tagged(transfer: u32, v: f64) -> f64 {
    match transfer {
        8 => v,
        13 => {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        4 => v.powf(2.2),
        5 => v.powf(2.8),
        7 => {
            if v < 0.0912 {
                v / 4.0
            } else {
                ((v + 0.1115) / 1.1115).powf(1.0 / 0.45)
            }
        }
        // HLG (BT.2100) as scene light, without the display OOTF, scaled so
        // reference white (75% signal) matches PQ's 203-nit reference (2.03).
        18 => hlg(v) * HLG_SCALE,
        16 => {
            let power = v.powf(1.0 / (2523.0 / 32.0));
            ((power - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * power))
                .powf(1.0 / (2610.0 / 16384.0))
                * 100.0
        }
        14 | 15 => {
            if v < 4.5 * 0.018053968510807 {
                v / 4.5
            } else {
                ((v + 0.09929682680944) / 1.09929682680944).powf(1.0 / 0.45)
            }
        }
        _ => {
            if v < 0.081 {
                v / 4.5
            } else {
                ((v + 0.099) / 1.099).powf(1.0 / 0.45)
            }
        }
    }
}
/// Camera log curves from the manufacturers' published formulas, on the normalised signal
/// (0..1 after range expansion); 18% grey decodes to 0.18.
fn log_curve(curve: Curve, x: f64) -> f64 {
    match curve {
        Curve::AppleLog => {
            const R0: f64 = -0.056_410_88;
            const RT: f64 = 0.01;
            const C: f64 = 47.287_112_36;
            const BETA: f64 = 0.009_640_52;
            const GAMMA: f64 = 0.085_504_79;
            const DELTA: f64 = 0.693_369_45;
            let pt = C * (RT - R0).powi(2);
            if x >= pt {
                ((x - DELTA) / GAMMA).exp2() - BETA
            } else if x >= 0.0 {
                (x / C).sqrt() + R0
            } else {
                R0
            }
        }
        Curve::Slog3 => {
            let code = x * 1023.0;
            if code >= 171.210_294_692_9 {
                10f64.powf((code - 420.0) / 261.5) * 0.19 - 0.01
            } else {
                (code - 95.0) * 0.01125 / (171.210_294_692_9 - 95.0)
            }
        }
        Curve::Logc3 => {
            // ARRI LogC3 at EI 800.
            const CUT: f64 = 0.010_591;
            const A: f64 = 5.555_556;
            const B: f64 = 0.052_272;
            const C: f64 = 0.247_190;
            const D: f64 = 0.385_537;
            const E: f64 = 5.367_655;
            const F: f64 = 0.092_809;
            if x > E * CUT + F {
                (10f64.powf((x - D) / C) - B) / A
            } else {
                (x - F) / E
            }
        }
        Curve::Logc4 => {
            let a = (2f64.powi(18) - 16.0) / 117.45;
            let b = (1023.0 - 95.0) / 1023.0;
            let c = 95.0 / 1023.0;
            let s = (7.0 * std::f64::consts::LN_2 * 2f64.powf(7.0 - 14.0 * c / b)) / (a * b);
            let t = (2f64.powf(14.0 * (-c / b) + 6.0) - 64.0) / a;
            if x >= 0.0 {
                (2f64.powf(14.0 * (x - c) / b + 6.0) - 64.0) / a
            } else {
                x * s + t
            }
        }
        Curve::Vlog => {
            const CUT: f64 = 0.181;
            const B: f64 = 0.008_73;
            const C: f64 = 0.241_514;
            const D: f64 = 0.598_206;
            if x < CUT {
                (x - 0.125) / 5.6
            } else {
                10f64.powf((x - D) / C) - B
            }
        }
        // Display curves never reach here; they use their tagged decoders.
        _ => tagged(1, x),
    }
}
impl Transform {
    pub fn new(primaries: u32, transfer: u32) -> Result<Self> {
        Ok(Self::build(tagged_primaries(primaries)?, |v| {
            tagged(transfer, v)
        }))
    }
    /// The transform for a Footage node's input colour: each `Auto` part follows the file's
    /// tags (`primaries`, `transfer`), anything else overrides them.
    pub fn with_input(input: InputColour, primaries: u32, transfer: u32) -> Result<Self> {
        let source = match input.gamut {
            Gamut::Auto => tagged_primaries(primaries)?,
            gamut => gamut_primaries(gamut),
        };
        let code = match input.curve {
            Curve::Auto => transfer,
            Curve::Rec709 => 1,
            Curve::Srgb => 13,
            Curve::Linear => 8,
            Curve::Pq => 16,
            Curve::Hlg => 18,
            // Below their black point the curves decode to small negatives: sensor noise, not
            // light. Through a wide camera gamut those become strong casts in dark scenes, so
            // they map to zero, as editors do (and as Klipp does).
            camera => return Ok(Self::build(source, |v| log_curve(camera, v).max(0.0))),
        };
        Ok(Self::build(source, |v| tagged(code, v)))
    }
    /// A 16-bit lookup table of `curve` and the primaries matrix from `source` to Rec.709.
    fn build(source: [[f64; 2]; 3], curve: impl Fn(f64) -> f64) -> Self {
        let rec709 = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];
        let input = xyz(source);
        let output = inverse(xyz(rec709));
        let matrix = std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..3).map(|k| output[r][k] * input[k][c]).sum::<f64>() as f32)
        });
        let lut = (0..=65535)
            .map(|n| curve(n as f64 / 65535.0) as f32)
            .collect();
        Self { lut, matrix }
    }
    pub fn pixels(&self, bytes: &[u8]) -> Vec<[f32; 4]> {
        bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|p| {
                let sample = |c: usize| u16::from_le_bytes([p[c * 2], p[c * 2 + 1]]) as usize;
                let rgb = [
                    self.lut[sample(0)],
                    self.lut[sample(1)],
                    self.lut[sample(2)],
                ];
                let alpha = sample(3) as f32 / 65535.0;
                let rgb = self
                    .matrix
                    .map(|m| (m[0] * rgb[0] + m[1] * rgb[1] + m[2] * rgb[2]) * alpha);
                [rgb[0], rgb[1], rgb[2], alpha]
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_and_alpha_have_a_single_explicit_conversion() {
        let transform = Transform::new(1, 13).unwrap();
        let bytes = [32768u16, 32768, 32768, 32768]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let p = transform.pixels(&bytes)[0];
        assert!((p[0] - 0.10702).abs() < 0.0001);
        assert!((p[3] - 0.5).abs() < 0.0001);
        let pq = Transform::new(9, 16).unwrap();
        assert!((pq.lut[32768] - 0.9225).abs() < 0.001);
        // HLG: reference white (75%) at 2.03 like PQ's 203 nits, mid signal on the square-law
        // segment, peak signal at the scale itself.
        let hlg = Transform::new(9, 18).unwrap();
        let at = |v: f64| hlg.lut[(v * 65535.0).round() as usize];
        assert!((at(0.75) - 2.03).abs() < 0.002, "{}", at(0.75));
        assert!((at(0.5) - 0.25 / 3.0 * HLG_SCALE as f32).abs() < 0.002);
        assert!((at(1.0) - HLG_SCALE as f32).abs() < 0.01);
        assert!(hlg.lut.windows(2).all(|p| p[0] <= p[1]), "monotonic");
    }
    #[test]
    fn camera_log_curves_put_their_published_grey_at_0_18() {
        // Each curve's 18% grey signal, from the manufacturers' specifications.
        for (curve, grey) in [
            (Curve::AppleLog, 0.488_34),
            (Curve::Slog3, 420.0 / 1023.0),
            (Curve::Logc3, 0.391_007),
            (Curve::Logc4, 0.278_396),
            (Curve::Vlog, 0.423_311),
        ] {
            let linear = log_curve(curve, grey);
            assert!((linear - 0.18).abs() < 0.001, "{curve:?}: {linear}");
            // Brighter signal means more light.
            assert!(log_curve(curve, grey + 0.1) > linear);
        }
        // Auto parts follow the file's tags; an override replaces them.
        let tagged = Transform::with_input(InputColour::default(), 1, 13).unwrap();
        assert_eq!(tagged.lut[32768], Transform::new(1, 13).unwrap().lut[32768]);
        let slog = Transform::with_input(
            InputColour {
                curve: Curve::Slog3,
                gamut: Gamut::SGamut3Cine,
            },
            1,
            1,
        )
        .unwrap();
        let at = (420.0 / 1023.0 * 65535.0_f64).round() as usize;
        assert!((slog.lut[at] - 0.18).abs() < 0.001);
        // Below the black point the table holds zero, never negative noise.
        assert_eq!(slog.lut[0], 0.0);
        assert!(slog.lut.iter().all(|v| *v >= 0.0));
        // A wide camera gamut maps neutral grey to neutral grey (rows sum to one).
        for row in slog.matrix {
            assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-4, "{row:?}");
        }
    }
}

impl Transform {
    /// CPU realization is reserved for callers explicitly requesting host pixels.
    /// Normal rendering uses the renderer's plane conversion on its own device.
    pub(super) fn decoded_pixels(
        &self,
        bytes: &[u8],
        e: veylvik_render::VideoEncoding,
    ) -> Vec<[f32; 4]> {
        use veylvik_render::VideoFormat;
        if e.format == VideoFormat::Rgba64 {
            return self.pixels(bytes);
        }
        let (width, height) = e.dimensions();
        let depth = if e.format == VideoFormat::P010 {
            4.0
        } else {
            1.0
        };
        let sample = |i: usize| {
            if e.format == VideoFormat::P010 {
                f32::from(u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]) >> 6)
            } else {
                f32::from(bytes[i])
            }
        };
        let uv = |x: i32, y: i32| {
            let x = x.clamp(0, e.width as i32 / 2 - 1) as usize;
            let y = y.clamp(0, e.height as i32 / 2 - 1) as usize;
            let i = e.width as usize * e.height as usize + y * e.width as usize + x * 2;
            [sample(i), sample(i + 1)]
        };
        (0..width * height)
            .map(|i| {
                let (x, y) = (i % width, i / width);
                let (x, y) = match e.rotation {
                    90 => (e.width - 1 - y, x),
                    180 => (e.width - 1 - x, e.height - 1 - y),
                    270 => (y, e.height - 1 - x),
                    _ => (x, y),
                };
                let mut luma = sample((y * e.width + x) as usize);
                let cx = (x as f32 - e.chroma_location[0]) * 0.5;
                let cy = (y as f32 - e.chroma_location[1]) * 0.5;
                let (bx, by) = (cx.floor() as i32, cy.floor() as i32);
                let (fx, fy) = (cx - cx.floor(), cy - cy.floor());
                let a = uv(bx, by);
                let b = uv(bx + 1, by);
                let c = uv(bx, by + 1);
                let d = uv(bx + 1, by + 1);
                let mut chroma = std::array::from_fn::<_, 2, _>(|k| {
                    (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy)
                        + (c[k] * (1.0 - fx) + d[k] * fx) * fy
                });
                if e.full_range {
                    luma /= 256.0 * depth - 1.0;
                    chroma = chroma.map(|v| (v - 128.0 * depth) / (256.0 * depth - 1.0));
                } else {
                    luma = (luma - 16.0 * depth) / (219.0 * depth);
                    chroma = chroma.map(|v| (v - 128.0 * depth) / (224.0 * depth));
                }
                let kg = 1.0 - e.kr - e.kb;
                let rgb = [
                    luma + 2.0 * (1.0 - e.kr) * chroma[1],
                    luma - 2.0 * e.kb * (1.0 - e.kb) / kg * chroma[0]
                        - 2.0 * e.kr * (1.0 - e.kr) / kg * chroma[1],
                    luma + 2.0 * (1.0 - e.kb) * chroma[0],
                ];
                let rgb = rgb.map(|v| self.lut[(v.clamp(0.0, 1.0) * 65535.0).round() as usize]);
                let rgb = self
                    .matrix
                    .map(|m| m[0] * rgb[0] + m[1] * rgb[1] + m[2] * rgb[2]);
                [rgb[0], rgb[1], rgb[2], 1.0]
            })
            .collect()
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    use veylvik_render::{Renderer, VideoEncoding, VideoFormat};
    #[test]
    #[ignore = "requires Vulkan"]
    fn reduced_conversion_area_averages_the_full_conversion_with_rotation() {
        let renderer = Renderer::new().unwrap();
        let colour = Transform::new(1, 13).unwrap();
        for rotation in [0, 90] {
            let encoding = VideoEncoding {
                width: 8,
                height: 4,
                format: VideoFormat::Nv12,
                rotation,
                kr: 0.2126,
                kb: 0.0722,
                full_range: false,
                chroma_location: [0.0, 0.5],
                primaries: colour.matrix,
            };
            let bytes: Vec<u8> = (0..48u32)
                .map(|i| {
                    if i < 32 {
                        (40 + i * 6) as u8
                    } else {
                        (110 + (i % 5) * 9) as u8
                    }
                })
                .collect();
            let full = renderer
                .import_video(&bytes, encoding, &colour.lut)
                .unwrap();
            let full = renderer.read_frame(&full).unwrap();
            let (w, h) = encoding.dimensions();
            let half = renderer
                .import_video_sized(&bytes, encoding, &colour.lut, Some([w / 2, h / 2]))
                .unwrap();
            assert_eq!((half.width(), half.height()), (w / 2, h / 2));
            let half = renderer.read_frame(&half).unwrap();
            for y in 0..h / 2 {
                for x in 0..w / 2 {
                    let at =
                        |dx: u32, dy: u32| full.pixels[((y * 2 + dy) * w + x * 2 + dx) as usize];
                    let expected: Vec<f32> = (0..4)
                        .map(|c| (at(0, 0)[c] + at(1, 0)[c] + at(0, 1)[c] + at(1, 1)[c]) / 4.)
                        .collect();
                    let actual = half.pixels[(y * (w / 2) + x) as usize];
                    for (a, b) in actual.iter().zip(&expected) {
                        assert!(
                            (a - b).abs() < 1e-5,
                            "rotation {rotation} at {x},{y}: {a} != {b}"
                        );
                    }
                }
            }
            assert!(
                renderer
                    .import_video_sized(&bytes, encoding, &colour.lut, Some([w + 1, h]))
                    .is_err()
            );
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn planes_preserve_hdr_range_chroma_and_rotated_geometry() {
        let renderer = Renderer::new().unwrap();
        for format in [VideoFormat::Nv12, VideoFormat::P010] {
            for full_range in [false, true] {
                for rotation in [0, 90, 180, 270] {
                    let colour = Transform::new(9, 16).unwrap();
                    let encoding = VideoEncoding {
                        width: 8,
                        height: 4,
                        format,
                        rotation,
                        kr: 0.2627,
                        kb: 0.0593,
                        full_range,
                        chroma_location: [0.5, 0.5],
                        primaries: colour.matrix,
                    };
                    let samples: Vec<u16> = (0..48)
                        .map(|i| {
                            if i < 32 {
                                64 + i * 5
                            } else {
                                100 + (i % 7) * 6
                            }
                        })
                        .collect();
                    let bytes = if format == VideoFormat::Nv12 {
                        samples.iter().map(|v| *v as u8).collect::<Vec<_>>()
                    } else {
                        samples
                            .iter()
                            .flat_map(|v| (v * 4 * 64).to_le_bytes())
                            .collect()
                    };
                    let gpu = renderer
                        .import_video(&bytes, encoding, &colour.lut)
                        .unwrap();
                    assert_eq!((gpu.width(), gpu.height()), encoding.dimensions());
                    let pixels = renderer.read_frame(&gpu).unwrap().pixels;
                    let reference = colour.decoded_pixels(&bytes, encoding);
                    for (a, b) in pixels.iter().zip(reference) {
                        for (a, b) in a.iter().zip(b) {
                            assert!((a - b).abs() < 0.015, "{format:?} {rotation} {a} != {b}");
                        }
                    }
                }
            }
        }
        let colour = Transform::new(1, 13).unwrap();
        let encoding = VideoEncoding {
            width: 1,
            height: 1,
            format: VideoFormat::Rgba64,
            rotation: 0,
            kr: 0.2126,
            kb: 0.0722,
            full_range: true,
            chroma_location: [0.0, 0.5],
            primaries: colour.matrix,
        };
        let bytes = [32768u16, 16384, 60000, 32768]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let result = renderer
            .read_frame(
                &renderer
                    .import_video(&bytes, encoding, &colour.lut)
                    .unwrap(),
            )
            .unwrap();
        for (a, b) in result.pixels[0].iter().zip(colour.pixels(&bytes)[0]) {
            assert!((a - b).abs() < 1e-6);
        }
        assert!(
            renderer
                .import_video(&bytes[..7], encoding, &colour.lut)
                .is_err()
        );
        let mut invalid = encoding;
        invalid.width = 8193;
        assert!(renderer.import_video(&bytes, invalid, &colour.lut).is_err());
    }
}
