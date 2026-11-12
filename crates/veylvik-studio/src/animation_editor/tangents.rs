use super::*;
use veylvik_document::{Handle, Segment};

pub(super) fn label(mode: Segment) -> &'static str {
    match mode {
        Segment::Linear => "Linear",
        Segment::Hold => "Hold",
        Segment::Smooth => "Smooth",
        Segment::Auto => "Auto",
        Segment::Bezier { .. } => "Bézier",
    }
}
impl Editor {
    pub(super) fn segment_controls(
        &mut self,
        ui: &mut egui::Ui,
        doc: &Composition,
        revision: u64,
        actions: &mut Vec<Action>,
    ) {
        let selected = self.selections();
        let modes: Vec<_> = selected
            .iter()
            .flat_map(|((id, p), indices)| {
                indices.iter().filter_map(|index| {
                    doc.nodes[id]
                        .operation
                        .scalar(p)
                        .unwrap()
                        .segment_mode(*index)
                })
            })
            .collect();
        let Some(current) = modes.first().copied() else {
            return;
        };
        let same = modes.iter().all(|mode| label(*mode) == label(current));
        ui.label("Selected outgoing segments");
        let mut chosen = None;
        egui::ComboBox::from_id_salt("segment_easing")
            .selected_text(if same { label(current) } else { "Mixed" })
            .show_ui(ui, |ui| {
                for mode in [
                    Segment::Auto,
                    Segment::Smooth,
                    Segment::Linear,
                    Segment::Hold,
                    Segment::Bezier {
                        outgoing: Handle {
                            time: 1.0 / 3.0,
                            value: 0.0,
                        },
                        incoming: Handle {
                            time: 1.0 / 3.0,
                            value: 0.0,
                        },
                    },
                ] {
                    if ui
                        .selectable_label(label(mode) == label(current), label(mode))
                        .clicked()
                    {
                        chosen = Some(mode);
                    }
                }
            });
        let inherit = ui
            .small_button("Use curve default")
            .on_hover_text("Remove per-segment overrides from selected keys")
            .clicked();
        if chosen.is_some() || inherit {
            let edits = selected
                .iter()
                .map(|((id, p), indices)| {
                    let scalar = doc.nodes[id].operation.scalar(p).unwrap();
                    let mut edited = scalar.clone();
                    for index in indices {
                        if scalar.segment_mode(*index).is_none() {
                            continue;
                        }
                        let mode = match chosen {
                            Some(Segment::Bezier { .. }) => Some(scalar.bezier_segment(*index)?),
                            other => other,
                        };
                        edited.keys_mut().unwrap()[*index].segment = mode;
                    }
                    edited.validate()?;
                    Ok((*id, p.clone(), edited))
                })
                .collect();
            self.batch(edits, doc, revision, actions);
            if self.error.is_none() {
                self.pending_group = Some(selected);
            }
        }
    }
}
/// Visible handles for segments touching a selected key. Each handle identifies
/// the outgoing segment's key, and whether it is the incoming/right control.
pub(super) fn controls(
    scalar: &Scalar,
    selection: &BTreeSet<usize>,
    graph: Rect,
    range: [f64; 2],
    fps: Time,
    min: f64,
    max: f64,
) -> Vec<(usize, bool, Pos2, Pos2)> {
    let Some(keys) = scalar.keys() else {
        return vec![];
    };
    let mut result = Vec::new();
    for (index, pair) in keys.windows(2).enumerate() {
        if !selection.contains(&index) && !selection.contains(&(index + 1)) {
            continue;
        }
        let Some(Segment::Bezier { outgoing, incoming }) = pair[0].segment else {
            continue;
        };
        let a = frame_number(pair[0].time, fps);
        let b = frame_number(pair[1].time, fps);
        for (right, handle, endpoint, x) in [
            (
                false,
                outgoing,
                &pair[0],
                a + (b - a) * f64::from(outgoing.time),
            ),
            (
                true,
                incoming,
                &pair[1],
                b - (b - a) * f64::from(incoming.time),
            ),
        ] {
            let point = |frame, value| {
                Pos2::new(
                    x_at(frame, graph, range),
                    graph.bottom() - ((value - min) / (max - min)) as f32 * graph.height(),
                )
            };
            result.push((
                index,
                right,
                point(x, f64::from(endpoint.value) + f64::from(handle.value)),
                point(frame_number(endpoint.time, fps), f64::from(endpoint.value)),
            ));
        }
    }
    result
}
pub(super) fn moved_handle(
    original: &Scalar,
    handle: (usize, bool),
    delta: Vec2,
    graph: Rect,
    range: [f64; 2],
    fps: Time,
    bounds: [f64; 2],
) -> veylvik_document::Result<Scalar> {
    let (index, right) = handle;
    let [min, max] = bounds;
    let mut edited = original.clone();
    let keys = edited
        .keys_mut()
        .ok_or_else(|| veylvik_document::Error("no keys".into()))?;
    let duration = frame_number(keys[index + 1].time, fps) - frame_number(keys[index].time, fps);
    if duration <= 0.0 {
        return Err(veylvik_document::Error(
            "Use exact inspector times for this interval".into(),
        ));
    }
    let Some(Segment::Bezier {
        mut outgoing,
        mut incoming,
    }) = keys[index].segment
    else {
        return Err(veylvik_document::Error("not a Bezier segment".into()));
    };
    let (handle, other) = if right {
        (&mut incoming, outgoing)
    } else {
        (&mut outgoing, incoming)
    };
    let horizontal = f64::from(delta.x / graph.width()) * (range[1] - range[0]) / duration;
    let available = 1.0 - f64::from(other.time);
    handle.time = (f64::from(handle.time) + if right { -horizontal } else { horizontal })
        .clamp(0.0, available) as f32;
    if f64::from(handle.time) > available {
        handle.time = handle.time.next_down().max(0.0);
    }
    handle.value =
        (f64::from(handle.value) - f64::from(delta.y / graph.height()) * (max - min)) as f32;
    keys[index].segment = Some(Segment::Bezier { outgoing, incoming });
    edited.validate()?;
    Ok(edited)
}
