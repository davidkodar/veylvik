//! Bounded SDR previews, area-filtered in linear premultiplied color.
use crate::Result;
pub const MAX_PREVIEW_EDGE: u32 = 2048;
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
}
pub fn thumbnail(width: u32, height: u32, pixels: &[[f32; 4]], max_edge: u32) -> Result<Thumbnail> {
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || pixels.len() as u64 != width as u64 * height as u64
        || !(1..=MAX_PREVIEW_EDGE).contains(&max_edge)
    {
        return Err(
            "preview needs matching dimensions within 1..8192 and max_edge within 1..2048".into(),
        );
    }
    if pixels.iter().any(|pixel| {
        pixel.iter().any(|value| !value.is_finite()) || !(0.0..=1.0).contains(&pixel[3])
    }) {
        return Err("preview needs finite pixels and alpha in [0,1]".into());
    }
    let longest = width.max(height);
    if longest <= max_edge {
        return sendable(Thumbnail {
            width,
            height,
            png: crate::encode_png(width, height, pixels)?,
        });
    }
    let out_width =
        ((width as u64 * max_edge as u64 + longest as u64 / 2) / longest as u64).max(1) as u32;
    let out_height =
        ((height as u64 * max_edge as u64 + longest as u64 / 2) / longest as u64).max(1) as u32;
    let sx = width as f64 / out_width as f64;
    let sy = height as f64 / out_height as f64;
    let mut filtered = Vec::with_capacity(out_width as usize * out_height as usize);
    for y in 0..out_height {
        let top = y as f64 * sy;
        let bottom = (y + 1) as f64 * sy;
        for x in 0..out_width {
            let left = x as f64 * sx;
            let right = (x + 1) as f64 * sx;
            let mut sum = [0.0_f64; 4];
            let mut weight = 0.0;
            for iy in top.floor() as u32..(bottom.ceil() as u32).min(height) {
                let wy = (bottom.min((iy + 1) as f64) - top.max(iy as f64)).max(0.0);
                for ix in left.floor() as u32..(right.ceil() as u32).min(width) {
                    let area = wy * (right.min((ix + 1) as f64) - left.max(ix as f64)).max(0.0);
                    let pixel = pixels[(iy * width + ix) as usize];
                    for channel in 0..4 {
                        sum[channel] += pixel[channel] as f64 * area;
                    }
                    weight += area;
                }
            }
            filtered.push(
                sum.map(|value| (value / weight).clamp(-(f32::MAX as f64), f32::MAX as f64) as f32),
            );
        }
    }
    sendable(Thumbnail {
        width: out_width,
        height: out_height,
        png: crate::encode_png(out_width, out_height, &filtered)?,
    })
}
/// The most a preview's PNG may weigh: encoded as text it must fit one control message.
pub const MAX_PREVIEW_BYTES: usize = 2_900_000;
fn sendable(thumbnail: Thumbnail) -> Result<Thumbnail> {
    if thumbnail.png.len() > MAX_PREVIEW_BYTES {
        return Err(format!(
            "this preview is {} bytes, more than one message holds ({MAX_PREVIEW_BYTES}): ask for a smaller max_edge or a region",
            thumbnail.png.len()
        )
        .into());
    }
    Ok(thumbnail)
}
