use super::*;
use std::collections::BTreeMap;
pub(super) type TrackId = (NodeId, String);
pub(super) type Selection = BTreeMap<TrackId, BTreeSet<usize>>;
pub(super) type Edits = Vec<(NodeId, String, Scalar)>;
pub(super) struct Marquee {
    pub origin: Pos2,
    pub base: Selection,
}
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct ClipboardTrack {
    pub node: NodeId,
    pub parameter: String,
    pub keys: Vec<Key>,
}
impl Editor {
    pub(super) fn selections(&self) -> Selection {
        let mut selected = self.linked.clone();
        if let Some(active) = &self.active
            && !self.selection.is_empty()
        {
            selected.insert(active.clone(), self.selection.clone());
        }
        selected.retain(|_, keys| !keys.is_empty());
        selected
    }
    pub(super) fn install_selection(&mut self, mut selected: Selection) {
        self.selection = self
            .active
            .as_ref()
            .and_then(|id| selected.remove(id))
            .unwrap_or_default();
        self.linked = selected;
    }
    pub(super) fn activate(&mut self, id: NodeId, parameter: &str, retain: bool) {
        let selected = if retain {
            self.selections()
        } else {
            Selection::new()
        };
        self.active = Some((id, parameter.into()));
        self.selected_node = Some(id);
        self.install_selection(selected);
    }
    pub(super) fn batch(
        &mut self,
        result: veylvik_document::Result<Edits>,
        doc: &Composition,
        revision: u64,
        actions: &mut Vec<Action>,
    ) {
        let result = result.and_then(|edits| {
            if edits.is_empty() || edits.len() > 64 {
                return Err(veylvik_document::Error(
                    "Select between 1 and 64 parameters".into(),
                ));
            }
            for (id, param, scalar) in &edits {
                let mut operation = doc
                    .nodes
                    .get(id)
                    .ok_or_else(|| veylvik_document::Error("missing node".into()))?
                    .operation
                    .clone();
                *operation
                    .scalar_mut(param)
                    .ok_or_else(|| veylvik_document::Error("missing parameter".into()))? =
                    scalar.clone();
                operation.validate()?;
            }
            Ok(edits)
        });
        match result {
            Ok(edits) => {
                actions.push(Action::Batch { revision, edits });
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub(super) fn copy_selection(&mut self, doc: &Composition) -> veylvik_document::Result<()> {
        let selected = self.selections();
        if selected.len() == 1 {
            let ((id, parameter), indices) = selected.first_key_value().unwrap();
            self.clipboard =
                key_edit::copied(doc.nodes[id].operation.scalar(parameter).unwrap(), indices)?;
            self.clipboard_tracks.clear();
            return Ok(());
        }
        if selected.is_empty() {
            return Err(veylvik_document::Error("Select keys first".into()));
        }
        if selected.len() > 64 {
            return Err(veylvik_document::Error(
                "Select at most 64 parameters".into(),
            ));
        }
        let mut first = None;
        for ((id, param), indices) in &selected {
            let scalar = doc.nodes[id].operation.scalar(param).unwrap();
            let time = scalar.keys().unwrap()[*indices.first().unwrap()].time;
            if first.is_none_or(|old| time.compare(old).is_ok_and(|o| o.is_lt())) {
                first = Some(time);
            }
        }
        let first = first.unwrap();
        let mut tracks = Vec::new();
        let mut count = 0;
        for ((id, param), indices) in selected {
            let scalar = doc.nodes[&id].operation.scalar(&param).unwrap();
            let offset = scalar.keys().unwrap()[*indices.first().unwrap()]
                .time
                .checked_sub(first)?;
            let mut keys = key_edit::copied(scalar, &indices)?;
            for key in &mut keys {
                key.time = key.time.checked_add(offset)?;
            }
            count += keys.len();
            if count > veylvik_document::animation::MAX_KEYS {
                return Err(veylvik_document::Error(
                    "Copy at most 4096 keys at once".into(),
                ));
            }
            tracks.push(ClipboardTrack {
                node: id,
                parameter: param,
                keys,
            });
        }
        self.clipboard.clear();
        self.clipboard_tracks = tracks;
        Ok(())
    }
    pub(super) fn paste_group(
        &self,
        doc: &Composition,
        now: Time,
    ) -> veylvik_document::Result<Edits> {
        let mut seen = BTreeSet::new();
        self.clipboard_tracks
            .iter()
            .map(|track| {
                if !seen.insert((track.node, track.parameter.clone())) {
                    return Err(veylvik_document::Error("duplicate clipboard track".into()));
                }
                let scalar = doc
                    .nodes
                    .get(&track.node)
                    .and_then(|n| n.operation.scalar(&track.parameter))
                    .ok_or_else(|| {
                        veylvik_document::Error(
                            "Grouped paste needs the original parameters in this composition"
                                .into(),
                        )
                    })?;
                Ok((
                    track.node,
                    track.parameter.clone(),
                    key_edit::pasted(scalar, &track.keys, now)?,
                ))
            })
            .collect()
    }
    pub(super) fn finish_marquee(&mut self, ui: &egui::Ui) {
        let Some(marquee) = &self.marquee else {
            return;
        };
        let Some(pointer) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let rect = Rect::from_two_pos(marquee.origin, pointer);
        if rect.size().length() > 3.0 {
            let mut selected = marquee.base.clone();
            for (id, param, index, point) in &self.hit_keys {
                if rect.contains(*point) {
                    selected
                        .entry((*id, param.clone()))
                        .or_default()
                        .insert(*index);
                }
            }
            self.install_selection(selected);
            ui.painter().rect_stroke(
                rect,
                0.0,
                (1.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if ui.input(|i| i.pointer.primary_released()) {
            self.marquee = None;
        }
    }
}
pub(super) fn moved_group(
    doc: &Composition,
    selected: &Selection,
    offset: Time,
) -> veylvik_document::Result<Edits> {
    if selected.len() > 64 {
        return Err(veylvik_document::Error(
            "Select at most 64 parameters".into(),
        ));
    }
    selected
        .iter()
        .map(|((id, param), indices)| {
            let scalar = doc
                .nodes
                .get(id)
                .and_then(|n| n.operation.scalar(param))
                .ok_or_else(|| veylvik_document::Error("missing selected parameter".into()))?;
            Ok((
                *id,
                param.clone(),
                key_edit::moved(scalar, indices, offset, 0.0)?,
            ))
        })
        .collect()
}
