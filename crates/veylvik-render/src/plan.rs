//! Compile authored nodes to explicit passes, then assign image lifetimes.
use crate::{Result, error};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{CompiledScalar, Composition, Node, NodeId, Operation, Scalar};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kernel {
    Text,
    Solid,
    Noise,
    FractalNoise,
    Fire,
    Atmosphere,
    Material,
    Patina,
    SunGlints,
    Glass,
    SurfaceReflection,
    MetallicShine,
    Exposure,
    Transform,
    Displace,
    MorphologyHorizontal,
    MorphologyVertical,
    BlurHorizontal,
    BlurVertical,
    Merge,
    Blend,
    Image,
    Affine,
    Ellipse,
    Mask,
    CombineMask,
    ChannelMask,
    Colorize,
    Gradient,
}
impl Kernel {
    pub(crate) const ALL: [Self; 29] = [
        Self::Text,
        Self::Solid,
        Self::Noise,
        Self::FractalNoise,
        Self::Fire,
        Self::Atmosphere,
        Self::Material,
        Self::Patina,
        Self::SunGlints,
        Self::Glass,
        Self::SurfaceReflection,
        Self::MetallicShine,
        Self::Exposure,
        Self::Transform,
        Self::Displace,
        Self::MorphologyHorizontal,
        Self::MorphologyVertical,
        Self::BlurHorizontal,
        Self::BlurVertical,
        Self::Merge,
        Self::Blend,
        Self::Image,
        Self::Affine,
        Self::Ellipse,
        Self::Mask,
        Self::CombineMask,
        Self::ChannelMask,
        Self::Colorize,
        Self::Gradient,
    ];
    pub(crate) fn entry(self) -> &'static str {
        match self {
            Self::Text => "text_image",
            Self::Solid => "solid",
            Self::Fire => "fire",
            Self::Atmosphere => "atmosphere",
            Self::Material => "material",
            Self::Patina => "patina",
            Self::SunGlints => "sun_glints",
            Self::Glass => "glass",
            Self::SurfaceReflection => "surface_reflection",
            Self::MetallicShine => "metallic_shine",
            Self::FractalNoise => "fractal_noise",
            Self::Noise => "noise",
            Self::Exposure => "exposure",
            Self::Displace => "displace",
            Self::Transform => "transform",
            Self::MorphologyHorizontal => "morphology_horizontal",
            Self::MorphologyVertical => "morphology_vertical",
            Self::BlurHorizontal => "blur_horizontal",
            Self::BlurVertical => "blur_vertical",
            Self::Merge => "merge",
            Self::Blend => "blend",
            Self::Image => "image",
            Self::Affine => "affine",
            Self::Ellipse => "ellipse",
            Self::ChannelMask => "channel_mask",
            Self::Colorize => "colorize",
            Self::Gradient => "gradient",
            Self::CombineMask => "combine_mask",
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
    root: NodeId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) nodes: Vec<Node>,
    pub(crate) curves: Vec<BTreeMap<&'static str, CompiledScalar>>,
    pub(crate) passes: Vec<Pass>,
    pub(crate) output_slot: usize,
    pub(crate) slot_count: usize,
}
impl Plan {
    pub fn compile(composition: &Composition) -> Result<Self> {
        Self::compile_output(composition, composition.output)
    }
    pub fn compile_output(composition: &Composition, output: NodeId) -> Result<Self> {
        composition.validate()?;
        if !composition.nodes.contains_key(&output) {
            return Err(error("requested output node is missing"));
        }
        // Resolve bypasses before traversal, so disabled compound effects dispatch no work.
        let mut effective = None;
        if composition.nodes.values().any(|node| node.bypass) {
            let mut next = composition.clone();
            for node in composition.nodes.values().filter(|n| n.bypass) {
                let source = if let Some(group) =
                    composition.groups.values().find(|g| g.output == node.id)
                {
                    let external: BTreeSet<_> = group
                        .members
                        .iter()
                        .flat_map(|id| composition.nodes[id].inputs.values())
                        .filter(|id| !group.members.contains(id))
                        .copied()
                        .collect();
                    (external.len() == 1).then(|| *external.first().unwrap())
                } else if node.inputs.len() == 1 {
                    node.inputs.values().next().copied()
                } else {
                    None
                };
                let source = source.ok_or_else(|| {
                    error("Bypass needs one source image or a single-input effect group")
                })?;
                let target = next.nodes.get_mut(&node.id).unwrap();
                target.operation = Operation::Transform {
                    x: 0.0.into(),
                    y: 0.0.into(),
                };
                target.inputs = [("image".into(), source)].into();
                target.bypass = false;
            }
            effective = Some(next);
        }
        let composition = effective.as_ref().unwrap_or(composition);
        // Depth-first postorder completes each input branch before starting its
        // siblings. UUID-sorted ready queues can retain all generators at once.
        // Use an explicit stack so valid deep documents cannot overflow Rust's stack.
        let mut visited = BTreeSet::new();
        let mut todo = vec![(output, false)];
        let mut nodes = Vec::new();
        while let Some((id, expanded)) = todo.pop() {
            let node = &composition.nodes[&id];
            if expanded {
                nodes.push(node.clone());
            } else if visited.insert(id) {
                todo.push((id, true));
                for port in node.operation.inputs().iter().rev() {
                    todo.push((node.inputs[*port], false));
                }
            }
        }
        let mut passes = Vec::new();
        let mut outputs = BTreeMap::new();
        let mut shared = BTreeMap::new();
        for (index, node) in nodes.iter().enumerate() {
            let mut inputs = [Input::Empty, Input::Empty];
            for (i, port) in node.operation.inputs().iter().enumerate() {
                inputs[i] = Input::Value(outputs[&node.inputs[*port]]);
            }
            // Preserve authored nodes, but alias exact constant identity effects
            // before lifetime assignment so they need no dispatch or image slot.
            if identity(&node.operation) {
                let Input::Value(value) = inputs[0] else {
                    unreachable!()
                };
                outputs.insert(node.id, value);
                continue;
            }
            // Collision-free structural keys are built once at compile time.
            // Labels, UUIDs and layout do not affect pure operation results.
            let key = if shareable(&node.operation) {
                Some((
                    serde_json::to_vec(&node.operation).map_err(error)?,
                    inputs.each_ref().map(|input| match input {
                        Input::Value(value) => Some(*value),
                        _ => None,
                    }),
                ))
            } else {
                None
            };
            if let Some(key) = &key
                && let Some(value) = shared.get(key)
            {
                outputs.insert(node.id, *value);
                continue;
            }
            let kernel = match &node.operation {
                Operation::Solid { .. } => Kernel::Solid,
                Operation::Fire { .. } => Kernel::Fire,
                Operation::Atmosphere { .. } => Kernel::Atmosphere,
                Operation::Material { .. } => Kernel::Material,
                Operation::Patina { .. } => Kernel::Patina,
                Operation::SunGlints { .. } => Kernel::SunGlints,
                Operation::Glass { .. } => Kernel::Glass,
                Operation::SurfaceReflection { .. } => Kernel::SurfaceReflection,
                Operation::MetallicShine { .. } => Kernel::MetallicShine,
                Operation::FractalNoise { .. } => Kernel::FractalNoise,
                Operation::Noise { .. } => Kernel::Noise,
                Operation::Exposure { .. } => Kernel::Exposure,
                Operation::Displace { .. } => Kernel::Displace,
                Operation::Transform { .. } => Kernel::Transform,
                Operation::Merge {} => Kernel::Merge,
                Operation::Blend { .. } => Kernel::Blend,
                Operation::Affine { .. } => Kernel::Affine,
                Operation::Ellipse { .. } => Kernel::Ellipse,
                Operation::ChannelMask { .. } => Kernel::ChannelMask,
                Operation::Colorize { .. } => Kernel::Colorize,
                Operation::Gradient { .. } => Kernel::Gradient,
                Operation::CombineMask { .. } => Kernel::CombineMask,
                Operation::Mask {} => Kernel::Mask,
                Operation::Path { .. } | Operation::Text { .. } => {
                    if composition.width > crate::MAX_VECTOR_DIMENSION
                        || composition.height > crate::MAX_VECTOR_DIMENSION
                    {
                        return Err(error("vector mask dimensions exceed 4096"));
                    }
                    inputs[0] = Input::Vector(index);
                    if matches!(node.operation, Operation::Text { .. }) {
                        Kernel::Text
                    } else {
                        Kernel::Image
                    }
                }
                Operation::Footage { .. } => {
                    inputs[0] = Input::Asset(footage_key(node.id));
                    Kernel::Image
                }
                Operation::Image { asset } => {
                    inputs[0] = Input::Asset(asset.clone());
                    Kernel::Image
                }
                Operation::Morphology { .. } => {
                    let intermediate = passes.len();
                    passes.push(Pass {
                        node: index,
                        kernel: Kernel::MorphologyHorizontal,
                        inputs,
                        output: intermediate,
                    });
                    inputs = [Input::Value(intermediate), Input::Empty];
                    Kernel::MorphologyVertical
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
            if let Some(key) = key {
                shared.insert(key, value);
            }
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
        let curves = nodes
            .iter()
            .map(|node| {
                node.operation
                    .descriptor()
                    .parameters
                    .iter()
                    .filter_map(|parameter| {
                        node.operation.scalar(parameter.name).map(|scalar| {
                            scalar.compile().map(|compiled| (parameter.name, compiled))
                        })
                    })
                    .collect::<std::result::Result<BTreeMap<_, _>, _>>()
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self {
            root: output,
            curves,
            width: composition.width,
            height: composition.height,
            nodes,
            passes,
            output_slot: slots[final_value],
            slot_count,
        })
    }
    /// Whether a validated composition has the same reachable render dependencies.
    /// Labels, positions, frame rate and disconnected nodes do not affect a plan.
    /// The entire candidate is validated, including disconnected nodes.
    pub fn matches_composition(&self, composition: &Composition, output: NodeId) -> Result<bool> {
        composition.validate()?;
        if self.root != output
            || self.width != composition.width
            || self.height != composition.height
        {
            return Ok(false);
        }
        for before in &self.nodes {
            let Some(after) = composition.nodes.get(&before.id) else {
                return Ok(false);
            };
            if before.version != after.version || before.inputs != after.inputs {
                return Ok(false);
            }
            // Exact serialized definitions preserve signed zero and timestamp
            // representation. Only used after an authoring revision changes.
            if serde_json::to_vec(&before.operation).map_err(error)?
                != serde_json::to_vec(&after.operation).map_err(error)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn required_inputs(&self) -> BTreeSet<&str> {
        self.passes
            .iter()
            .flat_map(|p| &p.inputs)
            .filter_map(|i| match i {
                Input::Asset(key) => Some(key.as_str()),
                _ => None,
            })
            .collect()
    }
    pub fn required_assets(&self) -> BTreeSet<&str> {
        self.nodes
            .iter()
            .filter_map(|n| match &n.operation {
                Operation::Image { asset } | Operation::Footage { asset, .. } => {
                    Some(asset.as_str())
                }
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
            .filter(|n| matches!(n.operation, Operation::Path { .. } | Operation::Text { .. }))
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

fn identity(operation: &Operation) -> bool {
    let zero = |value: &Scalar| matches!(value,Scalar::Constant { value } if *value==0.0);
    match operation {
        Operation::Exposure { stops } => zero(stops),
        Operation::Blur { radius } => zero(radius),
        Operation::Transform { x, y } => zero(x) && zero(y),
        _ => false,
    }
}

fn shareable(operation: &Operation) -> bool {
    // External/vector sources keep their existing ownership and resource policy.
    // New operation kinds must opt in explicitly after their purity is established.
    matches!(
        operation,
        Operation::Solid { .. }
            | Operation::Fire { .. }
            | Operation::Atmosphere { .. }
            | Operation::Material { .. }
            | Operation::Patina { .. }
            | Operation::SunGlints { .. }
            | Operation::Glass { .. }
            | Operation::SurfaceReflection { .. }
            | Operation::MetallicShine { .. }
            | Operation::FractalNoise { .. }
            | Operation::Noise { .. }
            | Operation::Exposure { .. }
            | Operation::Displace { .. }
            | Operation::Transform { .. }
            | Operation::Morphology { .. }
            | Operation::Blur { .. }
            | Operation::Merge {}
            | Operation::Blend { .. }
            | Operation::Affine { .. }
            | Operation::Ellipse { .. }
            | Operation::ChannelMask { .. }
            | Operation::Colorize { .. }
            | Operation::Gradient { .. }
            | Operation::CombineMask { .. }
            | Operation::Mask {}
    )
}

pub(crate) fn footage_key(id: veylvik_document::NodeId) -> String {
    format!("\0footage:{id}")
}
