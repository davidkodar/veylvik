//! Match the shot in 3D: from where the footage's light comes (`ShotLight`, or a light source seen in the frame) and the camera that films it, a sun's direction, colour and strength, and the environment's level.
use crate::{Camera, math};
use veylvik_document::optical::ShotLight;

/// A key light that matches a shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotMatch {
    /// The sun's placement rotation (degrees), shining along its -z.
    pub rotation: [f32; 3],
    pub colour: [f32; 3],
    pub intensity: f32,
    /// For the scene's Environment.
    pub environment: f32,
    /// False when the shot's light is soft and even (a soft light from above).
    pub directional: bool,
    /// True when the light comes from a source seen beyond the subject.
    pub backlight: bool,
}
/// Rotation (degrees) that makes a light shine along `travel` (unit, world).
pub fn shining(travel: [f32; 3]) -> [f32; 3] {
    let x = travel[1].clamp(-1.0, 1.0).asin();
    let y = (-travel[0]).atan2(-travel[2]);
    [x.to_degrees(), y.to_degrees(), 0.0]
}
/// Where a light source shows in the frame (pixels): the centroid of the brightest
/// 0.5% of a coarse grid within 2% of the frame's peak (a sun, lamp or
/// window, usually clipped), when it sits in the upper half, where sources are,
/// rather than on bright surfaces below them.
pub fn source_in_frame(pixels: &[[f32; 4]], width: u32, height: u32) -> Option<[f32; 2]> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || pixels.len() != w * h {
        return None;
    }
    let gw = w.min(96);
    let gh = (h * gw / w).max(1);
    let mut cells = Vec::with_capacity(gw * gh);
    for gy in 0..gh {
        for gx in 0..gw {
            let (x0, x1) = (gx * w / gw, ((gx + 1) * w / gw).max(gx * w / gw + 1));
            let (y0, y1) = (gy * h / gh, ((gy + 1) * h / gh).max(gy * h / gh + 1));
            let mut sum = 0.0f64;
            for y in y0..y1 {
                for p in &pixels[y * w + x0..y * w + x1] {
                    sum +=
                        f64::from(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).clamp(0.0, 1.0e4);
                }
            }
            let l = (sum / ((x1 - x0) * (y1 - y0)) as f64) as f32;
            cells.push((l, [(x0 + x1) as f32 * 0.5, (y0 + y1) as f32 * 0.5]));
        }
    }
    cells.sort_by(|a, b| b.0.total_cmp(&a.0));
    let peak = cells.first()?.0;
    if peak <= 1.0e-6 {
        return None;
    }
    let top: Vec<_> = cells
        .iter()
        .take((cells.len() / 200).max(1))
        .filter(|c| c.0 >= peak * 0.98)
        .collect();
    let n = top.len() as f32;
    let at = [
        top.iter().map(|c| c.1[0]).sum::<f32>() / n,
        top.iter().map(|c| c.1[1]).sum::<f32>() / n,
    ];
    (at[1] < height as f32 * 0.5).then_some(at)
}
/// The key light for `shot`, filmed by `camera` at `width` x `height`. A light source
/// seen in the frame (`source`, pixels) is a sun beyond the subject, along the
/// camera's ray through it. Otherwise the light comes from the bright side of the
/// frame, a little in front of the subject. Either way it stays above the horizon
/// (15 degrees, or 25 for a source seen in the frame).
pub fn match_shot(
    shot: &ShotLight,
    source: Option<[f32; 2]>,
    camera: &Camera,
    width: u32,
    height: u32,
) -> ShotMatch {
    let backlight = source.is_some();
    let directional = shot.contrast >= 1.3 || backlight;
    let nx = (shot.position[0] / width.max(1) as f32 - 0.5) * 2.0;
    let ny = (shot.position[1] / height.max(1) as f32 - 0.5) * 2.0;
    let f = crate::home_distance(width, camera.focal_length, camera.sensor_width);
    // Towards the light, in the camera's frame (x right, y down, looking along -z).
    let toward = match source {
        Some([x, y]) => [x - width as f32 * 0.5, y - height as f32 * 0.5, -f],
        None if directional => [nx, ny.min(0.3) - 0.6, 0.7],
        None => [0.0, -1.0, 0.3],
    };
    let mut l = math::direction(&camera.world, toward);
    let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt().max(1e-9);
    l = l.map(|v| v / n);
    // Keep it above the horizon (y is down): at least 15 degrees, and 25 for a source
    // seen in the frame, whose glare usually means the sun is at or above its edge.
    let lowest = if backlight { 0.42_f32 } else { 0.25 };
    if l[1] > -lowest {
        let flat = (l[0] * l[0] + l[2] * l[2]).sqrt().max(1e-9);
        let k = (1.0 - lowest * lowest).sqrt() / flat;
        l = [l[0] * k, -lowest, l[2] * k];
    }
    let strength = 1.0 + 0.5 * shot.contrast.min(10.0);
    ShotMatch {
        rotation: shining(l.map(|v| -v)),
        colour: shot.colour,
        intensity: match (directional, backlight) {
            (true, true) => strength.clamp(2.5, 6.0),
            (true, false) => strength.clamp(1.5, 6.0),
            _ => 1.0,
        },
        environment: (0.65 * shot.mean / 0.18).clamp(0.05, 4.0),
        directional,
        backlight,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn travel(rotation: [f32; 3]) -> [f32; 3] {
        math::direction(&math::rotate(rotation), [0.0, 0.0, -1.0])
    }
    #[test]
    fn a_bright_source_high_in_the_frame_is_found_but_bright_surfaces_below_are_not() {
        let (w, h) = (192u32, 108u32);
        let mut pixels = vec![[0.2f32, 0.2, 0.2, 1.0]; (w * h) as usize];
        let mut paint = |x0: u32, y0: u32, size: u32, v: f32| {
            for y in y0..y0 + size {
                for x in x0..x0 + size {
                    pixels[(y * w + x) as usize] = [v, v, v, 1.0];
                }
            }
        };
        // A large bright surface low in the frame, and a small sun at the top right.
        paint(40, 60, 40, 0.95);
        paint(150, 12, 8, 1.0);
        let at = source_in_frame(&pixels, w, h).unwrap();
        assert!(
            (at[0] - 154.0).abs() < 4.0 && (at[1] - 16.0).abs() < 4.0,
            "{at:?}"
        );
        // Only a bright surface low in the frame: no source.
        let mut low = vec![[0.2f32, 0.2, 0.2, 1.0]; (w * h) as usize];
        for y in 70..100 {
            for x in 40..80 {
                low[(y * w + x) as usize] = [1.0; 4];
            }
        }
        assert!(source_in_frame(&low, w, h).is_none());
    }
    #[test]
    fn the_sun_shines_from_the_bright_side_or_from_a_seen_source_and_from_above() {
        let camera = crate::home_camera(1920, 1080);
        let shot = |x: f32, y: f32, contrast: f32| ShotLight {
            position: [x, y],
            colour: [1.0, 0.9, 0.7],
            contrast,
            mean: 0.18,
        };
        // Bright at the right: light travels leftwards, downwards, away from the camera.
        let m = match_shot(&shot(1700.0, 500.0, 4.0), None, &camera, 1920, 1080);
        let t = travel(m.rotation);
        assert!(m.directional && !m.backlight);
        assert!(t[0] < -0.3 && t[1] > 0.2 && t[2] < 0.0, "{t:?}");
        // A source seen high at the right: a sun beyond the subject, shining back
        // towards the camera, down and to the left.
        let m = match_shot(
            &shot(960.0, 700.0, 2.0),
            Some([1500.0, 150.0]),
            &camera,
            1920,
            1080,
        );
        let t = travel(m.rotation);
        assert!(m.backlight && m.directional);
        assert!(t[2] > 0.3 && t[0] < 0.0 && t[1] > 0.2, "{t:?}");
        // Flat light: a soft light from above.
        let m = match_shot(&shot(960.0, 540.0, 1.1), None, &camera, 1920, 1080);
        assert!(!m.directional && travel(m.rotation)[1] > 0.9);
        assert!((m.environment - 0.65).abs() < 1e-4);
        // The rotation round-trips any direction.
        for t in [[0.3_f32, 0.8, -0.52], [-0.6, 0.5, 0.62]] {
            let n = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
            let t = t.map(|v| v / n);
            let back = travel(shining(t));
            assert!(
                (0..3).all(|k| (back[k] - t[k]).abs() < 1e-4),
                "{back:?} vs {t:?}"
            );
        }
    }
}
