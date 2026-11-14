//! Direct text material authoring, using ordinary graph nodes and atomic commands.
use super::{background, builtin, connect_after};
use crate::{Command, Composition, Node, NodeId, NodePosition, Operation, Result, fail};
fn input_layer(doc: &Composition, text: NodeId) -> Result<NodeId> {
    let reachable = background::reachable(doc);
    // Shape processing must precede styling: a material before Drip is reduced
    // back to coverage and disappears. Follow the unique liquid silhouette.
    let drips: Vec<_> = doc
        .nodes
        .values()
        .filter(|n| {
            reachable.contains(&n.id)
                && matches!(n.operation, Operation::Drip(_))
                && n.inputs.get("image") == Some(&text)
        })
        .map(|n| n.id)
        .collect();
    if drips.len() > 1 {
        return fail("This text feeds several liquid shapes. Select a branch to style it.");
    }
    let text = drips.first().copied().unwrap_or(text);
    let fills: Vec<_> = doc
        .nodes
        .values()
        .filter(|n| {
            reachable.contains(&n.id)
                && ((matches!(n.operation, Operation::Mask(crate::nodes::Mask { .. }))
                    && n.inputs.get("mask") == Some(&text))
                    || (matches!(n.operation, Operation::Colorize(_))
                        && n.inputs.get("image") == Some(&text)
                        && doc.nodes.values().any(|m| {
                            reachable.contains(&m.id)
                                && matches!(m.operation, Operation::Material(_))
                                && m.inputs.get("image") == Some(&n.id)
                        })))
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
                && matches!(
                    n.operation,
                    Operation::Material(crate::nodes::Material { .. })
                )
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
        .is_some_and(|n| matches!(n.operation, Operation::Text(crate::nodes::Text { .. })))
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
        .find(|n| {
            matches!(
                n.operation,
                Operation::Material(crate::nodes::Material { .. })
            )
        })
        .unwrap();
    if let Some(id) = current {
        let mut node = doc.nodes[&id].clone();
        let Operation::Material(crate::nodes::Material { kind: new_kind, .. }) = material.operation
        else {
            unreachable!()
        };
        if let Operation::Material(crate::nodes::Material { kind, optics, .. }) =
            &mut node.operation
        {
            *kind = new_kind;
            *optics = None;
        }
        node.label = format!("Text · {}", preset.group.label);
        let mut commands = vec![Command::Put {
            node: Box::new(node.clone()),
        }];
        // Wet looks combine pigment with a dielectric reflection pass. Once a
        // full surface material is selected, it replaces that pigment rather
        // than being additively tinted by it. Keep the output/group identity.
        let liquid = matches!(doc.nodes[&text].operation, Operation::Drip(_))
            || doc.nodes[&text]
                .inputs
                .get("mask")
                .is_some_and(|id| matches!(doc.nodes[id].operation, Operation::Drip(_)));
        for mix in doc
            .nodes
            .values()
            .filter(|n| n.inputs.get("foreground") == Some(&id))
        {
            if let Operation::Blend(blend) = &mix.operation
                && liquid
                && blend.mode == crate::BlendMode::Add
                && mix.inputs.get("background").is_some_and(|bg| {
                    *bg == text
                        || doc.nodes.get(bg).is_some_and(|n| {
                            matches!(n.operation, Operation::Colorize(_))
                                && n.inputs.get("image") == Some(&text)
                        })
                })
            {
                // Keep the pigment node feeding the material so captured
                // groups and their public colour control remain valid.
                node.inputs.insert("image".into(), mix.inputs["background"]);
                commands.push(Command::Put {
                    node: Box::new(node.clone()),
                });
                let mut mix = mix.clone();
                mix.operation = Operation::Transform(crate::nodes::Transform {
                    x: 0.0.into(),
                    y: 0.0.into(),
                });
                mix.inputs = [("image".to_owned(), id)].into();
                commands.push(Command::Put {
                    node: Box::new(mix),
                });
            }
        }
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
    let shadow = Node::new(
        "Shadow",
        Operation::Shadow(crate::nodes::Shadow {
            follow_lighting: false,
            distance: 20.0.into(),
            softness: 8.0.into(),
            strength: 0.65.into(),
            x: 8.0.into(),
            y: 12.0.into(),
            light: 0.0.into(),
        }),
        &[("image", source), ("lighting", source)],
    );
    let merge = Node::new(
        "Shadow behind text",
        Operation::Merge(crate::nodes::Merge {}),
        &[("background", shadow.id), ("foreground", source)],
    );
    let output = merge.id;
    let nodes = [shadow, merge];
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
            if node.operation.scalar(spec.name).is_some() || spec.name == "follow_lighting" {
                group.controls.insert(
                    format!("{}_{}", node.id, spec.name),
                    crate::groups::Control::new(
                        &candidate,
                        node.id,
                        spec.name,
                        &if spec.name == "follow_lighting" {
                            "Follow scene lighting".into()
                        } else {
                            format!("{} · {}", node.label, spec.name.replace('_', " "))
                        },
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
    // Reuse the connected gradient, including its authored colours and animation.
    if let Some(image) = fill_source(doc, text)?
        && matches!(
            doc.nodes[&image].operation,
            Operation::Colorize(crate::nodes::Colorize { .. })
        )
        && doc.nodes[&image].inputs.get("image").is_some_and(|id| {
            matches!(
                doc.nodes[id].operation,
                Operation::Gradient(crate::nodes::Gradient { .. })
            )
        })
    {
        return if current(doc, text)?.is_none() {
            Ok(vec![])
        } else {
            Ok(fill(doc, text, image)?.1)
        };
    }
    let gradient = Node::new(
        "Text gradient",
        Operation::Gradient(crate::nodes::Gradient {
            mode: crate::GradientMode::Linear,
            start_x: 0.0.into(),
            start_y: 0.0.into(),
            end_x: 0.0.into(),
            end_y: (doc.height as f32).into(),
        }),
        &[],
    );
    let colors = Node::new(
        "Text gradient colours",
        Operation::Colorize(crate::nodes::Colorize {
            low: [0.05, 0.15, 0.7, 1.0],
            high: [0.9, 0.5, 0.1, 1.0],
        }),
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

/// Recognise the original five-node text shadow without relying on labels.
pub fn legacy_shadow_nodes(doc: &Composition, output: NodeId) -> Option<[NodeId; 5]> {
    let group = doc
        .groups
        .values()
        .find(|g| g.output == output && g.members.len() == 5)?;
    let merge = doc.nodes.get(&output)?;
    if !matches!(merge.operation, Operation::Merge(_)) {
        return None;
    }
    let blur = doc.nodes.get(merge.inputs.get("background")?)?;
    let offset = doc.nodes.get(blur.inputs.get("image")?)?;
    let tint = doc.nodes.get(offset.inputs.get("image")?)?;
    let alpha = doc.nodes.get(tint.inputs.get("image")?)?;
    if !matches!(blur.operation, Operation::Blur(_))
        || !matches!(offset.operation, Operation::Transform(_))
        || !matches!(&tint.operation, Operation::Colorize(c) if c.low == [0.0;4] && c.high[..3] == [0.0;3])
        || !matches!(&alpha.operation, Operation::ChannelMask(c) if matches!(c.channel, crate::MaskChannel::Alpha))
        || alpha.inputs.get("image") != merge.inputs.get("foreground")
    {
        return None;
    }
    let ids = [alpha.id, tint.id, offset.id, blur.id, merge.id];
    ids.iter()
        .all(|id| group.members.contains(id))
        .then_some(ids)
}
/// Explicit, undoable migration. Keep the output identity and its downstream wiring.
pub fn upgrade_shadow(doc: &Composition, output: NodeId) -> Result<Vec<Command>> {
    let [alpha, tint, offset, blur, _] = legacy_shadow_nodes(doc, output)
        .ok_or_else(|| crate::Error("This is not an original text shadow group".into()))?;
    let removed = [alpha, tint, offset];
    let group = doc.groups.values().find(|g| g.output == output).unwrap();
    if doc
        .nodes
        .values()
        .any(|n| !group.members.contains(&n.id) && n.inputs.values().any(|id| removed.contains(id)))
    {
        return fail(
            "This shadow shares internal nodes. Keep it and add a separate light-following shadow instead.",
        );
    }
    let Operation::Blur(old_blur) = &doc.nodes[&blur].operation else {
        unreachable!()
    };
    let Operation::Colorize(old_tint) = &doc.nodes[&tint].operation else {
        unreachable!()
    };
    let source = doc.nodes[&output].inputs["foreground"];
    let mut shadow = doc.nodes[&blur].clone();
    shadow.label = "Shadow".into();
    shadow.operation = Operation::Shadow(crate::nodes::Shadow {
        follow_lighting: true,
        distance: 20.0.into(),
        softness: old_blur.radius.clone(),
        strength: old_tint.high[3].into(),
        x: 0.0.into(),
        y: 0.0.into(),
        light: 0.0.into(),
    });
    shadow.inputs = [("image".into(), source), ("lighting".into(), source)].into();
    let mut candidate = doc.clone();
    candidate.groups.remove(&group.id);
    for id in removed {
        candidate.nodes.remove(&id);
    }
    candidate.nodes.insert(blur, shadow.clone());
    let mut replacement =
        crate::groups::Group::capture(&candidate, [blur, output].into(), output, &group.label)?;
    replacement.id = group.id;
    for spec in shadow.operation.descriptor().parameters {
        replacement.controls.insert(
            spec.name.into(),
            crate::groups::Control::new(
                &candidate,
                blur,
                spec.name,
                &if spec.name == "follow_lighting" {
                    "Follow scene lighting".into()
                } else {
                    format!("Shadow · {}", spec.name.replace('_', " "))
                },
            )?,
        );
    }
    let mut commands = vec![
        Command::Ungroup { id: group.id },
        Command::Put {
            node: Box::new(shadow),
        },
    ];
    commands.extend(removed.into_iter().map(|id| Command::Remove { id }));
    commands.push(Command::PutGroup {
        group: Box::new(replacement),
    });
    crate::Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok(commands)
}
