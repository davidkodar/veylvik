//! Editable effect recipes expand into ordinary nodes and atomic Put commands.
use crate::{BlendMode, Command, Composition, MaskMode, Node, NodeId, Operation, Result, fail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recipe {
    Glow,
    Outline,
}
impl Recipe {
    pub const ALL: [Self; 2] = [Self::Glow, Self::Outline];
    pub fn id(self) -> &'static str {
        match self {
            Self::Glow => "glow",
            Self::Outline => "outline",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Glow => "Glow effect",
            Self::Outline => "Outline effect",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Glow => {
                "Add editable 8-pixel blur, +1 stop exposure and additive blend nodes around the selected image."
            }
            Self::Outline => {
                "Add an editable 4-pixel outside alpha outline, white color and foreground composite around the selected image."
            }
        }
    }
    pub fn node_count(self) -> usize {
        match self {
            Self::Glow => 3,
            Self::Outline => 4,
        }
    }
    /// Prepare an effect without changing the document or composition output.
    pub fn build(self, composition: &Composition, input: NodeId) -> Result<Effect> {
        composition.validate()?;
        composition.encoded_size()?;
        if !composition.nodes.contains_key(&input) {
            return fail("effect input does not exist");
        }
        if composition.nodes.len() + self.node_count() > crate::limits::MAX_NODES {
            return fail("effect would exceed the 4096-node document limit");
        }
        let mut occupied: BTreeSet<_> = composition.nodes.keys().copied().collect();
        let mut nodes = Vec::new();
        let mut roles = BTreeMap::new();
        let mut add = |role, label, operation, ports: &[(&str, NodeId)]| {
            let mut node = Node::new(label, operation, ports);
            while !occupied.insert(node.id) {
                node.id = NodeId::new_v4();
            }
            let id = node.id;
            nodes.push(node);
            roles.insert(role, id);
            id
        };
        let root = match self {
            Self::Glow => {
                let blur = add(
                    "blur",
                    "Glow blur",
                    Operation::Blur { radius: 8.0.into() },
                    &[("image", input)],
                );
                let bright = add(
                    "brightness",
                    "Glow brightness",
                    Operation::Exposure { stops: 1.0.into() },
                    &[("image", blur)],
                );
                add(
                    "composite",
                    "Glow composite",
                    Operation::Blend {
                        mode: BlendMode::Add,
                        opacity: 1.0.into(),
                    },
                    &[("background", input), ("foreground", bright)],
                )
            }
            Self::Outline => {
                let grow = add(
                    "grow",
                    "Outline width",
                    Operation::Morphology { radius: 4.0.into() },
                    &[("image", input)],
                );
                let outline = add(
                    "outline",
                    "Outside outline",
                    Operation::CombineMask {
                        mode: MaskMode::Subtract,
                    },
                    &[("mask", grow), ("other", input)],
                );
                let color = add(
                    "color",
                    "Outline color",
                    Operation::Colorize {
                        low: [1.0; 4],
                        high: [1.0; 4],
                    },
                    &[("image", outline)],
                );
                add(
                    "composite",
                    "Outline composite",
                    Operation::Merge {},
                    &[("background", input), ("foreground", color)],
                )
            }
        };
        Ok(Effect { root, nodes, roles })
    }
}
/// Nodes are in dependency order. Roles let automation find editable controls
/// without relying on display labels or reconstructing the expanded graph.
pub struct Effect {
    pub root: NodeId,
    pub nodes: Vec<Node>,
    pub roles: BTreeMap<&'static str, NodeId>,
}
impl Effect {
    pub fn into_commands(self) -> Vec<Command> {
        self.nodes
            .into_iter()
            .map(|node| Command::Put {
                node: Box::new(node),
            })
            .collect()
    }
}
pub fn catalog() -> serde_json::Value {
    serde_json::json!(Recipe::ALL.map(|recipe|serde_json::json!({"id":recipe.id(),"label":recipe.label(),"description":recipe.description(),"node_count":recipe.node_count()})))
}
