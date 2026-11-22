//! Named graph boundaries and parameter aliases. Groups add no render passes or
//! duplicate parameter storage: editing an exposed control edits its real node.
use crate::{Command, Composition, Error, NodeId, Result, fail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_CONTROLS: usize = 64;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub node: NodeId,
    pub port: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub label: String,
    pub targets: Vec<Port>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub label: String,
    pub node: NodeId,
    pub parameter: String,
    /// Optional narrower authoring bounds, enforced on every edit path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f32; 2]>,
    pub default: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: NodeId,
    pub label: String,
    pub members: BTreeSet<NodeId>,
    pub output: NodeId,
    pub inputs: BTreeMap<String, Input>,
    pub controls: BTreeMap<String, Control>,
}
/// Keeps every group's public inputs in step with its members' edges from outside:
/// a new one (a scene given to a shader inside an effect, say) becomes an input named
/// after its port, and an input whose edges are gone is dropped. Run on each
/// transaction, so connecting to a node inside a group needs no second step.
pub fn follow_edges(doc: &mut Composition) {
    let nodes = &doc.nodes;
    for group in doc.groups.values_mut() {
        let outside: BTreeSet<Port> = group
            .members
            .iter()
            .filter_map(|member| nodes.get(member).map(|node| (member, node)))
            .flat_map(|(member, node)| {
                node.inputs
                    .iter()
                    .filter(|(_, source)| !group.members.contains(source))
                    .map(|(port, _)| Port {
                        node: *member,
                        port: port.clone(),
                    })
            })
            .collect();
        for input in group.inputs.values_mut() {
            input.targets.retain(|target| outside.contains(target));
        }
        group.inputs.retain(|_, input| !input.targets.is_empty());
        let exposed: BTreeSet<Port> = group
            .inputs
            .values()
            .flat_map(|input| input.targets.iter().cloned())
            .collect();
        for port in outside.difference(&exposed) {
            let mut key = port.port.clone();
            let mut k = 2;
            while group.inputs.contains_key(&key) {
                key = format!("{}_{k}", port.port);
                k += 1;
            }
            let mut label: String = port.port.replace('_', " ");
            if let Some(first) = label.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            group.inputs.insert(
                key,
                Input {
                    label,
                    targets: vec![port.clone()],
                },
            );
        }
    }
}
fn name(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control)
    {
        return fail("group names and labels need 1..=128 printable characters");
    }
    Ok(())
}
pub fn set_parameter(
    doc: &mut Composition,
    id: NodeId,
    parameter: &str,
    value: Value,
) -> Result<()> {
    let node = doc
        .nodes
        .get_mut(&id)
        .ok_or_else(|| Error("missing control node".into()))?;
    if !node
        .operation
        .descriptor()
        .parameters
        .iter()
        .any(|p| p.name == parameter)
    {
        return fail("unknown control parameter");
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
    let mut operation = serde_json::to_value(&node.operation).map_err(|e| Error(e.to_string()))?;
    operation[parameter] = value;
    node.operation = serde_json::from_value(operation).map_err(|e| Error(e.to_string()))?;
    node.operation.validate()
}
impl Control {
    pub fn value(&self, doc: &Composition) -> Result<Value> {
        let node = doc
            .nodes
            .get(&self.node)
            .ok_or_else(|| Error("missing control node".into()))?;
        let value = serde_json::to_value(&node.operation).map_err(|e| Error(e.to_string()))?;
        value
            .get(&self.parameter)
            .cloned()
            .ok_or_else(|| Error("unknown control parameter".into()))
    }
    pub fn new(doc: &Composition, node: NodeId, parameter: &str, label: &str) -> Result<Self> {
        let mut control = Self {
            node,
            parameter: parameter.into(),
            label: label.into(),
            range: None,
            default: Value::Null,
        };
        control.default = control.value(doc)?;
        Ok(control)
    }
}
impl Group {
    /// Capture a single-output selection. Boundary inputs sharing an external
    /// source become one public input, so a reconnect updates every consumer.
    pub fn capture(
        doc: &Composition,
        members: BTreeSet<NodeId>,
        output: NodeId,
        label: &str,
    ) -> Result<Self> {
        let mut sources: BTreeMap<NodeId, Vec<Port>> = BTreeMap::new();
        for id in &members {
            let node = doc
                .nodes
                .get(id)
                .ok_or_else(|| Error("missing group member".into()))?;
            for (port, source) in &node.inputs {
                if !members.contains(source) {
                    sources.entry(*source).or_default().push(Port {
                        node: *id,
                        port: port.clone(),
                    });
                }
            }
        }
        let inputs = sources
            .into_iter()
            .enumerate()
            .map(|(i, (source, targets))| {
                (
                    format!("input_{}", i + 1),
                    Input {
                        label: doc
                            .nodes
                            .get(&source)
                            .map(|n| n.label.clone())
                            .unwrap_or_else(|| "Image".into()),
                        targets,
                    },
                )
            })
            .collect();
        let group = Self {
            id: NodeId::new_v4(),
            label: label.into(),
            members,
            output,
            inputs,
            controls: BTreeMap::new(),
        };
        let mut candidate = doc.clone();
        candidate.groups.insert(group.id, group.clone());
        candidate.validate()?;
        Ok(group)
    }
    pub fn source(&self, doc: &Composition, input: &str) -> Option<NodeId> {
        let target = self.inputs.get(input)?.targets.first()?;
        doc.nodes
            .get(&target.node)?
            .inputs
            .get(&target.port)
            .copied()
    }
    /// Make an independent instance, retaining external input connections and
    /// remapping all internal references, exposed controls and animation curves.
    pub fn duplicate(&self, doc: &Composition) -> Result<(Self, Vec<Command>)> {
        let mut checked = doc.clone();
        checked.groups.insert(self.id, self.clone());
        checked.validate()?;
        if doc.nodes.len() + self.members.len() > crate::limits::MAX_NODES {
            return fail("group copy exceeds node limit");
        }
        let mapping: BTreeMap<_, _> = self
            .members
            .iter()
            .map(|id| (*id, NodeId::new_v4()))
            .collect();
        let mut commands = Vec::new();
        for id in &self.members {
            let mut node = doc.nodes[id].clone();
            node.id = mapping[id];
            node.position = None;
            for source in node.inputs.values_mut() {
                if let Some(new) = mapping.get(source) {
                    *source = *new;
                }
            }
            commands.push(Command::Put {
                node: Box::new(node),
            });
        }
        let mut group = self.clone();
        group.id = NodeId::new_v4();
        group.label = format!("{} copy", self.label.chars().take(123).collect::<String>());
        group.output = mapping[&self.output];
        group.members = mapping.values().copied().collect();
        for input in group.inputs.values_mut() {
            for target in &mut input.targets {
                target.node = mapping[&target.node];
            }
        }
        for control in group.controls.values_mut() {
            control.node = mapping[&control.node];
        }
        commands.push(Command::PutGroup {
            group: Box::new(group.clone()),
        });
        Ok((group, commands))
    }
}
/// A control's node, parameter, default and range: the parameter is the node's, the
/// default is a valid value for it, and a range lies within the registered bounds and
/// holds both the value and the default.
fn check_control(doc: &Composition, control: &Control) -> Result<()> {
    if !doc.nodes.contains_key(&control.node) {
        return fail("a control's node does not exist");
    }
    let node = &doc.nodes[&control.node];
    if !node
        .operation
        .descriptor()
        .parameters
        .iter()
        .any(|p| p.name == control.parameter)
    {
        return fail("unknown exposed parameter");
    }
    let mut default_node = node.clone();
    let mut value = serde_json::to_value(&node.operation).map_err(|e| Error(e.to_string()))?;
    value[&control.parameter] = control.default.clone();
    default_node.operation = serde_json::from_value(value).map_err(|e| Error(e.to_string()))?;
    default_node.operation.validate()?;
    if let Some([min, max]) = control.range {
        let descriptor = node
            .operation
            .descriptor()
            .parameters
            .iter()
            .find(|p| p.name == control.parameter)
            .unwrap();
        if !min.is_finite()
            || !max.is_finite()
            || min > max
            || descriptor.min.is_none_or(|bound| min < bound)
            || descriptor.max.is_none_or(|bound| max > bound)
        {
            return fail("control range must lie within the registered scalar bounds");
        }
        for operation in [&node.operation, &default_node.operation] {
            operation
                .scalar(&control.parameter)
                .ok_or_else(|| Error("only scalar controls have ranges".into()))?
                .bounded(min, max, &control.label)?;
        }
    }
    Ok(())
}
/// The composition's own published controls: at most 64, each naming a distinct
/// parameter of an existing node, checked as a group's controls are.
pub(crate) fn validate_published(doc: &Composition) -> Result<()> {
    if doc.published.len() > MAX_CONTROLS {
        return fail("a composition publishes at most 64 controls");
    }
    let mut targets = BTreeSet::new();
    for control in &doc.published {
        name(&control.label)?;
        if !targets.insert((control.node, &control.parameter)) {
            return fail("a parameter is published once");
        }
        check_control(doc, control)?;
    }
    Ok(())
}
/// Everything a host shows for a composition: its own published controls in their
/// order, then each group's exposed controls.
pub fn published(doc: &Composition) -> Vec<Control> {
    doc.published
        .iter()
        .chain(
            doc.groups
                .values()
                .flat_map(|group| group.controls.values()),
        )
        .cloned()
        .collect()
}
pub(crate) fn validate(doc: &Composition) -> Result<()> {
    if doc.groups.len() > 256 {
        return fail("composition exceeds 256 groups");
    }
    let mut owners = BTreeMap::new();
    for (id, group) in &doc.groups {
        if *id != group.id || doc.nodes.contains_key(id) {
            return fail("invalid group identity");
        }
        name(&group.label)?;
        if group.members.is_empty() || !group.members.contains(&group.output) {
            return fail("group needs members and one member output");
        }
        if group.controls.len() > MAX_CONTROLS || group.inputs.len() > 64 {
            return fail("group exceeds 64 controls or inputs");
        }
        for member in &group.members {
            if !doc.nodes.contains_key(member) || owners.insert(*member, *id).is_some() {
                return fail("group members must exist and belong to only one group");
            }
        }
        // Require every member to contribute to the public output. Besides making
        // the boundary unambiguous, this prevents collapsed orphan nodes.
        let mut reachable = BTreeSet::new();
        let mut todo = vec![group.output];
        while let Some(node) = todo.pop() {
            if group.members.contains(&node) && reachable.insert(node) {
                todo.extend(doc.nodes[&node].inputs.values().copied());
            }
        }
        if reachable != group.members {
            return fail("every group member must feed its output");
        }
        let mut expected = BTreeSet::new();
        for member in &group.members {
            for (port, source) in &doc.nodes[member].inputs {
                if !group.members.contains(source) {
                    expected.insert(Port {
                        node: *member,
                        port: port.clone(),
                    });
                }
            }
        }
        let mut actual = BTreeSet::new();
        for (key, input) in &group.inputs {
            name(key)?;
            name(&input.label)?;
            if input.targets.is_empty() {
                return fail("group input needs targets");
            }
            let mut shared_source = None;
            for target in &input.targets {
                if !expected.contains(target) || !actual.insert(target.clone()) {
                    return fail("invalid or duplicate group input target");
                }
                let source = doc.nodes[&target.node].inputs[&target.port];
                if shared_source
                    .replace(source)
                    .is_some_and(|old| old != source)
                {
                    return fail("group input targets must share one source");
                }
            }
        }
        if actual != expected {
            return fail("all external group inputs must be exposed");
        }
        let mut targets = BTreeSet::new();
        for (key, control) in &group.controls {
            name(key)?;
            name(&control.label)?;
            if !group.members.contains(&control.node)
                || !targets.insert((control.node, &control.parameter))
            {
                return fail("control must reference a unique parameter inside its group");
            }
            check_control(doc, control)?;
        }
    }
    for node in doc.nodes.values() {
        for source in node.inputs.values() {
            if let Some(owner) = owners.get(source) {
                let group = &doc.groups[owner];
                if owners.get(&node.id) != Some(owner) && *source != group.output {
                    return fail("connections leaving a group must use its output");
                }
            }
        }
    }
    if let Some(owner) = owners.get(&doc.output)
        && doc.groups[owner].output != doc.output
    {
        return fail("composition output must use the group's public output");
    }
    Ok(())
}

#[cfg(test)]
mod published_tests {
    use super::*;
    use crate::{Document, Node, Operation, Time, blank};
    fn blur() -> Operation {
        serde_json::from_value(
            crate::registry::TYPES
                .iter()
                .find(|t| t.type_id == "blur")
                .unwrap()
                .default_operation(),
        )
        .unwrap()
    }
    fn apply(document: &mut Document, commands: &[Command]) -> Result<u64> {
        let revision = document.revision();
        document.apply(revision, commands)
    }
    #[test]
    fn published_controls_validate_save_follow_removed_nodes_and_hold_their_range() {
        let composition = blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
        assert!(
            !serde_json::to_string(&composition)
                .unwrap()
                .contains("published")
        );
        let source = composition.output;
        let mut document = Document::new(composition).unwrap();
        let node = Node::new("Soften", blur(), &[("image", source)]);
        let id = node.id;
        apply(
            &mut document,
            &[Command::Put {
                node: Box::new(node),
            }],
        )
        .unwrap();
        let mut softness = Control::new(document.snapshot(), id, "radius", "Softness").unwrap();
        let published = |list: Vec<Control>| Command::Published { published: list };
        apply(&mut document, &[published(vec![softness.clone()])]).unwrap();
        assert_eq!(published_of(&document), vec![softness.clone()]);
        assert_eq!(
            crate::describe::step(&[published(vec![])], document.snapshot()),
            "Stop showing a setting on Edit"
        );
        // Saved and read back as it was.
        let text = serde_json::to_string(document.snapshot()).unwrap();
        let back: Composition = serde_json::from_str(&text).unwrap();
        assert_eq!(back.published, vec![softness.clone()]);
        // Refused: twice the same parameter, an unknown one, a missing node, no name, a
        // range beyond the registered bounds, and more than 64.
        let mut unknown = softness.clone();
        unknown.parameter = "nothing".into();
        let mut missing = softness.clone();
        missing.node = uuid::Uuid::new_v4();
        let mut unnamed = softness.clone();
        unnamed.label = " ".into();
        let mut wide = softness.clone();
        wide.range = Some([-5.0, 1.0e9]);
        for bad in [
            vec![softness.clone(), softness.clone()],
            vec![unknown],
            vec![missing],
            vec![unnamed],
            vec![wide],
            vec![softness.clone(); MAX_CONTROLS + 1],
        ] {
            assert!(apply(&mut document, &[published(bad)]).is_err());
        }
        assert_eq!(published_of(&document), vec![softness.clone()]);
        // A range is held on every edit of the parameter.
        softness.range = Some([0.0, 10.0]);
        apply(&mut document, &[published(vec![softness.clone()])]).unwrap();
        let set = |value: f32| Command::SetParameter {
            id,
            parameter: "radius".into(),
            value: serde_json::json!({"kind": "constant", "value": value}),
        };
        apply(&mut document, &[set(4.0)]).unwrap();
        assert!(apply(&mut document, &[set(20.0)]).is_err());
        // Removing the node removes what it published.
        apply(&mut document, &[Command::Remove { id }]).unwrap();
        assert!(document.snapshot().published.is_empty());
    }
    fn published_of(document: &Document) -> Vec<Control> {
        published(document.snapshot())
    }
}
