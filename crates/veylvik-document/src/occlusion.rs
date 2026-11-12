//! Foreground occlusion assembled from ordinary GPU graph operations.
use crate::{Command, Composition, Node, NodeId, Operation, Result, fail, vector::PathMask};
use std::collections::BTreeSet;

pub fn depends_on(doc: &Composition, root: NodeId, target: NodeId) -> bool {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if id == target {
            return true;
        }
        if seen.insert(id)
            && let Some(node) = doc.nodes.get(&id)
        {
            pending.extend(node.inputs.values().copied());
        }
    }
    false
}
/// Layers that do not contain the selected text, computed in one graph traversal.
pub fn independent_sources(doc: &Composition, text: NodeId) -> BTreeSet<NodeId> {
    let mut consumers = std::collections::BTreeMap::<NodeId, Vec<NodeId>>::new();
    for node in doc.nodes.values() {
        for input in node.inputs.values() {
            consumers.entry(*input).or_default().push(node.id);
        }
    }
    let mut dependent = BTreeSet::new();
    let mut pending = vec![text];
    while let Some(id) = pending.pop() {
        if dependent.insert(id)
            && let Some(children) = consumers.get(&id)
        {
            pending.extend(children);
        }
    }
    doc.nodes
        .keys()
        .filter(|id| !dependent.contains(id))
        .copied()
        .collect()
}
#[derive(Clone, Copy)]
pub struct Setup {
    pub outline: NodeId,
    pub expansion: NodeId,
    pub softness: NodeId,
    pub source: NodeId,
    pub output: NodeId,
}

pub fn create(
    doc: &Composition,
    text: NodeId,
    source: NodeId,
    geometry: PathMask,
) -> Result<(Setup, Vec<Command>)> {
    doc.validate()?;
    geometry.validate()?;
    if !doc
        .nodes
        .get(&text)
        .is_some_and(|n| matches!(n.operation, Operation::Text { .. }))
    {
        return fail("Select editable text first");
    }
    if !depends_on(doc, doc.output, text) {
        return fail(
            "Connect this text to the composition output before placing it behind an object",
        );
    }
    if !doc.nodes.contains_key(&source) || depends_on(doc, source, text) {
        return fail("Choose a foreground layer that does not already contain this text");
    }
    let path = Node::new("Foreground outline", Operation::Path { geometry }, &[]);
    let grow = Node::new(
        "Foreground edge expansion",
        Operation::Morphology { radius: 0.0.into() },
        &[("image", path.id)],
    );
    let blur = Node::new(
        "Foreground edge softness",
        Operation::Blur { radius: 0.0.into() },
        &[("image", grow.id)],
    );
    let cutout = Node::new(
        "Foreground object",
        Operation::Mask {},
        &[("image", source), ("mask", blur.id)],
    );
    let merge = Node::new(
        "Text behind object",
        Operation::Merge {},
        &[("background", doc.output), ("foreground", cutout.id)],
    );
    let setup = Setup {
        outline: path.id,
        expansion: grow.id,
        softness: blur.id,
        source,
        output: merge.id,
    };
    let mut commands: Vec<_> = [path, grow, blur, cutout, merge]
        .into_iter()
        .map(|node| Command::Put {
            node: Box::new(node),
        })
        .collect();
    commands.push(Command::Output { id: setup.output });
    // Validate size, ports and graph limits before handing a transaction to a host.
    crate::Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok((setup, commands))
}

/// Recognize an unambiguous editable setup after save/load, without hidden UI metadata.
pub fn find(doc: &Composition, outline: NodeId) -> Option<Setup> {
    if !matches!(doc.nodes.get(&outline)?.operation, Operation::Path { .. }) {
        return None;
    }
    let child = |id, port: &str| {
        let mut nodes = doc
            .nodes
            .values()
            .filter(|n| n.inputs.get(port) == Some(&id));
        let first = nodes.next()?;
        if nodes.next().is_some() {
            None
        } else {
            Some(first)
        }
    };
    let grow = child(outline, "image")?;
    if !matches!(grow.operation, Operation::Morphology { .. }) {
        return None;
    }
    let blur = child(grow.id, "image")?;
    if !matches!(blur.operation, Operation::Blur { .. }) {
        return None;
    }
    let cutout = child(blur.id, "mask")?;
    if !matches!(cutout.operation, Operation::Mask {}) {
        return None;
    }
    let merge = child(cutout.id, "foreground")?;
    if !matches!(merge.operation, Operation::Merge {}) {
        return None;
    }
    Some(Setup {
        outline,
        expansion: grow.id,
        softness: blur.id,
        source: *cutout.inputs.get("image")?,
        output: merge.id,
    })
}
