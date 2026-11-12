//! Direct text material authoring, using ordinary graph nodes and atomic commands.
use super::{background, builtin, connect_after};
use crate::{Command, Composition, Node, NodeId, NodePosition, Operation, Result, fail};
fn input_layer(doc: &Composition, text: NodeId) -> Result<NodeId> {
    let reachable = background::reachable(doc);
    let fills: Vec<_> = doc
        .nodes
        .values()
        .filter(|n| {
            reachable.contains(&n.id)
                && matches!(n.operation, Operation::Mask { .. })
                && n.inputs.get("mask") == Some(&text)
        })
        .map(|n| n.id)
        .collect();
    if fills.len() > 1 {
        return fail("This text feeds several image fills. Select a branch to style it.");
    }
    Ok(fills.first().copied().unwrap_or(text))
}
pub fn current(doc: &Composition, text: NodeId) -> Result<Option<NodeId>> {
    let text = input_layer(doc, text)?;
    let reachable = background::reachable(doc);
    let materials: Vec<_> = doc
        .nodes
        .values()
        .filter(|n| {
            reachable.contains(&n.id)
                && matches!(n.operation, Operation::Material { .. })
                && n.inputs.get("image") == Some(&text)
        })
        .map(|n| n.id)
        .collect();
    if materials.len() > 1 {
        return fail(
            "This text feeds several materials. Select the material node to edit a specific branch.",
        );
    }
    Ok(materials.first().copied())
}
pub fn apply(
    doc: &Composition,
    text: NodeId,
    style: Option<&str>,
    position: Option<NodePosition>,
) -> Result<Vec<Command>> {
    if !doc
        .nodes
        .get(&text)
        .is_some_and(|n| matches!(n.operation, Operation::Text { .. }))
    {
        return fail("Select editable text first");
    }
    let current = current(doc, text)?;
    let text = input_layer(doc, text)?;
    let Some(style) = style else {
        return match current {
            Some(id) => connect_after(doc, id, text, vec![]),
            None => Ok(vec![]),
        };
    };
    if !builtin::is_material(style) {
        return fail("Choose a surface material");
    }
    let preset =
        builtin::build(style)?.adapted(doc.width, doc.height, super::resolution::Sizing::Fit)?;
    let material = preset
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Material { .. }))
        .unwrap();
    if let Some(id) = current {
        let mut node = doc.nodes[&id].clone();
        let Operation::Material { kind: new_kind, .. } = material.operation else {
            unreachable!()
        };
        if let Operation::Material { kind, .. } = &mut node.operation {
            *kind = new_kind;
        }
        node.label = format!("Text · {}", preset.group.label);
        let mut commands = vec![Command::Put {
            node: Box::new(node),
        }];
        if let Some(group) = doc
            .groups
            .values()
            .find(|g| g.output == id && g.members.len() == 1)
        {
            let mut group = group.clone();
            group.label = preset.group.label;
            commands.push(Command::PutGroup {
                group: Box::new(group),
            });
        }
        crate::Document::new(doc.clone())?.prepare(0, &commands)?;
        return Ok(commands);
    }
    let mut node = Node::new(
        &format!("Text · {}", preset.group.label),
        material.operation.clone(),
        &[("image", text)],
    );
    node.position = position;
    connect_after(
        doc,
        text,
        node.id,
        vec![Command::Put {
            node: Box::new(node),
        }],
    )
}

/// The styled layer is the input for new effects, so a glow follows its surface.
pub fn output(doc: &Composition, text: NodeId) -> Result<NodeId> {
    Ok(current(doc, text)?.unwrap_or(input_layer(doc, text)?))
}
/// Image fill replaces a surface and reuses an existing fill node, in one undo step.
pub fn fill(doc: &Composition, text: NodeId, image: NodeId) -> Result<(NodeId, Vec<Command>)> {
    let mut commands = apply(doc, text, None, None)?;
    let owner = crate::Document::new(doc.clone())?;
    let plain = if commands.is_empty() {
        doc.clone()
    } else {
        owner.prepare(0, &commands)?
    };
    let layer = input_layer(&plain, text)?;
    let output = if layer == text {
        let (output, fill) = super::fill_layer(&plain, text, image)?;
        commands.extend(fill);
        output
    } else {
        let mut node = plain.nodes[&layer].clone();
        node.inputs.insert("image".into(), image);
        commands.push(Command::Put {
            node: Box::new(node),
        });
        layer
    };
    owner.prepare(0, &commands)?;
    Ok((output, commands))
}
