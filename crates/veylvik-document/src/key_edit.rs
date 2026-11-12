//! Non-destructive key selection operations. Commit the returned scalar through
//! SetParameter so desktop and MCP clients share validation and atomic history.
use crate::{Key, Result, Scalar, Time, fail};
use std::collections::BTreeSet;

fn selected<'a>(scalar: &'a Scalar, indices: &BTreeSet<usize>) -> Result<&'a [Key]> {
    scalar.validate()?;
    let keys = scalar
        .keys()
        .ok_or_else(|| crate::Error("no keyframes".into()))?;
    if indices.is_empty() || indices.iter().any(|i| *i >= keys.len()) {
        return fail("select existing keyframes first");
    }
    Ok(keys)
}
/// Move selected keys by an exact time offset and a value offset. Never overwrite
/// an unselected key; duplicate times and non-finite values are errors.
pub fn moved(
    scalar: &Scalar,
    indices: &BTreeSet<usize>,
    offset: Time,
    value_offset: f64,
) -> Result<Scalar> {
    selected(scalar, indices)?;
    offset.seconds()?;
    if offset.ticks == 0 && value_offset == 0.0 {
        return Ok(scalar.clone());
    }
    let mut result = scalar.clone();
    let keys = result.keys_mut().unwrap();
    for index in indices {
        keys[*index].time = keys[*index].time.checked_add(offset)?;
        keys[*index].value = (f64::from(keys[*index].value) + value_offset) as f32;
    }
    keys.sort_by(|a, b| a.time.compare(b.time).expect("validated time"));
    result.validate()?;
    Ok(result)
}
/// Copy keys relative to the first selected key, preserving subframe spacing.
pub fn copied(scalar: &Scalar, indices: &BTreeSet<usize>) -> Result<Vec<Key>> {
    let keys = selected(scalar, indices)?;
    let first = keys[*indices.first().unwrap()].time;
    indices
        .iter()
        .map(|i| {
            Ok(Key {
                time: keys[*i].time.checked_sub(first)?,
                value: keys[*i].value,
            })
        })
        .collect()
}
/// Paste relative keys at an exact playhead time, rejecting collisions as a unit.
/// Existing curves retain their interpolation; constants become linear curves.
pub fn pasted(scalar: &Scalar, clipboard: &[Key], at: Time) -> Result<Scalar> {
    scalar.validate()?;
    if clipboard.is_empty() {
        return fail("clipboard has no keys");
    }
    let mut result = if scalar.keys().is_some() {
        scalar.clone()
    } else {
        Scalar::Linear { keys: vec![] }
    };
    let keys = result.keys_mut().unwrap();
    if keys.len().saturating_add(clipboard.len()) > crate::animation::MAX_KEYS {
        return fail("too many keyframes");
    }
    for key in clipboard {
        keys.push(Key {
            time: at.checked_add(key.time)?,
            value: key.value,
        });
    }
    keys.sort_by(|a, b| a.time.compare(b.time).expect("validated time"));
    result.validate()?;
    Ok(result)
}
/// Deleting all keys leaves a constant evaluated at the current playhead.
pub fn deleted(scalar: &Scalar, indices: &BTreeSet<usize>, at: Time) -> Result<Scalar> {
    selected(scalar, indices)?;
    let mut result = scalar.clone();
    let keys = result.keys_mut().unwrap();
    for i in indices.iter().rev() {
        keys.remove(*i);
    }
    if keys.is_empty() {
        result = scalar.at(at)?.into();
    }
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn curve() -> Scalar {
        Scalar::Smooth {
            keys: [0, 1, 2]
                .map(|ticks| Key {
                    time: Time::new(ticks, 3).unwrap(),
                    value: ticks as f32,
                })
                .into(),
        }
    }
    #[test]
    fn moves_and_clipboard_preserve_subframes_and_reject_collisions() {
        let source = curve();
        let one = [0].into();
        assert!(moved(&source, &one, Time::new(1, 3).unwrap(), 0.0).is_err());
        let moved = moved(&source, &[0, 1].into(), Time::new(-1, 6).unwrap(), 0.5).unwrap();
        assert_eq!(
            moved.keys().unwrap()[0]
                .time
                .compare(Time::new(-1, 6).unwrap())
                .unwrap(),
            std::cmp::Ordering::Equal
        );
        assert_eq!(moved.keys().unwrap()[1].value, 1.5);
        assert_eq!(moved.interpolation(), source.interpolation());
        let clip = copied(&source, &[1, 2].into()).unwrap();
        let pasted = pasted(&source, &clip, Time::new(1, 1).unwrap()).unwrap();
        assert_eq!(pasted.keys().unwrap().len(), 5);
        assert_eq!(
            pasted.keys().unwrap()[4]
                .time
                .compare(Time::new(4, 3).unwrap())
                .unwrap(),
            std::cmp::Ordering::Equal
        );
        assert!(super::pasted(&source, &clip, Time::new(0, 1).unwrap()).is_err());
        assert!(copied(&source, &[99].into()).is_err());
        assert!(super::moved(&source, &one, Time::new(0, 1).unwrap(), f64::INFINITY).is_err());
        assert_eq!(source.keys().unwrap().len(), 3);
    }
    #[test]
    fn exact_time_arithmetic_and_delete_all() {
        let min = Time::new(i64::MIN, 1).unwrap();
        assert_eq!(min.checked_sub(min).unwrap().ticks, 0);
        assert!(min.checked_add(Time::new(-1, 1).unwrap()).is_err());
        assert!(Time { ticks: 1, rate: 0 }.checked_add(min).is_err());
        let t = Time::at_frame(3, Time::new(30000, 1001).unwrap()).unwrap();
        assert_eq!(
            t.checked_sub(Time::at_frame(1, Time::new(30000, 1001).unwrap()).unwrap())
                .unwrap()
                .compare(Time::new(1001, 15000).unwrap())
                .unwrap(),
            std::cmp::Ordering::Equal
        );
        let source = curve();
        let at = Time::new(1, 6).unwrap();
        assert_eq!(
            deleted(&source, &[0, 1, 2].into(), at)
                .unwrap()
                .at(at)
                .unwrap(),
            source.at(at).unwrap()
        );
    }
}
