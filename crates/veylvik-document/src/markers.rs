//! Named points in time on the composition: at most 256, in strictly rising time, with short plain labels. They guide the editor and never change a render.
use crate::{Result, Time, fail};
use serde::{Deserialize, Serialize};

/// Markers a composition may hold.
pub const MAX_MARKERS: usize = 256;
/// Characters in a marker's label.
pub const MAX_LABEL: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub time: Time,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}
/// Whether `a` is strictly before `b`, compared exactly.
fn before(a: Time, b: Time) -> bool {
    i128::from(a.ticks) * i128::from(b.rate) < i128::from(b.ticks) * i128::from(a.rate)
}
pub fn validate(markers: &[Marker]) -> Result<()> {
    if markers.len() > MAX_MARKERS {
        return fail("a composition holds at most 256 markers");
    }
    for marker in markers {
        if !(0.0..=86400.0).contains(&marker.time.seconds()?) {
            return fail("marker times must be within 0..86400 seconds");
        }
        if marker.label.chars().count() > MAX_LABEL || marker.label.chars().any(char::is_control) {
            return fail("a marker label is at most 80 characters without control characters");
        }
    }
    if markers
        .windows(2)
        .any(|pair| !before(pair[0].time, pair[1].time))
    {
        return fail("markers must be in rising time, one at each time");
    }
    Ok(())
}
/// `markers` with one at `time` added, or relabelled when one is there already.
pub fn with(markers: &[Marker], time: Time, label: &str) -> Vec<Marker> {
    let mut next: Vec<Marker> = markers.to_vec();
    let at = next.partition_point(|m| before(m.time, time));
    match next.get_mut(at) {
        Some(marker) if !before(time, marker.time) => marker.label = label.to_owned(),
        _ => next.insert(
            at,
            Marker {
                time,
                label: label.to_owned(),
            },
        ),
    }
    next
}
/// `markers` without the one at `time`.
pub fn without(markers: &[Marker], time: Time) -> Vec<Marker> {
    markers
        .iter()
        .filter(|m| before(m.time, time) || before(time, m.time))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Document, blank};
    fn t(ticks: i64, rate: u32) -> Time {
        Time::new(ticks, rate).unwrap()
    }
    #[test]
    fn markers_stay_ordered_bounded_and_undo_with_the_document() {
        let mut list = with(&[], t(2, 1), "Drop");
        list = with(&list, t(1, 2), "Intro");
        list = with(&list, t(5, 1), "");
        // The same instant written another way relabels and does not duplicate.
        list = with(&list, t(4, 2), "Beat");
        assert_eq!(
            list.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            ["Intro", "Beat", ""]
        );
        validate(&list).unwrap();
        assert_eq!(without(&list, t(10, 5)).len(), 2);
        assert_eq!(without(&list, t(3, 1)).len(), 3);
        // Refused: out of order, a duplicate, a long or control-character label, too many.
        let mut bad = list.clone();
        bad.swap(0, 1);
        assert!(validate(&bad).is_err());
        let mut twice = list.clone();
        twice[1].time = t(1, 2);
        assert!(validate(&twice).is_err());
        assert!(validate(&with(&[], t(0, 1), &"x".repeat(81))).is_err());
        assert!(validate(&with(&[], t(0, 1), "a\nb")).is_err());
        assert!(validate(&with(&[], t(-1, 1), "")).is_err());
        let many: Vec<Marker> = (0..257)
            .map(|i| Marker {
                time: t(i, 24),
                label: String::new(),
            })
            .collect();
        assert!(validate(&many).is_err());
        validate(&many[..256]).unwrap();
        // Through the document: one undo step, and a project without markers saves as before.
        let blank = blank(64, 36, t(24, 1)).unwrap();
        let plain = serde_json::to_string(&blank).unwrap();
        assert!(!plain.contains("markers"));
        let mut document = Document::new(blank).unwrap();
        document
            .apply(
                document.revision(),
                &[Command::Markers {
                    markers: list.clone(),
                }],
            )
            .unwrap();
        assert_eq!(document.snapshot().markers.len(), 3);
        let saved = serde_json::to_string(document.snapshot()).unwrap();
        let back: crate::Composition = serde_json::from_str(&saved).unwrap();
        assert_eq!(back.markers[1].label, "Beat");
        let revision = document.revision();
        assert!(
            document
                .apply(revision, &[Command::Markers { markers: bad }])
                .is_err()
        );
        assert_eq!(document.snapshot().markers.len(), 3);
        document.undo(document.revision()).unwrap();
        assert!(document.snapshot().markers.is_empty());
        assert_eq!(serde_json::to_string(document.snapshot()).unwrap(), plain);
    }
}
