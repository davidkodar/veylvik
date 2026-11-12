//! Tracking results become ordinary, editable transform keys in one transaction.
use crate::{Command, Composition, Key, Node, NodeId, Operation, Result, Scalar, Time, fail};
#[derive(Debug, Clone)]
pub struct Sample {
    pub time: Time,
    pub position: [f32; 2],
}
/// Wrap the target and rewire consumers. Existing transforms may be explicitly
/// updated; their keys outside the tracked interval remain intact.
pub fn attach(
    doc: &Composition,
    target: NodeId,
    origin: [f32; 2],
    samples: &[Sample],
    offset: [f32; 2],
    replace: bool,
) -> Result<(NodeId, Vec<Command>)> {
    doc.validate()?;
    if samples.len() < 2
        || samples.len() > crate::animation::MAX_KEYS
        || origin.into_iter().chain(offset).any(|v| !v.is_finite())
    {
        return fail("Tracking needs 2..4096 samples and finite offsets");
    }
    let input = doc
        .nodes
        .get(&target)
        .ok_or_else(|| crate::Error("Tracking target no longer exists".into()))?;
    let mut sorted = samples.to_vec();
    for sample in &sorted {
        sample.time.seconds()?;
        if sample.position.iter().any(|v| !v.is_finite()) {
            return fail("Invalid tracking position");
        }
    }
    sorted.sort_by(|a, b| a.time.compare(b.time).unwrap());
    for pair in sorted.windows(2) {
        if pair[0].time.compare(pair[1].time)? != std::cmp::Ordering::Less {
            return fail("Tracking sample times must be unique");
        }
    }
    let mut node = if replace {
        if !matches!(input.operation, Operation::Transform { .. }) {
            return fail("Only translation transforms can replace tracking keys");
        }
        input.clone()
    } else {
        Node::new(
            "Tracked motion",
            Operation::Transform {
                x: 0.0.into(),
                y: 0.0.into(),
            },
            &[("image", target)],
        )
    };
    for (axis, parameter) in ["x", "y"].into_iter().enumerate() {
        let scalar = node.operation.scalar_mut(parameter).unwrap();
        let baseline = scalar.at(samples[0].time)?;
        let mut keys = scalar.keys().map_or_else(Vec::new, |keys| keys.to_vec());
        keys.retain(|k| {
            k.time.compare(sorted[0].time).unwrap().is_lt()
                || k.time.compare(sorted.last().unwrap().time).unwrap().is_gt()
        });
        keys.extend(sorted.iter().map(|s| Key {
            time: s.time,
            value: baseline + s.position[axis] - origin[axis] + offset[axis],
            segment: Some(crate::Segment::Linear),
        }));
        keys.sort_by(|a, b| a.time.compare(b.time).unwrap());
        if let Some(existing) = scalar.keys_mut() {
            *existing = keys;
        } else {
            *scalar = Scalar::Linear { keys };
        }
    }
    let id = node.id;
    let mut commands = vec![Command::Put {
        node: Box::new(node),
    }];
    if !replace {
        for consumer in doc.nodes.values() {
            if consumer.inputs.values().any(|&source| source == target) {
                let mut consumer = consumer.clone();
                for source in consumer.inputs.values_mut() {
                    if *source == target {
                        *source = id;
                    }
                }
                commands.push(Command::Put {
                    node: Box::new(consumer),
                });
            }
        }
        for group in doc.groups.values().filter(|g| g.members.contains(&target)) {
            let mut group = group.clone();
            group.members.insert(id);
            if group.output == target {
                group.output = id;
            }
            commands.push(Command::PutGroup {
                group: Box::new(group),
            });
        }
        if doc.output == target {
            commands.push(Command::Output { id });
        }
    }
    crate::Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok((id, commands))
}
