//! One document owner with bounded, atomic undo/redo snapshots.
use crate::{
    Command, Composition, Error, Result, fail,
    limits::{MAX_HISTORY_BYTES, MAX_HISTORY_STEPS},
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
}
impl Document {
    pub fn new(composition: Composition) -> Result<Self> {
        Self::with_history_budget(composition, MAX_HISTORY_BYTES)
    }
    /// Limit retained snapshots by canonical JSON size, capped at 128 MiB and
    /// 64 entries. This accounting excludes allocator overhead and the current document.
    pub fn with_history_budget(composition: Composition, bytes: usize) -> Result<Self> {
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
        })
    }
    pub fn snapshot(&self) -> &Composition {
        &self.composition
    }
    pub fn revision(&self) -> u64 {
        self.revision
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
                Command::Put { node } => {
                    next.nodes.insert(node.id, node.as_ref().clone());
                }
                Command::Remove { id } => {
                    if next.nodes.remove(id).is_none() {
                        return fail("cannot remove missing node");
                    }
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
        next.validate()?;
        Ok(next)
    }
    fn replace(&mut self, next: Snapshot) -> Snapshot {
        Snapshot {
            composition: std::mem::replace(&mut self.composition, next.composition),
            bytes: std::mem::replace(&mut self.bytes, next.bytes),
        }
    }
    fn trim(&mut self, prefer_undo: bool) {
        while self.history_bytes > self.history_limit
            || self.undo.len() + self.redo.len() > MAX_HISTORY_STEPS
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
        let previous = self.replace(Snapshot {
            composition: next,
            bytes,
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
