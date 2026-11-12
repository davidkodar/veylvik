//! Portable, versioned effect graphs. Boundary sources are supplied by the host;
//! files contain no filesystem dependencies or application/session state.
pub mod builtin;
use crate::{
    Command, Composition, Error, Node, NodeId, NodePosition, Operation, Result, Time, fail,
    groups::Group,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub schema: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Time,
    pub frame_count: u32,
    pub group: Group,
    /// Public input edges are omitted; Group.inputs describes where to bind them.
    pub nodes: BTreeMap<NodeId, Node>,
}
pub struct Instance {
    pub group: NodeId,
    pub output: NodeId,
    pub commands: Vec<Command>,
}
impl Preset {
    pub fn capture(doc: &Composition, id: NodeId) -> Result<Self> {
        doc.validate()?;
        let group = doc
            .groups
            .get(&id)
            .ok_or_else(|| Error("effect group does not exist".into()))?
            .clone();
        let mut nodes: BTreeMap<_, _> = group
            .members
            .iter()
            .map(|id| (*id, doc.nodes[id].clone()))
            .collect();
        for input in group.inputs.values() {
            for target in &input.targets {
                nodes
                    .get_mut(&target.node)
                    .unwrap()
                    .inputs
                    .remove(&target.port);
            }
        }
        let preset = Self {
            schema: 1,
            width: doc.width,
            height: doc.height,
            frame_rate: doc.frame_rate,
            frame_count: doc.frame_count,
            group,
            nodes,
        };
        preset.validate()?;
        Ok(preset)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 {
            return fail("unsupported effect preset schema");
        }
        crate::limits::encoded_size(self)?;
        if self.nodes.keys().copied().collect::<BTreeSet<_>>() != self.group.members {
            return fail("preset nodes must exactly match its group members");
        }
        if self
            .nodes
            .values()
            .any(|n| matches!(n.operation, Operation::Image { .. }))
        {
            return fail("Expose image sources as effect inputs before saving a portable preset");
        }
        let mut nodes = self.nodes.clone();
        for input in self.group.inputs.values() {
            let mut stub = Node::new("Input", Operation::Solid { color: [0.0; 4] }, &[]);
            while nodes.contains_key(&stub.id) || stub.id == self.group.id {
                stub.id = NodeId::new_v4();
            }
            for target in &input.targets {
                let node = nodes
                    .get_mut(&target.node)
                    .ok_or_else(|| Error("missing preset input target".into()))?;
                if node.inputs.insert(target.port.clone(), stub.id).is_some() {
                    return fail("preset public input edge must not contain a source");
                }
            }
            nodes.insert(stub.id, stub);
        }
        Composition {
            schema: 1,
            width: self.width,
            height: self.height,
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            output: self.group.output,
            groups: [(self.group.id, self.group.clone())].into(),
            nodes,
        }
        .validate()?;
        Ok(())
    }
    /// Keep authored pixel units and exact animation times. Hosts explicitly bind
    /// every public input; insertion never silently changes the composition output.
    pub fn instantiate(
        &self,
        doc: &Composition,
        inputs: &BTreeMap<String, NodeId>,
        position: Option<NodePosition>,
    ) -> Result<Instance> {
        self.validate()?;
        doc.validate()?;
        if inputs.keys().collect::<Vec<_>>() != self.group.inputs.keys().collect::<Vec<_>>() {
            return fail("bind exactly the preset's named inputs");
        }
        if doc.nodes.len() + self.nodes.len() > crate::limits::MAX_NODES {
            return fail("preset insertion exceeds the node limit");
        }
        let mut used: BTreeSet<_> = doc.nodes.keys().chain(doc.groups.keys()).copied().collect();
        let mut allocate = || loop {
            let id = NodeId::new_v4();
            if used.insert(id) {
                break id;
            }
        };
        let mapping: BTreeMap<_, _> = self.nodes.keys().map(|id| (*id, allocate())).collect();
        let mut group = self.group.clone();
        group.id = allocate();
        group.output = mapping[&group.output];
        group.members = mapping.values().copied().collect();
        for control in group.controls.values_mut() {
            control.node = mapping[&control.node];
        }
        let mut nodes = self.nodes.clone();
        for (name, input) in &self.group.inputs {
            let source = inputs[name];
            if !doc.nodes.contains_key(&source) {
                return fail("preset input source does not exist");
            }
            for target in &input.targets {
                nodes
                    .get_mut(&target.node)
                    .unwrap()
                    .inputs
                    .insert(target.port.clone(), source);
            }
        }
        let original = self.nodes[&self.group.output].position;
        let mut commands = Vec::new();
        for (old, mut node) in nodes {
            node.id = mapping[&old];
            // Only authored internal edges are remapped. A caller's source UUID
            // may happen to equal a preset-local ID in a different composition.
            for (port, source) in &mut node.inputs {
                if self.nodes[&old].inputs.contains_key(port) {
                    *source = mapping[source];
                }
            }
            if let Some(anchor) = position {
                node.position = if old == self.group.output {
                    Some(anchor)
                } else {
                    original.zip(node.position).map(|(origin, p)| NodePosition {
                        x: anchor.x + p.x - origin.x,
                        y: anchor.y + p.y - origin.y,
                    })
                };
            }
            commands.push(Command::Put {
                node: Box::new(node),
            });
        }
        for input in group.inputs.values_mut() {
            for target in &mut input.targets {
                target.node = mapping[&target.node];
            }
        }
        commands.push(Command::PutGroup {
            group: Box::new(group.clone()),
        });
        let owner = crate::Document::new(doc.clone())?;
        owner.prepare(0, &commands)?;
        Ok(Instance {
            group: group.id,
            output: group.output,
            commands,
        })
    }
}

/// Shared transport selector: either a bundled starter or a current authored group.
pub fn read(
    doc: &Composition,
    group: Option<&serde_json::Value>,
    starter: Option<&serde_json::Value>,
) -> Result<Preset> {
    match (group, starter) {
        (Some(group), None) => {
            let id = group
                .as_str()
                .ok_or_else(|| Error("group must be a UUID".into()))?
                .parse()
                .map_err(|_| Error("invalid group UUID".into()))?;
            Preset::capture(doc, id)
        }
        (None, Some(starter)) => builtin::build(
            starter
                .as_str()
                .ok_or_else(|| Error("starter must be a string".into()))?,
        ),
        _ => fail("supply exactly one of group or starter"),
    }
}
