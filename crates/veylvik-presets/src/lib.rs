//! Ready-made scenes as lists of document commands: every scene is ordinary nodes the user can open and change, sized to the composition it is added to.
pub mod authoring;
mod gold_title;
pub mod particles;
pub use gold_title::gold_title;
use serde_json::{Value, json};
use veylvik_document::{
    Command, Composition, Error, Node, NodeId, NodePosition, Operation, Result, registry::NodeType,
};

/// A scene ready to add: its nodes as one transaction, and the node that shows it.
pub struct Built {
    pub commands: Vec<Command>,
    pub output: NodeId,
    /// The node to select afterwards, where the scene's words are edited.
    pub edit: Option<NodeId>,
}
/// A constant animatable value.
fn c(value: f32) -> Value {
    json!({"kind": "constant", "value": value})
}
/// Collects a scene's nodes, laid out on a grid below what is in the graph already.
struct Scene {
    nodes: Vec<Node>,
    top: f32,
}
impl Scene {
    fn below(composition: &Composition) -> Self {
        let top = composition
            .nodes
            .values()
            .filter_map(|n| n.position.map(|p| p.y))
            .fold(0.0, f32::max);
        Self {
            nodes: Vec::new(),
            top: top + 220.0,
        }
    }
    /// Adds a node of `kind` with `set` written over its default parameters.
    fn add(
        &mut self,
        label: &str,
        kind: &NodeType,
        set: Value,
        inputs: &[(&str, NodeId)],
    ) -> Result<NodeId> {
        let mut operation = kind.default_operation();
        if let (Some(target), Some(values)) = (operation.as_object_mut(), set.as_object()) {
            for (key, value) in values {
                target.insert(key.clone(), value.clone());
            }
        }
        let operation: Operation = serde_json::from_value(operation)
            .map_err(|e| Error(format!("preset node {label}: {e}")))?;
        let mut node = Node::new(label, operation, inputs);
        let i = self.nodes.len();
        node.position = Some(NodePosition {
            x: (i % 8) as f32 * 230.0,
            y: self.top + (i / 8) as f32 * 145.0,
        });
        let id = node.id;
        self.nodes.push(node);
        Ok(id)
    }
    /// Merges any number of scene parts, eight to a Merge scenes node.
    fn merge(&mut self, label: &str, mut parts: Vec<NodeId>) -> Result<NodeId> {
        let port = |i: usize| {
            if i == 0 {
                "scene".to_string()
            } else {
                format!("scene_{}", i + 1)
            }
        };
        let mut level = 1;
        while parts.len() > 8 {
            let mut merged = Vec::new();
            for (k, group) in parts.chunks(8).enumerate() {
                let names: Vec<String> = (0..group.len()).map(port).collect();
                let inputs: Vec<(&str, NodeId)> = names
                    .iter()
                    .map(String::as_str)
                    .zip(group.iter().copied())
                    .collect();
                merged.push(self.add(
                    &format!("{label} part {level}.{}", k + 1),
                    &veylvik_document::nodes::MERGE_SCENES,
                    json!({}),
                    &inputs,
                )?);
            }
            parts = merged;
            level += 1;
        }
        let names: Vec<String> = (0..parts.len()).map(port).collect();
        let inputs: Vec<(&str, NodeId)> = names
            .iter()
            .map(String::as_str)
            .zip(parts.iter().copied())
            .collect();
        self.add(
            label,
            &veylvik_document::nodes::MERGE_SCENES,
            json!({}),
            &inputs,
        )
    }
    fn built(self, output: NodeId, edit: Option<NodeId>, mut extra: Vec<Command>) -> Built {
        let mut commands: Vec<Command> = self
            .nodes
            .into_iter()
            .map(|node| Command::Put {
                node: Box::new(node),
            })
            .collect();
        commands.append(&mut extra);
        Built {
            commands,
            output,
            edit,
        }
    }
}
