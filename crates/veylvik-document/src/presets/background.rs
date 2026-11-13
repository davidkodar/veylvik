//! Background placement keeps subjects and their animation above a replaceable plate.
use crate::{Command, Composition, NodeId, NodePosition, Operation, Result};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Automatic,
    Behind,
    Replace(NodeId),
}

pub fn reachable(doc: &Composition) -> BTreeSet<NodeId> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![doc.output];
    while let Some(id) = pending.pop() {
        if seen.insert(id)
            && let Some(node) = doc.nodes.get(&id)
        {
            pending.extend(node.inputs.values().copied());
        }
    }
    seen
}

/// Internal group nodes cannot be disconnected without changing group membership.
pub fn candidates(doc: &Composition) -> Vec<NodeId> {
    reachable(doc)
        .into_iter()
        .filter(|id| {
            !doc.groups
                .values()
                .any(|g| g.members.contains(id) && g.output != *id)
        })
        .collect()
}

/// Prefer authored intent. Older graphs can expose a backdrop through the
/// background spine of composites and single-input effects; never infer by colour.
pub fn automatic(doc: &Composition) -> Option<NodeId> {
    let eligible = candidates(doc);
    let marked: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|id| doc.nodes[id].scene_background)
        .collect();
    if marked.len() == 1 {
        return Some(marked[0]);
    }
    if !marked.is_empty() {
        return None;
    }
    let mut id = doc.output;
    let mut visited = BTreeSet::new();
    let mut composite = false;
    while visited.insert(id) {
        let node = doc.nodes.get(&id)?;
        // Build 101 added material backgrounds below the already-composited scene.
        // Look through that wrapper to repair those saved graphs as well.
        if node.label == "Material layer composite"
            && node.inputs.get("background").is_some_and(|back| {
                doc.groups
                    .values()
                    .any(|g| g.output == *back && g.label.ends_with(" background"))
            })
        {
            id = *node.inputs.get("foreground")?;
            continue;
        }
        if matches!(
            node.operation,
            Operation::Merge(crate::nodes::Merge {})
                | Operation::Blend(crate::nodes::Blend { .. })
                | Operation::SurfaceBlend(crate::nodes::SurfaceBlend { .. })
        ) {
            composite = true;
            id = *node.inputs.get("background")?;
        } else if node.inputs.len() == 1 && node.inputs.contains_key("image") {
            id = node.inputs["image"];
        } else {
            // A flattened photograph or masked subject is never an automatic backdrop.
            return ((composite || id == doc.output)
                && eligible.contains(&id)
                && matches!(
                    node.operation,
                    Operation::Solid(crate::nodes::Solid { .. })
                        | Operation::Gradient(crate::nodes::Gradient { .. })
                        | Operation::Noise(crate::nodes::Noise { .. })
                        | Operation::FractalNoise(crate::nodes::FractalNoise { .. })
                ))
            .then_some(id);
        }
    }
    None
}

pub fn insert(
    id: &str,
    doc: &Composition,
    placement: Placement,
    position: Option<NodePosition>,
) -> Result<super::Instance> {
    if placement == Placement::Behind {
        return super::builtin::material_layer(id, doc, false, position);
    }
    let target = match placement {
        Placement::Automatic => automatic(doc).ok_or_else(|| crate::Error("No single backdrop identified. Open Background placement to choose what to replace, or place the material behind the finished composition.".into()))?,
        Placement::Replace(id) => id,
        Placement::Behind => unreachable!(),
    };
    if !candidates(doc).contains(&target) {
        return Err(crate::Error(
            "Choose a reachable node or group output as the backdrop".into(),
        ));
    }
    let mut instance = super::builtin::material_surface(id, doc, false, position)?;
    for command in &mut instance.commands {
        if let Command::Put { node } = command
            && node.id == instance.output
        {
            node.scene_background = true;
        }
    }
    let reachable = reachable(doc);
    let old_members = doc
        .groups
        .values()
        .find(|g| g.output == target)
        .map(|g| &g.members);
    for node in doc.nodes.values().filter(|n| reachable.contains(&n.id)) {
        if old_members.is_some_and(|members| members.contains(&node.id)) {
            continue;
        }
        if node.inputs.values().any(|source| *source == target) {
            let mut node = node.clone();
            for source in node.inputs.values_mut() {
                if *source == target {
                    *source = instance.output;
                }
            }
            instance.commands.push(Command::Put {
                node: Box::new(node),
            });
        }
    }
    if doc.output == target {
        instance.commands.push(Command::Output {
            id: instance.output,
        });
    }
    crate::Document::new(doc.clone())?.prepare(0, &instance.commands)?;
    Ok(instance)
}
