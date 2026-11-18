//! One document owner with bounded, atomic undo/redo snapshots.
use crate::{
    Command, Composition, Error, Result, fail,
    limits::{DEFAULT_HISTORY_STEPS, MAX_HISTORY_BYTES, MAX_HISTORY_STEPS},
};
use std::collections::VecDeque;
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct HistoryStats {
    pub undo_steps: usize,
    pub redo_steps: usize,
    pub retained_bytes: usize,
    pub limit_bytes: usize,
}
struct Snapshot {
    composition: Composition,
    bytes: usize,
    /// The step between this state and the next newer one, named for people.
    label: String,
}
/// Only this owner publishes revisions. Renderers receive immutable snapshots.
pub struct Document {
    composition: Composition,
    bytes: usize,
    revision: u64,
    undo: VecDeque<Snapshot>,
    redo: VecDeque<Snapshot>,
    history_bytes: usize,
    history_limit: usize,
    step_limit: usize,
}
impl Document {
    pub fn new(composition: Composition) -> Result<Self> {
        Self::with_history_budget(composition, MAX_HISTORY_BYTES)
    }
    /// Limit retained snapshots by canonical JSON size, capped at 128 MiB and
    /// 64 entries unless `set_history_limits` says otherwise. This accounting excludes allocator overhead and the current document.
    pub fn with_history_budget(mut composition: Composition, bytes: usize) -> Result<Self> {
        composition.resolve_expressions()?;
        composition.validate()?;
        let size = composition.encoded_size()?;
        Ok(Self {
            composition,
            bytes: size,
            revision: 0,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            history_bytes: 0,
            history_limit: bytes.min(MAX_HISTORY_BYTES),
            step_limit: DEFAULT_HISTORY_STEPS,
        })
    }
    /// Keep at most `steps` undo and redo steps (1 to 256) within `bytes` of history
    /// (at most 128 MiB). Steps beyond the new limits are forgotten at once, the oldest
    /// first; the current composition is never touched.
    pub fn set_history_limits(&mut self, steps: usize, bytes: usize) {
        let (steps, bytes) = (
            steps.clamp(1, MAX_HISTORY_STEPS),
            bytes.min(MAX_HISTORY_BYTES),
        );
        if (steps, bytes) != (self.step_limit, self.history_limit) {
            (self.step_limit, self.history_limit) = (steps, bytes);
            self.trim(true);
        }
    }
    pub fn snapshot(&self) -> &Composition {
        &self.composition
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The names of the steps that can be undone, oldest first, and of those that can
    /// be redone, nearest first.
    pub fn steps(&self) -> (Vec<String>, Vec<String>) {
        (
            self.undo.iter().map(|s| s.label.clone()).collect(),
            self.redo.iter().rev().map(|s| s.label.clone()).collect(),
        )
    }
    pub fn history(&self) -> HistoryStats {
        HistoryStats {
            undo_steps: self.undo.len(),
            redo_steps: self.redo.len(),
            retained_bytes: self.history_bytes,
            limit_bytes: self.history_limit,
        }
    }
    fn check_revision(&self, expected: u64) -> Result<()> {
        if self.revision != expected {
            return fail(format!(
                "revision conflict: expected {expected}, current {}",
                self.revision
            ));
        }
        Ok(())
    }
    fn next_revision(&self) -> Result<u64> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| Error("document revision exhausted".into()))
    }
    pub fn prepare(&self, expected: u64, commands: &[Command]) -> Result<Composition> {
        self.check_revision(expected)?;
        if commands.is_empty() || commands.len() > 4096 {
            return fail("transaction needs 1..=4096 commands");
        }
        let mut next = self.composition.clone();
        for command in commands {
            match command {
                Command::PutGroup { group } => {
                    next.groups.insert(group.id, group.as_ref().clone());
                }
                Command::Ungroup { id } => {
                    if next.groups.remove(id).is_none() {
                        return fail("group does not exist");
                    }
                }
                Command::SetControl { id, control, value } => {
                    let exposed = next
                        .groups
                        .get(id)
                        .and_then(|g| g.controls.get(control))
                        .ok_or_else(|| Error("unknown group control".into()))?
                        .clone();
                    crate::groups::set_parameter(
                        &mut next,
                        exposed.node,
                        &exposed.parameter,
                        value.clone(),
                    )?;
                }
                Command::ConnectGroup { id, input, source } => {
                    let ports = next
                        .groups
                        .get(id)
                        .and_then(|g| g.inputs.get(input))
                        .ok_or_else(|| Error("unknown group input".into()))?
                        .targets
                        .clone();
                    for target in ports {
                        next.nodes
                            .get_mut(&target.node)
                            .ok_or_else(|| Error("missing group member".into()))?
                            .inputs
                            .insert(target.port, *source);
                    }
                }
                Command::Put { node } => {
                    next.nodes.insert(node.id, node.as_ref().clone());
                }
                Command::SetParameter {
                    id,
                    parameter,
                    value,
                } => {
                    let node = next
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| Error("cannot edit missing node".into()))?;
                    if !node
                        .operation
                        .descriptor()
                        .parameters
                        .iter()
                        .any(|p| p.name == parameter)
                    {
                        return fail("unknown node parameter");
                    }
                    if parameter == "content"
                        && let Some(after) = value.as_str()
                        && let crate::Operation::Text(crate::nodes::Text {
                            content,
                            typography,
                            ..
                        }) = &mut node.operation
                    {
                        typography.remap(content, after);
                    }
                    let mut operation = serde_json::to_value(&node.operation)
                        .map_err(|error| Error(error.to_string()))?;
                    operation[parameter] = value.clone();
                    node.operation = serde_json::from_value(operation)
                        .map_err(|error| Error(error.to_string()))?;
                }
                Command::SetKey {
                    id,
                    parameter,
                    time,
                    value,
                } => {
                    let operation = &mut next
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| Error("cannot edit missing node".into()))?
                        .operation;
                    if let Some(scalar) = operation.scalar_mut(parameter) {
                        scalar.set_key_at(*time, *value)?;
                    } else if let Some(crate::track::Property::Number(scalar)) =
                        operation.property_mut(parameter)
                    {
                        scalar.set_key_at(*time, *value)?;
                    } else {
                        return fail("parameter is not an animated scalar");
                    }
                }
                Command::RemoveKey {
                    id,
                    parameter,
                    time,
                } => {
                    let operation = &mut next
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| Error("cannot edit missing node".into()))?
                        .operation;
                    if let Some(scalar) = operation.scalar_mut(parameter) {
                        scalar.remove_key_at(*time)?;
                    } else {
                        match operation.property_mut(parameter) {
                            Some(crate::track::Property::Number(scalar)) => {
                                scalar.remove_key_at(*time)?
                            }
                            Some(crate::track::Property::Vector(track)) => {
                                track.remove_key_at(*time)?
                            }
                            None => return fail("parameter has no keys"),
                        }
                    }
                }
                Command::SetTrackKey {
                    id,
                    track,
                    time,
                    value,
                } => {
                    let operation = &mut next
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| Error("cannot edit missing node".into()))?
                        .operation;
                    match operation.property_mut(track) {
                        Some(crate::track::Property::Vector(track)) => {
                            track.set_key_at(*time, *value)?
                        }
                        _ => return fail("not a keyable colour or vector"),
                    }
                }
                Command::Remove { id } => {
                    if next.nodes.remove(id).is_none() {
                        return fail("cannot remove missing node");
                    }
                    crate::annotations::forget(&mut next.backdrops, *id);
                }
                Command::FrameRate { frame_rate } => next.frame_rate = *frame_rate,
                Command::FrameCount { frame_count } => next.frame_count = *frame_count,
                Command::OcioViews { settings } => next.ocio = settings.clone(),
                Command::WorkingSpace { space } => next.working_space = *space,
                Command::Display { display } => next.display = *display,
                Command::Markers { markers } => next.markers = markers.clone(),
                Command::Backdrops { backdrops } => next.backdrops = backdrops.clone(),
                Command::Annotate { id, tag, note } => {
                    let node = next
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| Error("cannot annotate missing node".into()))?;
                    node.tag = *tag;
                    node.note = note.clone();
                }
                Command::Output { id } => next.output = *id,
                Command::Place { id, position } => {
                    next.nodes
                        .get_mut(id)
                        .ok_or_else(|| Error("cannot place missing node".into()))?
                        .position = *position;
                }
                Command::Resize { width, height } => {
                    next.width = *width;
                    next.height = *height;
                }
            }
        }
        next.resolve_expressions()?;
        next.validate()?;
        Ok(next)
    }
    fn replace(&mut self, next: Snapshot) -> Snapshot {
        Snapshot {
            composition: std::mem::replace(&mut self.composition, next.composition),
            bytes: std::mem::replace(&mut self.bytes, next.bytes),
            label: next.label,
        }
    }
    fn trim(&mut self, prefer_undo: bool) {
        while self.history_bytes > self.history_limit
            || self.undo.len() + self.redo.len() > self.step_limit
        {
            // Discard the most distant state on the opposite side first.
            let removed = if prefer_undo {
                self.redo.pop_front().or_else(|| self.undo.pop_front())
            } else {
                self.undo.pop_front().or_else(|| self.redo.pop_front())
            };
            let Some(removed) = removed else {
                break;
            };
            self.history_bytes -= removed.bytes;
        }
    }
    pub fn apply(&mut self, expected: u64, commands: &[Command]) -> Result<u64> {
        let revision = self.next_revision()?;
        let next = self.prepare(expected, commands)?;
        let bytes = next.encoded_size()?;
        let label = crate::describe::step(commands, &self.composition);
        let previous = self.replace(Snapshot {
            composition: next,
            bytes,
            label,
        });
        self.history_bytes -= self
            .redo
            .iter()
            .map(|snapshot| snapshot.bytes)
            .sum::<usize>();
        self.redo.clear();
        self.history_bytes += previous.bytes;
        self.undo.push_back(previous);
        self.trim(true);
        self.revision = revision;
        Ok(revision)
    }
    pub fn undo(&mut self, expected: u64) -> Result<u64> {
        self.check_revision(expected)?;
        let revision = self.next_revision()?;
        let previous = self
            .undo
            .pop_back()
            .ok_or_else(|| Error("nothing to undo".into()))?;
        self.history_bytes -= previous.bytes;
        let current = self.replace(previous);
        self.history_bytes += current.bytes;
        self.redo.push_back(current);
        self.trim(false);
        self.revision = revision;
        Ok(revision)
    }
    pub fn redo(&mut self, expected: u64) -> Result<u64> {
        self.check_revision(expected)?;
        let revision = self.next_revision()?;
        let next = self
            .redo
            .pop_back()
            .ok_or_else(|| Error("nothing to redo".into()))?;
        self.history_bytes -= next.bytes;
        let current = self.replace(next);
        self.history_bytes += current.bytes;
        self.undo.push_back(current);
        self.trim(true);
        self.revision = revision;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_revision_does_not_change_document_or_history() {
        let mut document = Document::new(crate::demo()).unwrap();
        document
            .apply(
                0,
                &[Command::Resize {
                    width: 100,
                    height: 50,
                }],
            )
            .unwrap();
        document.revision = u64::MAX;
        let before = document.history();
        assert!(document.undo(u64::MAX).is_err());
        assert!(
            document
                .apply(
                    u64::MAX,
                    &[Command::Resize {
                        width: 2,
                        height: 2
                    }]
                )
                .is_err()
        );
        assert_eq!(document.snapshot().width, 100);
        assert_eq!(document.history().undo_steps, before.undo_steps);
        assert_eq!(document.history().retained_bytes, before.retained_bytes);
    }
}
