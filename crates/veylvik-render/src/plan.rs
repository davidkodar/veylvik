//! Compile authored nodes to explicit passes, then assign image lifetimes.
use crate::{Result, error};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{Composition, Node, NodeId, Operation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kernel {
    Solid,
    Noise,
    Exposure,
    Transform,
    BlurHorizontal,
    BlurVertical,
    Merge,
    Image,
    Affine,
    Ellipse,
    Mask,
}
impl Kernel {
    pub(crate) const ALL: [Self; 11] = [
        Self::Solid,
        Self::Noise,
        Self::Exposure,
        Self::Transform,
        Self::BlurHorizontal,
        Self::BlurVertical,
        Self::Merge,
        Self::Image,
        Self::Affine,
        Self::Ellipse,
        Self::Mask,
    ];
    pub(crate) fn entry(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Noise => "noise",
            Self::Exposure => "exposure",
            Self::Transform => "transform",
            Self::BlurHorizontal => "blur_horizontal",
            Self::BlurVertical => "blur_vertical",
            Self::Merge => "merge",
            Self::Image => "image",
            Self::Affine => "affine",
            Self::Ellipse => "ellipse",
            Self::Mask => "mask",
        }
    }
}
#[derive(Clone)]
pub(crate) enum Input {
    Empty,
    Value(usize),
    Asset(String),
    Vector(usize),
}
pub(crate) struct Pass {
    pub node: usize,
    pub kernel: Kernel,
    pub inputs: [Input; 2],
    pub output: usize,
}
#[derive(serde::Serialize)]
pub struct PassInfo {
    pub node: NodeId,
    pub kernel: Kernel,
    pub input_slots: Vec<usize>,
    pub output_slot: usize,
}
/// Immutable device-independent execution plan. UI layout never participates.
pub struct Plan {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) nodes: Vec<Node>,
    pub(crate) passes: Vec<Pass>,
    pub(crate) output_slot: usize,
    pub(crate) slot_count: usize,
}
impl Plan {
    pub fn compile(composition: &Composition) -> Result<Self> {
        Self::compile_output(composition, composition.output)
    }
    pub fn compile_output(composition: &Composition, output: NodeId) -> Result<Self> {
        let order = composition.validate()?;
        if !composition.nodes.contains_key(&output) {
            return Err(error("requested output node is missing"));
        }
        let mut reachable = BTreeSet::new();
        let mut todo = vec![output];
        while let Some(id) = todo.pop() {
            if reachable.insert(id) {
                todo.extend(composition.nodes[&id].inputs.values().copied());
            }
        }
        let nodes: Vec<Node> = order
            .into_iter()
            .filter(|id| reachable.contains(id))
            .map(|id| composition.nodes[&id].clone())
            .collect();
        let mut passes = Vec::new();
        let mut outputs = BTreeMap::new();
        for (index, node) in nodes.iter().enumerate() {
            let mut inputs = [Input::Empty, Input::Empty];
            for (i, port) in node.operation.inputs().iter().enumerate() {
                inputs[i] = Input::Value(outputs[&node.inputs[*port]]);
            }
            let kernel = match &node.operation {
                Operation::Solid { .. } => Kernel::Solid,
                Operation::Noise { .. } => Kernel::Noise,
                Operation::Exposure { .. } => Kernel::Exposure,
                Operation::Transform { .. } => Kernel::Transform,
                Operation::Merge {} => Kernel::Merge,
                Operation::Affine { .. } => Kernel::Affine,
                Operation::Ellipse { .. } => Kernel::Ellipse,
                Operation::Mask {} => Kernel::Mask,
                Operation::Path { .. } => {
                    if composition.width > crate::MAX_VECTOR_DIMENSION
                        || composition.height > crate::MAX_VECTOR_DIMENSION
                    {
                        return Err(error("vector mask dimensions exceed 4096"));
                    }
                    inputs[0] = Input::Vector(index);
                    Kernel::Image
                }
                Operation::Image { asset } => {
                    inputs[0] = Input::Asset(asset.clone());
                    Kernel::Image
                }
                Operation::Blur { .. } => {
                    let intermediate = passes.len();
                    passes.push(Pass {
                        node: index,
                        kernel: Kernel::BlurHorizontal,
                        inputs,
                        output: intermediate,
                    });
                    inputs = [Input::Value(intermediate), Input::Empty];
                    Kernel::BlurVertical
                }
            };
            let value = passes.len();
            passes.push(Pass {
                node: index,
                kernel,
                inputs,
                output: value,
            });
            outputs.insert(node.id, value);
        }
        let final_value = outputs[&output];
        let mut uses = vec![0usize; passes.len()];
        for pass in &passes {
            for input in &pass.inputs {
                if let Input::Value(value) = input {
                    uses[*value] += 1;
                }
            }
        }
        uses[final_value] += 1;
        let mut slots = vec![0; passes.len()];
        let mut free = Vec::new();
        let mut slot_count = 0;
        for pass in &mut passes {
            let value = pass.output;
            let slot = free.pop().unwrap_or_else(|| {
                let slot = slot_count;
                slot_count += 1;
                slot
            });
            slots[value] = slot;
            pass.output = slot;
            // Retire inputs only after assigning the output, never within their own pass.
            for input in &mut pass.inputs {
                if let Input::Value(value) = input {
                    let source = *value;
                    *value = slots[source];
                    uses[source] -= 1;
                    if uses[source] == 0 {
                        free.push(slots[source]);
                    }
                }
            }
        }
        Ok(Self {
            width: composition.width,
            height: composition.height,
            nodes,
            passes,
            output_slot: slots[final_value],
            slot_count,
        })
    }
    pub fn required_assets(&self) -> BTreeSet<&str> {
        self.nodes
            .iter()
            .filter_map(|n| match &n.operation {
                Operation::Image { asset } => Some(asset.as_str()),
                _ => None,
            })
            .collect()
    }
    pub fn image_slots(&self) -> usize {
        self.slot_count
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }
    pub fn passes(&self) -> Vec<PassInfo> {
        self.passes
            .iter()
            .map(|pass| PassInfo {
                node: self.nodes[pass.node].id,
                kernel: pass.kernel,
                input_slots: pass
                    .inputs
                    .iter()
                    .filter_map(|input| match input {
                        Input::Value(slot) => Some(*slot),
                        _ => None,
                    })
                    .collect(),
                output_slot: pass.output,
            })
            .collect()
    }
    pub(crate) fn vector_bytes(&self) -> u64 {
        self.nodes
            .iter()
            .filter(|n| matches!(n.operation, Operation::Path { .. }))
            .count() as u64
            * u64::from(self.width)
            * u64::from(self.height)
            * 16
    }
    /// Working bytes when pixels stay on the GPU (only a 4-byte validation result is read).
    pub fn gpu_working_bytes(&self) -> u64 {
        self.working_bytes() - self.width as u64 * self.height as u64 * 16 + 8
    }
    /// Declared GPU buffer bytes, excluding source uploads and driver/staging overhead.
    pub fn working_bytes(&self) -> u64 {
        self.width as u64 * self.height as u64 * 16 * (self.slot_count as u64 + 1)
            + self.vector_bytes()
            + self.passes.len() as u64 * 48
            + 16
    }
}
