//! Bounded node selection transfer with explicit external-input boundaries.
use crate::{Composition, Node, NodeId, Operation, Result, editing::BranchCopy, fail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeClipboard {
    pub veylvik_nodes: u32,
    pub source_session: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_base: Option<String>,
    pub selected: BTreeSet<NodeId>,
    pub external: BTreeSet<NodeId>,
    pub fragment: Composition,
}
impl NodeClipboard {
    pub fn capture(
        doc: &Composition,
        source_session: NodeId,
        selected: BTreeSet<NodeId>,
        active: NodeId,
    ) -> Result<Self> {
        doc.validate()?;
        if selected.is_empty() || selected.iter().any(|id| !doc.nodes.contains_key(id)) {
            return fail("Select nodes to copy");
        }
        let external: BTreeSet<_> = selected
            .iter()
            .flat_map(|id| doc.nodes[id].inputs.values().copied())
            .filter(|id| !selected.contains(id))
            .collect();
        let mut fragment = doc.clone();
        fragment.nodes.retain(|id, _| selected.contains(id));
        fragment
            .groups
            .retain(|_, group| group.members.iter().all(|id| selected.contains(id)));
        for id in &external {
            let mut stub = Node::new(
                &format!("Input: {}", doc.nodes[id].label),
                Operation::Solid(crate::nodes::Solid { color: [0.0; 4] }),
                &[],
            );
            stub.id = *id;
            fragment.nodes.insert(*id, stub);
        }
        fragment.output = if selected.contains(&active) {
            active
        } else {
            *selected
                .iter()
                .find(|id| {
                    !selected
                        .iter()
                        .any(|other| doc.nodes[other].inputs.values().any(|source| source == *id))
                })
                .unwrap_or(selected.first().unwrap())
        };
        let clipboard = Self {
            veylvik_nodes: 1,
            source_session,
            source_base: None,
            selected,
            external,
            fragment,
        };
        clipboard.validate()?;
        Ok(clipboard)
    }
    pub fn decode(text: &str) -> Result<Self> {
        if text.len() > crate::limits::MAX_DOCUMENT_BYTES {
            return fail("Node clipboard exceeds 16 MiB");
        }
        let clipboard: Self = serde_json::from_str(text).map_err(|_| {
            crate::Error("Clipboard contains no valid Veylvik node selection".into())
        })?;
        clipboard.validate()?;
        Ok(clipboard)
    }
    pub fn encode(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|e| crate::Error(e.to_string()))
    }
    pub fn validate(&self) -> Result<()> {
        if self.veylvik_nodes != 1
            || self.selected.is_empty()
            || !self.selected.contains(&self.fragment.output)
            || !self.selected.is_disjoint(&self.external)
            || self
                .selected
                .union(&self.external)
                .copied()
                .collect::<BTreeSet<_>>()
                != self.fragment.nodes.keys().copied().collect()
        {
            return fail("Invalid node clipboard structure");
        }
        if self
            .source_base
            .as_ref()
            .is_some_and(|base| !std::path::Path::new(base).is_absolute())
        {
            return fail("Clipboard image base must be absolute");
        }
        for id in &self.external {
            let node = &self.fragment.nodes[id];
            if !matches!(node.operation,Operation::Solid(crate::nodes::Solid {color}) if color==[0.0;4])
                || !node.inputs.is_empty()
                || self
                    .fragment
                    .groups
                    .values()
                    .any(|g| g.members.contains(id))
            {
                return fail("Invalid clipboard input boundary");
            }
        }
        self.fragment.validate()?;
        crate::limits::encoded_size(self)?;
        Ok(())
    }
    /// Same-session boundaries may reconnect only to surviving original node IDs.
    pub fn paste(&self, doc: &Composition, session: NodeId) -> Result<BranchCopy> {
        self.validate()?;
        doc.validate()?;
        let linked: BTreeSet<_> = self
            .external
            .iter()
            .filter(|id| self.source_session == session && doc.nodes.contains_key(id))
            .copied()
            .collect();
        if doc.nodes.len() + self.fragment.nodes.len() - linked.len() > crate::limits::MAX_NODES {
            return fail("Paste would exceed the 4096-node document limit");
        }
        let mut allocated: BTreeSet<_> =
            doc.nodes.keys().chain(doc.groups.keys()).copied().collect();
        let mut fresh = || loop {
            let id = NodeId::new_v4();
            if allocated.insert(id) {
                break id;
            }
        };
        let mapping: BTreeMap<_, _> = self
            .fragment
            .nodes
            .keys()
            .map(|id| (*id, if linked.contains(id) { *id } else { fresh() }))
            .collect();
        let mut nodes: BTreeMap<_, _> = self
            .fragment
            .nodes
            .iter()
            .filter(|(id, _)| !linked.contains(id))
            .map(|(id, node)| {
                let mut node = node.clone();
                node.id = mapping[id];
                if self.source_session != session
                    && let Some(base) = &self.source_base
                    && let Operation::Image(crate::nodes::Image { asset })
                    | Operation::Footage(crate::nodes::Footage { asset, .. }) =
                        &mut node.operation
                    && std::path::Path::new(asset).is_relative()
                {
                    *asset = std::path::Path::new(base)
                        .join(&*asset)
                        .to_string_lossy()
                        .into_owned();
                }
                for input in node.inputs.values_mut() {
                    *input = mapping[input];
                }
                (node.id, node)
            })
            .collect();
        for node in nodes.values_mut() {
            crate::expression::moved_links(&mut node.operation, &mapping)?;
        }
        // Missing boundary sources sit just left of the copied selection.
        let left = self
            .selected
            .iter()
            .filter_map(|id| self.fragment.nodes[id].position)
            .map(|p| p.x)
            .reduce(f32::min)
            .unwrap_or(0.0)
            - 220.0;
        let top = self
            .selected
            .iter()
            .filter_map(|id| self.fragment.nodes[id].position)
            .map(|p| p.y)
            .reduce(f32::min)
            .unwrap_or(0.0);
        for (index, id) in self.external.difference(&linked).enumerate() {
            nodes.get_mut(&mapping[id]).unwrap().position = Some(crate::NodePosition {
                x: left,
                y: top + index as f32 * 100.0,
            });
        }
        let groups = self
            .fragment
            .groups
            .values()
            .map(|group| {
                let mut group = group.clone();
                group.id = fresh();
                group.output = mapping[&group.output];
                group.members = group.members.iter().map(|id| mapping[id]).collect();
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
            root: mapping[&self.fragment.output],
            mapping,
            nodes,
            groups,
        })
    }
}
