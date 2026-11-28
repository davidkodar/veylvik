//! Cameras that come with a model file (USD, FBX), kept on its 3D model node as keys read on import: in the model's own normalised frame (centred, largest side 1, y down), so the node's size and placement carry them with the model and its clip plays them.
use crate::*;
use serde::{Deserialize, Serialize};

/// The most cameras one model brings.
pub const MAX_MODEL_CAMERAS: usize = 16;
/// The most keys all of a model's cameras hold together.
pub const MAX_CAMERA_KEYS: usize = 200_000;

/// A camera of a model file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCamera {
    pub name: String,
    /// Where it is and how it sees, at moments of the model's clip (one key: still).
    pub keys: Vec<CameraKey>,
}
/// A model camera at a moment: seconds into the clip; its place in the model's frame
/// (largest side 1); its turn as a quaternion x y z w (it looks along its -z, y down);
/// its lens in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraKey {
    pub seconds: f32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub focal_length: f32,
    pub sensor_width: f32,
}
impl ModelCamera {
    /// Where it is at `seconds` into the clip: straight between the keys either side (the
    /// turn along the shorter way), held before the first and after the last.
    pub fn at(&self, seconds: f32) -> Option<CameraKey> {
        let keys = &self.keys;
        let first = *keys.first()?;
        if keys.len() == 1 || seconds <= first.seconds {
            return Some(first);
        }
        let last = *keys.last()?;
        if seconds >= last.seconds {
            return Some(last);
        }
        let k = keys
            .partition_point(|k| k.seconds <= seconds)
            .saturating_sub(1);
        let (a, b) = (keys[k], keys[(k + 1).min(keys.len() - 1)]);
        let s = ((seconds - a.seconds) / (b.seconds - a.seconds).max(1e-9)).clamp(0.0, 1.0);
        let lerp = |x: f32, y: f32| x + (y - x) * s;
        let dot: f32 = (0..4).map(|i| a.rotation[i] * b.rotation[i]).sum();
        let sign = if dot < 0.0 { -1.0 } else { 1.0 };
        let mut q = [0, 1, 2, 3].map(|i| lerp(a.rotation[i], sign * b.rotation[i]));
        let l = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        q = q.map(|v| v / l);
        Some(CameraKey {
            seconds,
            position: [0, 1, 2].map(|i| lerp(a.position[i], b.position[i])),
            rotation: q,
            focal_length: lerp(a.focal_length, b.focal_length),
            sensor_width: lerp(a.sensor_width, b.sensor_width),
        })
    }
}
/// Checks a model's cameras: at most [`MAX_MODEL_CAMERAS`] holding at most
/// [`MAX_CAMERA_KEYS`] keys, each finite, rising in time, with a lens.
pub fn validate_cameras(cameras: &[ModelCamera]) -> Result<()> {
    if cameras.len() > MAX_MODEL_CAMERAS {
        return fail(format!(
            "A model brings at most {MAX_MODEL_CAMERAS} cameras"
        ));
    }
    if cameras.iter().map(|c| c.keys.len()).sum::<usize>() > MAX_CAMERA_KEYS {
        return fail(format!(
            "A model's cameras hold at most {MAX_CAMERA_KEYS} keys"
        ));
    }
    for camera in cameras {
        if camera.name.len() > 256 || camera.keys.is_empty() {
            return fail("A model camera needs keys and a name of at most 256 bytes");
        }
        let mut before = f32::NEG_INFINITY;
        for k in &camera.keys {
            let numbers = k.position.iter().chain(&k.rotation).chain([
                &k.seconds,
                &k.focal_length,
                &k.sensor_width,
            ]);
            if numbers.clone().any(|v| !v.is_finite())
                || k.seconds <= before
                || k.focal_length <= 0.0
                || k.sensor_width <= 0.0
            {
                return fail(format!(
                    "Model camera {:?} has a key that is not finite, not later than the one before, or without a lens",
                    camera.name
                ));
            }
            before = k.seconds;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(seconds: f32, x: f32, turn: f32) -> CameraKey {
        let (s, c) = (turn * 0.5).sin_cos();
        CameraKey {
            seconds,
            position: [x, 0.0, 0.0],
            rotation: [0.0, s, 0.0, c],
            focal_length: 35.0,
            sensor_width: 36.0,
        }
    }
    #[test]
    fn a_camera_moves_and_turns_straight_between_its_keys() {
        let camera = ModelCamera {
            name: "Shot".into(),
            keys: vec![key(0.0, 0.0, 0.0), key(2.0, 4.0, 1.0)],
        };
        let half = camera.at(1.0).unwrap();
        assert!((half.position[0] - 2.0).abs() < 1e-6);
        let (s, c) = 0.25f32.sin_cos();
        assert!((half.rotation[1] - s).abs() < 1e-3 && (half.rotation[3] - c).abs() < 1e-3);
        assert_eq!(camera.at(-1.0).unwrap().position, [0.0; 3]);
        assert_eq!(camera.at(9.0).unwrap().position, [4.0, 0.0, 0.0]);
        assert!(validate_cameras(std::slice::from_ref(&camera)).is_ok());
        let mut backwards = camera;
        backwards.keys.reverse();
        assert!(validate_cameras(&[backwards]).is_err());
    }
}
