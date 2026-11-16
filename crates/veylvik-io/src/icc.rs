//! Reads the one ICC profile shape PNG import decodes exactly: greyscale with a pure gamma curve.

/// The exponent of a greyscale profile whose grey tone curve (`kTRC`) is a pure power
/// law: a `curv` with one gamma entry (or none, linear) or a `para` function type 0.
/// Any other profile returns None so the caller can refuse it.
pub(crate) fn grey_gamma(profile: &[u8]) -> Option<f32> {
    let u32_at = |at: usize| {
        profile
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    if profile.get(16..20)? != b"GRAY" {
        return None;
    }
    let count = u32_at(128)? as usize;
    if count > 64 {
        return None;
    }
    let (offset, size) = (0..count).find_map(|k| {
        let entry = 132 + 12 * k;
        (profile.get(entry..entry + 4)? == b"kTRC")
            .then(|| Some((u32_at(entry + 4)? as usize, u32_at(entry + 8)? as usize)))?
    })?;
    let curve = profile.get(offset..offset.checked_add(size)?)?;
    let gamma = match curve.get(0..4)? {
        b"curv" => match u32::from_be_bytes(curve.get(8..12)?.try_into().ok()?) {
            0 => 1.0,
            1 => u16::from_be_bytes(curve.get(12..14)?.try_into().ok()?) as f32 / 256.0,
            _ => return None,
        },
        b"para" if curve.get(8..10)? == [0, 0] => {
            i32::from_be_bytes(curve.get(12..16)?.try_into().ok()?) as f32 / 65536.0
        }
        _ => return None,
    };
    (gamma > 0.2 && gamma < 5.0).then_some(gamma)
}
#[cfg(test)]
pub(crate) fn grey_profile(curve: &[u8]) -> Vec<u8> {
    let mut profile = vec![0u8; 128];
    profile[12..16].copy_from_slice(b"mntr");
    profile[16..20].copy_from_slice(b"GRAY");
    profile[36..40].copy_from_slice(b"acsp");
    profile.extend(1u32.to_be_bytes());
    profile.extend(b"kTRC");
    profile.extend(144u32.to_be_bytes());
    profile.extend((curve.len() as u32).to_be_bytes());
    profile.extend(curve);
    let size = profile.len() as u32;
    profile[0..4].copy_from_slice(&size.to_be_bytes());
    profile
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_pure_gamma_grey_profiles_only() {
        let para = [
            b"para".as_slice(),
            &[0; 4],
            &[0, 0, 0, 0],
            &(144179i32).to_be_bytes(),
        ]
        .concat();
        assert!((grey_gamma(&grey_profile(&para)).unwrap() - 2.2).abs() < 0.001);
        let curv = [
            b"curv".as_slice(),
            &[0; 4],
            &1u32.to_be_bytes(),
            &[0x01, 0xCD],
        ]
        .concat();
        assert!((grey_gamma(&grey_profile(&curv)).unwrap() - 1.8).abs() < 0.01);
        let linear = [b"curv".as_slice(), &[0; 4], &0u32.to_be_bytes()].concat();
        assert_eq!(grey_gamma(&grey_profile(&linear)), Some(1.0));
        // A sampled curve, an RGB profile and truncated data are refused.
        let table = [b"curv".as_slice(), &[0; 4], &2u32.to_be_bytes(), &[0; 4]].concat();
        assert_eq!(grey_gamma(&grey_profile(&table)), None);
        let mut rgb = grey_profile(&para);
        rgb[16..20].copy_from_slice(b"RGB ");
        assert_eq!(grey_gamma(&rgb), None);
        assert_eq!(grey_gamma(&grey_profile(&para)[..150]), None);
    }
}
