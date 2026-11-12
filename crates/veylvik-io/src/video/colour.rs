//! Explicit encoded-RGB to scene-linear Rec.709 conversion. Alpha stays straight
//! until the final multiplication, avoiding implicit decoder alpha transforms.
use crate::Result;
pub struct Transform {
    lut: Vec<f32>,
    matrix: [[f32; 3]; 3],
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
impl Transform {
    pub fn new(primaries: u32, transfer: u32) -> Result<Self> {
        let rec709 = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];
        let source = match primaries {
            1 => rec709,
            5 => [[0.64, 0.33], [0.29, 0.60], [0.15, 0.06]],
            6 | 7 => [[0.630, 0.340], [0.310, 0.595], [0.155, 0.070]],
            9 => [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
            _ => return Err("Unsupported video colour primaries; convert to Rec.709 first".into()),
        };
        if transfer == 18 {
            return Err("HLG footage needs an explicit display transform; convert to linear EXR or SDR first".into());
        }
        let input = xyz(source);
        let output = inverse(xyz(rec709));
        let matrix = std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..3).map(|k| output[r][k] * input[k][c]).sum::<f64>() as f32)
        });
        let lut = (0..=65535)
            .map(|n| {
                let v = n as f64 / 65535.0;
                let linear = match transfer {
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
                    16 => {
                        let power = v.powf(1.0 / (2523.0 / 32.0));
                        ((power - 3424.0 / 4096.0).max(0.0)
                            / (2413.0 / 128.0 - 2392.0 / 128.0 * power))
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
                };
                linear as f32
            })
            .collect();
        Ok(Self { lut, matrix })
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
    }
}
