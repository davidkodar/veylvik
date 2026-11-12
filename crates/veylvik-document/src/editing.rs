//! Graph edits prepared as ordinary atomic document commands.
use crate::{Command, Composition, Node, NodeId, Result, fail};
use std::collections::{BTreeMap, BTreeSet};

/// Independent copies of a root and all its upstream dependencies.
/// Image asset keys are retained; copying a branch does not copy external files.
pub struct BranchCopy {
    pub root: NodeId,
    pub mapping: BTreeMap<NodeId, NodeId>,
    pub nodes: BTreeMap<NodeId, Node>,
    pub groups: Vec<crate::groups::Group>,
}
impl BranchCopy {
    pub fn into_commands(self) -> Vec<Command> {
        self.nodes
            .into_values()
            .map(|node| Command::Put {
                node: Box::new(node),
            })
            .chain(self.groups.into_iter().map(|group| Command::PutGroup {
                group: Box::new(group),
            }))
            .collect()
    }
}
/// Prepare a branch copy without mutating the composition or changing its output.
/// Shared upstream nodes are copied once and every copied input is remapped.
/// The host can place copied nodes before applying the commands as one transaction.
pub fn copy_branch(composition: &Composition, root: NodeId) -> Result<BranchCopy> {
    composition.validate()?;
    composition.encoded_size()?;
    if !composition.nodes.contains_key(&root) {
        return fail("cannot copy a missing branch root");
    }
    let mut source = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if source.insert(id) {
            pending.extend(composition.nodes[&id].inputs.values().copied());
        }
    }
    if composition.nodes.len() + source.len() > crate::limits::MAX_NODES {
        return fail("branch copy would exceed the 4096-node document limit");
    }
    let mut allocated: BTreeSet<_> = composition.nodes.keys().copied().collect();
    let mapping: BTreeMap<_, _> = source
        .into_iter()
        .map(|old| {
            let new = loop {
                let id = NodeId::new_v4();
                if allocated.insert(id) {
                    break id;
                }
            };
            (old, new)
        })
        .collect();
    let nodes = mapping
        .iter()
        .map(|(old, new)| {
            let mut node = composition.nodes[old].clone();
            node.id = *new;
            node.label.push_str(" copy");
            for input in node.inputs.values_mut() {
                *input = mapping[input];
            }
            (*new, node)
        })
        .collect();
    let groups = composition
        .groups
        .values()
        .filter(|g| g.members.iter().all(|id| mapping.contains_key(id)))
        .map(|g| {
            let mut group = g.clone();
            group.id = NodeId::new_v4();
            group.output = mapping[&g.output];
            group.members = g.members.iter().map(|id| mapping[id]).collect();
            for input in group.inputs.values_mut() {
                for target in &mut input.targets {
                    target.node = mapping[&target.node];
                }
            }
            for control in group.controls.values_mut() {
                control.node = mapping[&control.node];
            }
            group
        })
        .collect();
    Ok(BranchCopy {
        groups,
        root: mapping[&root],
        mapping,
        nodes,
    })
}
