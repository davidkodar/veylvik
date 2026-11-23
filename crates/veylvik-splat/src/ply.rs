//! The .ply files Gaussian splatting tools write: a text header naming each splat's properties, then the splats as binary little-endian floats.
use crate::{MOST, STRIDE, Splats, bounds};

/// Normalisation of the base colour coefficient (spherical harmonics, band 0).
const SH0: f32 = 0.282_094_8;

/// Reads a splat .ply: binary little endian, each vertex with x, y, z, f_dc_0..2,
/// opacity (before the logistic), scale_0..2 (logarithms) and rot_0..3 (w first), and
/// any f_rest_* (view-dependent colour) up to degree 3.
pub fn parse(bytes: &[u8]) -> Result<Splats, String> {
    let end = find(bytes, b"end_header\n").ok_or("not a .ply file: no end of header")?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "the .ply header is not text")?;
    let mut lines = header.lines();
    if lines.next().map(str::trim) != Some("ply") {
        return Err("not a .ply file".into());
    }
    let mut count = None;
    let mut in_vertex = false;
    let mut properties: Vec<(String, usize)> = Vec::new(); // name and byte size
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", format, ..] if *format != "binary_little_endian" => {
                return Err(format!(
                    "{format} .ply files are not read; save it as binary little endian"
                ));
            }
            ["element", "vertex", n] => {
                count = Some(n.parse::<usize>().map_err(|_| "a bad vertex count")?);
                in_vertex = true;
            }
            ["element", ..] => in_vertex = false,
            ["property", kind, name] if in_vertex => {
                let size = match *kind {
                    "float" | "float32" | "int" | "uint" | "int32" | "uint32" => 4,
                    "double" | "float64" => 8,
                    "uchar" | "char" | "uint8" | "int8" => 1,
                    "short" | "ushort" | "int16" | "uint16" => 2,
                    other => return Err(format!("property type {other} is not read")),
                };
                if size != 4 && !matches!(*kind, "double" | "float64") {
                    properties.push(((*name).to_string() + "\0ignored", size));
                } else {
                    properties.push((
                        (*name).to_string() + if size == 8 { "\0double" } else { "" },
                        size,
                    ));
                }
            }
            _ => {}
        }
    }
    let count = count.ok_or("the .ply has no vertices")?;
    if count > MOST {
        return Err(format!("{count} splats: at most {MOST} are read"));
    }
    let row: usize = properties.iter().map(|(_, s)| s).sum();
    let body = &bytes[end + b"end_header\n".len()..];
    if body.len() < row * count {
        return Err("the .ply ends before its last splat".into());
    }
    let at = |name: &str| -> Option<(usize, bool)> {
        let mut offset = 0;
        for (n, size) in &properties {
            let (base, double) = match n.split_once('\0') {
                Some((b, "double")) => (b, true),
                Some((_, _)) => ("", false),
                None => (n.as_str(), false),
            };
            if base == name {
                return Some((offset, double));
            }
            offset += size;
        }
        None
    };
    let need = |name: &str| {
        at(name).ok_or_else(|| format!("the .ply has no {name}: not a Gaussian splat file"))
    };
    let place = [need("x")?, need("y")?, need("z")?];
    let dc = [need("f_dc_0")?, need("f_dc_1")?, need("f_dc_2")?];
    let opacity = need("opacity")?;
    let scale = [need("scale_0")?, need("scale_1")?, need("scale_2")?];
    let rot = [
        need("rot_0")?,
        need("rot_1")?,
        need("rot_2")?,
        need("rot_3")?,
    ];
    let mut rest = Vec::new();
    while let Some(field) = at(&format!("f_rest_{}", rest.len())) {
        rest.push(field);
    }
    // 3 colours times 3, 8 or 15 coefficients; anything else is read as degree 0.
    let degree = match rest.len() {
        9 => 1,
        24 => 2,
        n if n >= 45 => 3,
        _ => 0,
    };
    let coefficients = ((degree + 1) * (degree + 1) - 1) as usize;
    let read = |splat: usize, (offset, double): (usize, bool)| -> f32 {
        let i = splat * row + offset;
        if double {
            f64::from_le_bytes(body[i..i + 8].try_into().unwrap_or([0; 8])) as f32
        } else {
            f32::from_le_bytes(body[i..i + 4].try_into().unwrap_or([0; 4]))
        }
    };
    let mut data = vec![0.0f32; count * STRIDE];
    let mut harmonics = vec![0.0f32; count * coefficients * 3];
    for s in 0..count {
        let d = &mut data[s * STRIDE..(s + 1) * STRIDE];
        for k in 0..3 {
            d[k] = read(s, place[k]);
            d[4 + k] = read(s, scale[k]).exp();
            d[12 + k] = (0.5 + SH0 * read(s, dc[k])).max(0.0);
        }
        d[3] = 1.0 / (1.0 + (-read(s, opacity)).exp());
        // The file's rotation is w, x, y, z; kept as x, y, z, w, normalised.
        let q = [
            read(s, rot[1]),
            read(s, rot[2]),
            read(s, rot[3]),
            read(s, rot[0]),
        ];
        let length = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        for k in 0..4 {
            d[8 + k] = q[k] / length;
        }
        // The file holds each colour's coefficients in a run; kept so.
        for c in 0..coefficients * 3 {
            let colour = c / coefficients;
            let k = c % coefficients;
            let field = rest[colour * (rest.len() / 3) + k];
            harmonics[s * coefficients * 3 + c] = read(s, field);
        }
        if d.iter().any(|v| !v.is_finite()) {
            return Err(format!("splat {s} has a value that is not a number"));
        }
    }
    let bounds = bounds(&data, count);
    Ok(Splats {
        count,
        data,
        degree,
        rest: harmonics,
        bounds,
    })
}
/// Reads a .splat file: for each splat its place (three floats), size along its axes
/// (three floats), colour and opacity (four bytes) and turn (four bytes, w first, each
/// 128 + 128 times the quaternion's part). No view-dependent colour.
pub fn parse_splat(bytes: &[u8]) -> Result<Splats, String> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(32) {
        return Err("not a .ply or .splat file (a .splat is 32 bytes a splat)".into());
    }
    let count = bytes.len() / 32;
    if count > MOST {
        return Err(format!("{count} splats: at most {MOST} are read"));
    }
    let float =
        |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let mut data = vec![0.0f32; count * STRIDE];
    for s in 0..count {
        let at = s * 32;
        let d = &mut data[s * STRIDE..(s + 1) * STRIDE];
        for k in 0..3 {
            d[k] = float(at + 4 * k);
            d[4 + k] = float(at + 12 + 4 * k);
            d[12 + k] = f32::from(bytes[at + 24 + k]) / 255.0;
        }
        d[3] = f32::from(bytes[at + 27]) / 255.0;
        let part = |k: usize| (f32::from(bytes[at + 28 + k]) - 128.0) / 128.0;
        let q = [part(1), part(2), part(3), part(0)];
        let length = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        for k in 0..4 {
            d[8 + k] = q[k] / length;
        }
        if d.iter().any(|v| !v.is_finite()) {
            return Err(format!("splat {s} has a value that is not a number"));
        }
    }
    let bounds = bounds(&data, count);
    Ok(Splats {
        count,
        data,
        degree: 0,
        rest: Vec::new(),
        bounds,
    })
}
fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .take(1 << 20)
        .position(|w| w == needle)
}
/// A splat .ply of these splats (place, colour, opacity, size, turn as x y z w), for
/// tests and examples.
pub fn write(splats: &[crate::Described]) -> Vec<u8> {
    let mut out = format!(
        "ply\nformat binary_little_endian 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nend_header\n",
        splats.len()
    )
    .into_bytes();
    for (place, colour, opacity, size, turn) in splats {
        let logit = (opacity / (1.0 - opacity).max(1e-6)).max(1e-9).ln();
        let values = [
            place[0],
            place[1],
            place[2],
            0.0,
            0.0,
            0.0,
            (colour[0] - 0.5) / SH0,
            (colour[1] - 0.5) / SH0,
            (colour[2] - 0.5) / SH0,
            logit,
            size[0].ln(),
            size[1].ln(),
            size[2].ln(),
            turn[3],
            turn[0],
            turn[1],
            turn[2],
        ];
        for v in values {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splats_written_are_read_back() {
        let file = write(&[
            (
                [1.0, 2.0, 3.0],
                [1.0, 0.0, 0.25],
                0.8,
                [0.5, 1.0, 2.0],
                [0.0, 0.0, 0.0, 1.0],
            ),
            (
                [-4.0, 0.0, 9.0],
                [0.2, 0.7, 0.3],
                0.3,
                [1.0, 1.0, 1.0],
                [
                    0.0,
                    std::f32::consts::FRAC_1_SQRT_2,
                    0.0,
                    std::f32::consts::FRAC_1_SQRT_2,
                ],
            ),
        ]);
        let s = parse(&file).unwrap();
        assert_eq!((s.count, s.degree), (2, 0));
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let d = &s.data;
        assert!(close(d[0], 1.0) && close(d[1], 2.0) && close(d[2], 3.0));
        assert!(close(d[3], 0.8), "opacity {}", d[3]);
        assert!(close(d[4], 0.5) && close(d[5], 1.0) && close(d[6], 2.0));
        assert!(close(d[12], 1.0) && close(d[13], 0.0) && close(d[14], 0.25));
        assert!(
            close(d[STRIDE + 9], std::f32::consts::FRAC_1_SQRT_2)
                && close(d[STRIDE + 11], std::f32::consts::FRAC_1_SQRT_2)
        );
        assert!(parse(b"ply\nformat ascii 1.0\nelement vertex 1\nend_header\n").is_err());
        assert!(parse(&file[..file.len() - 10]).is_err(), "cut short");
    }
    #[test]
    fn a_splat_file_is_read() {
        let mut bytes = Vec::new();
        for v in [1.0f32, -2.0, 3.0, 0.5, 0.25, 2.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        // Red, half see-through, turned a quarter about y: w and y at 0.7071.
        bytes.extend_from_slice(&[255, 0, 51, 128, 218, 128, 218, 128]);
        let s = crate::parse(&bytes).unwrap();
        let d = &s.data;
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert_eq!(s.count, 1);
        assert!(close(d[0], 1.0) && close(d[1], -2.0) && close(d[2], 3.0));
        assert!(close(d[4], 0.5) && close(d[5], 0.25) && close(d[6], 2.0));
        assert!(close(d[12], 1.0) && close(d[14], 0.2) && close(d[3], 0.502));
        assert!(
            close(d[9], std::f32::consts::FRAC_1_SQRT_2)
                && close(d[11], std::f32::consts::FRAC_1_SQRT_2),
            "{:?}",
            &d[8..12]
        );
        assert!(crate::parse(&bytes[..31]).is_err(), "not whole splats");
    }
}
