//! Adobe/Resolve `.cube` 3D LUTs as images the renderer can sample. A LUT of size N becomes
//! N² × (N + 1) pixels: cell (r, g, b) at x = r + N·g, y = b, red fastest as in the file, and a
//! last row holding the domain minimum, maximum and size. Parsing is strict and bounded.
use crate::Result;
use std::{io::Read, path::Path};
use veylvik_render::Image;

/// Largest supported LUT size; 65 is the common maximum.
pub const MAX_SIZE: usize = 65;
const MAX_FILE: u64 = 32 * 1024 * 1024;

pub fn is_lut(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cube"))
}
pub fn load(path: &Path) -> Result<Image> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("a LUT must be a regular file".into());
    }
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE {
        return Err("LUT file exceeds 32 MiB".into());
    }
    parse(&text)
}
pub fn parse(text: &str) -> Result<Image> {
    let (mut size, mut min, mut max) = (None, [0.0f32; 3], [1.0f32; 3]);
    let mut cells: Vec<[f32; 4]> = Vec::new();
    let triple = |words: &[&str], line: usize| -> Result<[f32; 3]> {
        let values: Vec<f32> = words
            .iter()
            .map(|w| w.parse::<f32>())
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| format!("LUT line {line}: expected three numbers"))?;
        let values: [f32; 3] = values
            .try_into()
            .map_err(|_| format!("LUT line {line}: expected three numbers"))?;
        if values.iter().any(|v| !v.is_finite()) {
            return Err(format!("LUT line {line}: values must be finite").into());
        }
        Ok(values)
    };
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let words: Vec<&str> = content.split_whitespace().collect();
        match words[0] {
            "TITLE" => {}
            "LUT_1D_SIZE" => {
                return Err("1D LUTs are not supported; use a 3D .cube LUT".into());
            }
            "LUT_3D_SIZE" => {
                let n: usize = words
                    .get(1)
                    .and_then(|w| w.parse().ok())
                    .ok_or(format!("LUT line {line}: LUT_3D_SIZE needs a number"))?;
                if !(2..=MAX_SIZE).contains(&n) {
                    return Err(format!("LUT size must be 2 to {MAX_SIZE}, not {n}").into());
                }
                size = Some(n);
                cells.reserve(n * n * n);
            }
            "DOMAIN_MIN" => min = triple(&words[1..], line)?,
            "DOMAIN_MAX" => max = triple(&words[1..], line)?,
            first if first.starts_with(|c: char| c.is_ascii_alphabetic()) => {
                // Other keywords (vendor metadata) carry no colour data.
            }
            _ => {
                let n = size.ok_or("LUT data comes before LUT_3D_SIZE")?;
                if cells.len() == n * n * n {
                    return Err(format!("LUT line {line}: more than {n}³ entries").into());
                }
                let [r, g, b] = triple(&words, line)?;
                cells.push([r, g, b, 1.0]);
            }
        }
    }
    let n = size.ok_or("not a 3D .cube LUT: LUT_3D_SIZE is missing")?;
    if cells.len() != n * n * n {
        return Err(format!("LUT has {} entries, expected {}", cells.len(), n * n * n).into());
    }
    if (0..3).any(|c| max[c] <= min[c]) {
        return Err("LUT DOMAIN_MAX must exceed DOMAIN_MIN".into());
    }
    let width = n * n;
    // Rows by blue; within a row red runs fastest, then green: the file's own order.
    let mut pixels = cells;
    let mut meta = vec![[0.0, 0.0, 0.0, 1.0]; width];
    meta[0] = [min[0], min[1], min[2], 1.0];
    meta[1] = [max[0], max[1], max[2], 1.0];
    meta[2] = [n as f32, 0.0, 0.0, 1.0];
    pixels.extend(meta);
    Ok(Image::new(width as u32, (n + 1) as u32, pixels)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cube_files_become_a_sampled_layout_with_domain_and_size() {
        let mut text = String::from(
            "TITLE \"t\"\n# comment\nLUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 2\n",
        );
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    text.push_str(&format!("{r} {g} {}\n", b as f32 * 0.5));
                }
            }
        }
        let image = parse(&text).unwrap();
        assert_eq!((image.width(), image.height()), (4, 3));
        let at = |x: usize, y: usize| image.pixels()[y * 4 + x];
        // Cell (r=1, g=1, b=1) at x = 1 + 2·1, y = 1.
        assert_eq!(at(3, 1), [1.0, 1.0, 0.5, 1.0]);
        assert_eq!(at(1, 0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(at(0, 2), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(at(1, 2), [1.0, 1.0, 2.0, 1.0]);
        assert_eq!(at(2, 2)[0], 2.0);
        for bad in [
            "LUT_1D_SIZE 4\n",
            "LUT_3D_SIZE 2\n0 0 0\n",
            "0 0 0\n",
            "LUT_3D_SIZE 99\n",
            "LUT_3D_SIZE 2\nDOMAIN_MIN 1 1 1\nDOMAIN_MAX 0 0 0\n",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        let mut long = text.clone();
        long.push_str("0 0 0\n");
        assert!(parse(&long).is_err());
    }
}
