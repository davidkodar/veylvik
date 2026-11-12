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

/// One public processing stage; compound effects keep their internal graph intact.
fn members(doc: &Composition, id: NodeId) -> std::collections::BTreeSet<NodeId> {
    doc.groups
        .values()
        .find(|g| g.output == id)
        .map_or_else(|| [id].into_iter().collect(), |g| g.members.clone())
}
fn sources(doc: &Composition, id: NodeId) -> std::collections::BTreeSet<NodeId> {
    let members = members(doc, id);
    members
        .iter()
        .flat_map(|id| doc.nodes[id].inputs.values())
        .filter(|id| !members.contains(id))
        .copied()
        .collect()
}
pub fn chain(doc: &Composition, text: NodeId) -> Result<Vec<NodeId>> {
    let reachable = background::reachable(doc);
    let mut source = output(doc, text)?;
    let mut chain = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    while seen.insert(source) {
        let own = members(doc, source);
        let consumers: std::collections::BTreeSet<_> = doc
            .nodes
            .values()
            .filter(|n| {
                reachable.contains(&n.id)
                    && !own.contains(&n.id)
                    && n.inputs.values().any(|id| *id == source)
            })
            .map(|n| {
                doc.groups
                    .values()
                    .find(|g| g.members.contains(&n.id))
                    .map_or(n.id, |g| g.output)
            })
            .collect();
        if consumers.len() != 1 {
            break;
        }
        let next = *consumers.first().unwrap();
        if sources(doc, next) != [source].into_iter().collect() {
            break;
        }
        chain.push(next);
        source = next;
    }
    Ok(chain)
}
pub fn swap(doc: &Composition, text: NodeId, index: usize) -> Result<Vec<Command>> {
    let chain = chain(doc, text)?;
    if index + 1 >= chain.len() {
        return fail("No adjacent effect to reorder");
    }
    let (a, b) = (chain[index], chain[index + 1]);
    let source = *sources(doc, a).first().unwrap();
    let am = members(doc, a);
    let bm = members(doc, b);
    let mut commands = Vec::new();
    for node in doc.nodes.values() {
        let mut next = node.clone();
        for input in next.inputs.values_mut() {
            if am.contains(&node.id) && *input == source {
                *input = b;
            } else if bm.contains(&node.id) && *input == a {
                *input = source;
            } else if !am.contains(&node.id) && !bm.contains(&node.id) && *input == b {
                *input = a;
            }
        }
        if next.inputs != node.inputs {
            commands.push(Command::Put {
                node: Box::new(next),
            });
        }
    }
    if doc.output == b {
        commands.push(Command::Output { id: a });
    }
    crate::Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok(commands)
}
pub fn remove_effect(doc: &Composition, text: NodeId, id: NodeId) -> Result<Vec<Command>> {
    if !chain(doc, text)?.contains(&id) {
        return fail("Effect is not in this text's serial chain");
    }
    let source = *sources(doc, id).first().unwrap();
    let mut commands = connect_after(doc, id, source, vec![])?;
    if let Some(group) = doc.groups.values().find(|g| g.output == id) {
        commands.push(Command::Ungroup { id: group.id });
    }
    for id in members(doc, id) {
        commands.push(Command::Remove { id });
    }
    crate::Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok(commands)
}

pub fn shadow(doc: &Composition, source: NodeId) -> Result<Vec<Command>> {
    let alpha = Node::new(
        "Shadow alpha",
        Operation::ChannelMask {
            channel: crate::MaskChannel::Alpha,
        },
        &[("image", source)],
    );
    let tint = Node::new(
        "Shadow colour",
        Operation::Colorize {
            low: [0.0; 4],
            high: [0.0, 0.0, 0.0, 0.65],
        },
        &[("image", alpha.id)],
    );
    let offset = Node::new(
        "Shadow offset",
        Operation::Transform {
            x: 8.0.into(),
            y: 12.0.into(),
        },
        &[("image", tint.id)],
    );
    let blur = Node::new(
        "Shadow softness",
        Operation::Blur { radius: 8.0.into() },
        &[("image", offset.id)],
    );
    let merge = Node::new(
        "Shadow behind text",
        Operation::Merge {},
        &[("background", blur.id), ("foreground", source)],
    );
    let output = merge.id;
    let nodes = [alpha, tint, offset, blur, merge];
    let mut candidate = doc.clone();
    for node in &nodes {
        candidate.nodes.insert(node.id, node.clone());
    }
    let mut group = crate::groups::Group::capture(
        &candidate,
        nodes.iter().map(|n| n.id).collect(),
        output,
        "Text shadow",
    )?;
    for node in &nodes {
        for spec in node.operation.descriptor().parameters {
            if node.operation.scalar(spec.name).is_some() {
                group.controls.insert(
                    format!("{}_{}", node.id, spec.name),
                    crate::groups::Control::new(
                        &candidate,
                        node.id,
                        spec.name,
                        &format!("{} · {}", node.label, spec.name),
                    )?,
                );
            }
        }
    }
    let mut commands: Vec<_> = nodes
        .into_iter()
        .map(|node| Command::Put {
            node: Box::new(node),
        })
        .collect();
    commands.push(Command::PutGroup {
        group: Box::new(group),
    });
    connect_after(doc, source, output, commands)
}

pub fn plain(doc: &Composition, text: NodeId) -> Result<Vec<Command>> {
    let mut commands = apply(doc, text, None, None)?;
    let owner = crate::Document::new(doc.clone())?;
    let candidate = if commands.is_empty() {
        doc.clone()
    } else {
        owner.prepare(0, &commands)?
    };
    let layer = input_layer(&candidate, text)?;
    if layer != text {
        commands.extend(connect_after(&candidate, layer, text, vec![])?);
    }
    if !commands.is_empty() {
        owner.prepare(0, &commands)?;
    }
    Ok(commands)
}
pub fn gradient(doc: &Composition, text: NodeId) -> Result<Vec<Command>> {
    let gradient = Node::new(
        "Text gradient",
        Operation::Gradient {
            mode: crate::GradientMode::Linear,
            start_x: 0.0.into(),
            start_y: 0.0.into(),
            end_x: 0.0.into(),
            end_y: (doc.height as f32).into(),
        },
        &[],
    );
    let colors = Node::new(
        "Text gradient colours",
        Operation::Colorize {
            low: [0.05, 0.15, 0.7, 1.0],
            high: [0.9, 0.5, 0.1, 1.0],
        },
        &[("image", gradient.id)],
    );
    let image = colors.id;
    let mut commands = vec![
        Command::Put {
            node: Box::new(gradient),
        },
        Command::Put {
            node: Box::new(colors),
        },
    ];
    let owner = crate::Document::new(doc.clone())?;
    let candidate = owner.prepare(0, &commands)?;
    commands.extend(fill(&candidate, text, image)?.1);
    owner.prepare(0, &commands)?;
    Ok(commands)
}
pub fn fill_source(doc: &Composition, text: NodeId) -> Result<Option<NodeId>> {
    let layer = input_layer(doc, text)?;
    Ok((layer != text).then(|| doc.nodes[&layer].inputs["image"]))
}
