//! What an artist adds to a graph to read it: a colour tag and a note on a node, and backdrops, named frames around sets of nodes. Bounded, validated with the composition, and never part of a render.
use crate::{Composition, NodeId, Result, fail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Characters in a node's note.
pub const MAX_NOTE: usize = 400;
/// Backdrops a composition may hold, and characters in a backdrop's name.
pub const MAX_BACKDROPS: usize = 64;
pub const MAX_LABEL: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Purple,
    Grey,
}
impl Tag {
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Teal,
        Self::Blue,
        Self::Purple,
        Self::Grey,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Teal => "Teal",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Grey => "Grey",
        }
    }
    /// Display colour as sRGB bytes.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Red => [214, 84, 84],
            Self::Orange => [226, 140, 62],
            Self::Yellow => [222, 196, 72],
            Self::Green => [96, 184, 104],
            Self::Teal => [70, 180, 170],
            Self::Blue => [84, 140, 222],
            Self::Purple => [160, 112, 214],
            Self::Grey => [140, 146, 158],
        }
    }
}
/// A named frame around nodes; it is drawn around its members wherever they sit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backdrop {
    pub id: NodeId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<Tag>,
    pub nodes: BTreeSet<NodeId>,
}
impl Backdrop {
    /// A new blue frame named "Backdrop" around `nodes`.
    pub fn around(nodes: BTreeSet<NodeId>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            label: "Backdrop".into(),
            tag: Some(Tag::Blue),
            nodes,
        }
    }
}
/// A note may run over lines; nothing else that is not text.
pub fn validate_note(note: &str) -> Result<()> {
    if note.chars().count() > MAX_NOTE || note.chars().any(|c| c.is_control() && c != '\n') {
        return fail("a node note is at most 400 characters of plain text");
    }
    Ok(())
}
pub fn validate(doc: &Composition) -> Result<()> {
    if doc.backdrops.len() > MAX_BACKDROPS {
        return fail("a composition holds at most 64 backdrops");
    }
    let mut ids = BTreeSet::new();
    for backdrop in &doc.backdrops {
        if !ids.insert(backdrop.id) {
            return fail("backdrop ids must be unique");
        }
        if backdrop.label.chars().count() > MAX_LABEL
            || backdrop.label.chars().any(char::is_control)
        {
            return fail("a backdrop name is at most 80 characters without control characters");
        }
        if backdrop.nodes.is_empty() {
            return fail("a backdrop frames at least one node");
        }
        if backdrop.nodes.iter().any(|id| !doc.nodes.contains_key(id)) {
            return fail("a backdrop frames a node that does not exist");
        }
    }
    Ok(())
}
/// Forget `node` in every backdrop; a backdrop left empty goes with it.
pub fn forget(backdrops: &mut Vec<Backdrop>, node: NodeId) {
    for backdrop in backdrops.iter_mut() {
        backdrop.nodes.remove(&node);
    }
    backdrops.retain(|backdrop| !backdrop.nodes.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Document, Node, Operation, Time, blank};
    #[test]
    fn tags_notes_and_backdrops_validate_save_and_follow_removed_nodes() {
        let composition = blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
        let plain = serde_json::to_string(&composition).unwrap();
        assert!(!plain.contains("backdrops") && !plain.contains("note") && !plain.contains("tag"));
        let output = composition.output;
        let mut document = Document::new(composition).unwrap();
        let extra = Node::new(
            "Spare",
            Operation::Solid(crate::nodes::Solid { color: [0.0; 4] }),
            &[],
        );
        let spare = extra.id;
        let apply = |document: &mut Document, commands: &[Command]| {
            let revision = document.revision();
            document.apply(revision, commands)
        };
        apply(
            &mut document,
            &[Command::Put {
                node: Box::new(extra),
            }],
        )
        .unwrap();
        apply(
            &mut document,
            &[Command::Annotate {
                id: spare,
                tag: Some(Tag::Teal),
                note: "Replace with the\nfinal plate".into(),
            }],
        )
        .unwrap();
        let backdrop = Backdrop {
            id: uuid::Uuid::new_v4(),
            label: "Plates".into(),
            tag: Some(Tag::Blue),
            nodes: [spare, output].into(),
        };
        apply(
            &mut document,
            &[Command::Backdrops {
                backdrops: vec![backdrop.clone()],
            }],
        )
        .unwrap();
        // Saved and read back unchanged.
        let saved = serde_json::to_string(document.snapshot()).unwrap();
        let back: Composition = serde_json::from_str(&saved).unwrap();
        assert_eq!(back.nodes[&spare].tag, Some(Tag::Teal));
        assert_eq!(back.nodes[&spare].note, "Replace with the\nfinal plate");
        assert_eq!(back.backdrops, vec![backdrop.clone()]);
        // Refused: a long or control-character note, a missing node, a duplicate, an empty frame.
        for note in ["x".repeat(401), "tab\there".to_owned()] {
            assert!(
                apply(
                    &mut document,
                    &[Command::Annotate {
                        id: spare,
                        tag: None,
                        note
                    }]
                )
                .is_err()
            );
        }
        assert!(
            apply(
                &mut document,
                &[Command::Annotate {
                    id: uuid::Uuid::new_v4(),
                    tag: None,
                    note: String::new()
                }]
            )
            .is_err()
        );
        let mut missing = backdrop.clone();
        missing.nodes.insert(uuid::Uuid::new_v4());
        let mut empty = backdrop.clone();
        empty.nodes.clear();
        for bad in [
            vec![missing],
            vec![empty],
            vec![backdrop.clone(), backdrop.clone()],
        ] {
            assert!(apply(&mut document, &[Command::Backdrops { backdrops: bad }]).is_err());
        }
        assert_eq!(document.snapshot().backdrops.len(), 1);
        // Removing a framed node takes it out of the frame; removing the last takes the frame.
        apply(&mut document, &[Command::Remove { id: spare }]).unwrap();
        assert_eq!(document.snapshot().backdrops[0].nodes, [output].into());
        let mut only = vec![backdrop];
        forget(&mut only, spare);
        forget(&mut only, output);
        assert!(only.is_empty());
    }
}
