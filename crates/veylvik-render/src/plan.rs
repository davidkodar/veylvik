//! Compile authored nodes to explicit passes, then assign image lifetimes.
use crate::{Result, error};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{CompiledScalar, Composition, Node, NodeId, Operation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kernel {
    ToAcescg,
    FromAcescg,
    Text,
    Solid,
    Noise,
    FractalNoise,
    Fire,
    Dust,
    DripRoots,
    Drip,
    Atmosphere,
    Material,
    Condensation,
    Patina,
    GlintSeeds,
    SunGlints,
    Glass,
    SurfaceReflection,
    MetallicShine,
    Exposure,
    ColorCorrect,
    Fade,
    ChromaKey,
    LumaKey,
    Transform,
    Displace,
    MorphologyHorizontal,
    MorphologyVertical,
    Shadow,
    BlurHorizontal,
    BlurVertical,
    LensPrefilterHorizontal,
    LensPrefilterVertical,
    LensBlur,
    BevelHeightHorizontal,
    BevelHeightVertical,
    EnvironmentPyramid,
    FrostHorizontal,
    FrostVertical,
    Merge,
    Blend,
    SurfaceBlend,
    Image,
    Affine,
    PlanarWarp,
    Ellipse,
    Mask,
    CombineMask,
    ChannelMask,
    Colorize,
    Gradient,
    MattePack,
    KnownSubject,
    KnownBackground,
    MatteBoxHorizontal,
    MatteBoxVertical,
    MatteProject,
    MatteJoin,
    FuseForeground,
    FuseBackground,
    FuseBoxHorizontal,
    FuseBoxVertical,
    FuseEstimate,
    FuseFinish,
    Lut,
}
impl Kernel {
    pub(crate) const ALL: [Self; 65] = [
        Self::ToAcescg,
        Self::FromAcescg,
        Self::Text,
        Self::Solid,
        Self::Noise,
        Self::FractalNoise,
        Self::Fire,
        Self::Dust,
        Self::DripRoots,
        Self::Drip,
        Self::Atmosphere,
        Self::Material,
        Self::Condensation,
        Self::Patina,
        Self::GlintSeeds,
        Self::SunGlints,
        Self::Glass,
        Self::SurfaceReflection,
        Self::MetallicShine,
        Self::Exposure,
        Self::ColorCorrect,
        Self::Fade,
        Self::ChromaKey,
        Self::LumaKey,
        Self::Transform,
        Self::Displace,
        Self::MorphologyHorizontal,
        Self::MorphologyVertical,
        Self::Shadow,
        Self::BlurHorizontal,
        Self::BlurVertical,
        Self::LensPrefilterHorizontal,
        Self::LensPrefilterVertical,
        Self::LensBlur,
        Self::BevelHeightHorizontal,
        Self::BevelHeightVertical,
        Self::EnvironmentPyramid,
        Self::FrostHorizontal,
        Self::FrostVertical,
        Self::Merge,
        Self::Blend,
        Self::SurfaceBlend,
        Self::Image,
        Self::Affine,
        Self::PlanarWarp,
        Self::Ellipse,
        Self::Mask,
        Self::CombineMask,
        Self::ChannelMask,
        Self::Colorize,
        Self::Gradient,
        Self::MattePack,
        Self::KnownSubject,
        Self::KnownBackground,
        Self::MatteBoxHorizontal,
        Self::MatteBoxVertical,
        Self::MatteProject,
        Self::MatteJoin,
        Self::FuseForeground,
        Self::FuseBackground,
        Self::FuseBoxHorizontal,
        Self::FuseBoxVertical,
        Self::FuseEstimate,
        Self::FuseFinish,
        Self::Lut,
    ];
    pub(crate) fn entry(self) -> &'static str {
        match self {
            Self::ToAcescg => "to_acescg",
            Self::FromAcescg => "from_acescg",
            Self::Text => "text_image",
            Self::Solid => "solid",
            Self::Fire => "fire",
            Self::Dust => "dust",
            Self::DripRoots => "drip_roots",
            Self::Drip => "drip",
            Self::Atmosphere => "atmosphere",
            Self::Material => "material",
            Self::Condensation => "condensation",
            Self::Patina => "patina",
            Self::GlintSeeds => "glint_seeds",
            Self::SunGlints => "sun_glints",
            Self::Glass => "glass",
            Self::SurfaceReflection => "surface_reflection",
            Self::MetallicShine => "metallic_shine",
            Self::FractalNoise => "fractal_noise",
            Self::Noise => "noise",
            Self::Exposure => "exposure",
            Self::ColorCorrect => "color_correct",
            Self::Fade => "fade",
            Self::ChromaKey => "chroma_key",
            Self::LumaKey => "luma_key",
            Self::Displace => "displace",
            Self::Transform => "transform",
            Self::MorphologyHorizontal => "morphology_horizontal",
            Self::MorphologyVertical => "morphology_vertical",
            Self::Shadow => "shadow",
            Self::BlurHorizontal => "blur_horizontal",
            Self::BlurVertical => "blur_vertical",
            Self::LensPrefilterHorizontal => "lens_prefilter_horizontal",
            Self::LensPrefilterVertical => "lens_prefilter_vertical",
            Self::LensBlur => "lens_blur",
            Self::BevelHeightHorizontal => "bevel_height_horizontal",
            Self::BevelHeightVertical => "bevel_height_vertical",
            Self::EnvironmentPyramid => "environment_pyramid",
            Self::FrostHorizontal => "frost_horizontal",
            Self::FrostVertical => "frost_vertical",
            Self::Merge => "merge",
            Self::Blend => "blend",
            Self::SurfaceBlend => "surface_blend",
            Self::Image => "image",
            Self::Affine => "affine",
            Self::PlanarWarp => "planar_warp",
            Self::Ellipse => "ellipse",
            Self::ChannelMask => "channel_mask",
            Self::Colorize => "colorize",
            Self::Gradient => "gradient",
            Self::MattePack => "matte_pack",
            Self::KnownSubject => "known_subject",
            Self::KnownBackground => "known_background",
            Self::MatteBoxHorizontal => "matte_box_horizontal",
            Self::MatteBoxVertical => "matte_box_vertical",
            Self::MatteProject => "matte_project",
            Self::MatteJoin => "matte_join",
            Self::FuseForeground => "fuse_foreground",
            Self::FuseBackground => "fuse_background",
            Self::FuseBoxHorizontal => "fuse_box_horizontal",
            Self::FuseBoxVertical => "fuse_box_vertical",
            Self::FuseEstimate => "fuse_estimate",
            Self::FuseFinish => "fuse_finish",
            Self::Lut => "lut",
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
    pub inputs: [Input; 3],
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
pub(crate) struct TemporalBranch {
    pub operation: Operation,
    pub plan: Box<Plan>,
    pub key: String,
}
pub struct Plan {
    pub(crate) ocio: veylvik_document::color_management::OcioViews,
    pub(crate) temporal: Vec<TemporalBranch>,
    pub(crate) frame_rate: veylvik_document::Time,
    working_space: veylvik_document::color_management::WorkingSpace,
    root: NodeId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) nodes: Vec<Node>,
    /// Inputs of light rigs (planar tracks) read as data; never rendered.
    pub(crate) light_data: Vec<Node>,
    pub(crate) curves: Vec<BTreeMap<&'static str, CompiledScalar>>,
    pub(crate) passes: Vec<Pass>,
    pub(crate) output_slot: usize,
    pub(crate) slot_count: usize,
}
impl Plan {
    pub fn output_node(&self) -> NodeId {
        self.root
    }
    pub fn compile(composition: &Composition) -> Result<Self> {
        Self::compile_output(composition, composition.output)
    }
    pub fn compile_output(composition: &Composition, output: NodeId) -> Result<Self> {
        composition.validate()?;
        Self::compile_branch(composition, output, 0, &mut 16384)
    }
    fn compile_branch(
        composition: &Composition,
        output: NodeId,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self> {
        if depth > 8 {
            return Err(error("Temporal nesting exceeds 8 branches"));
        }
        if !composition.nodes.contains_key(&output) {
            return Err(error("requested output node is missing"));
        }
        if composition.nodes[&output].operation.descriptor().output
            != veylvik_document::registry::PortKind::Image
        {
            return Err(error(
                "A Light rig has no image of its own; view a surface it lights",
            ));
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
                } else if let [port] = node
                    .operation
                    .descriptor()
                    .required()
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    // Optional inputs such as a reflection source do not pass through.
                    node.inputs.get(*port).copied()
                } else {
                    None
                };
                let source = source.ok_or_else(|| {
                    error("Bypass needs one source image or a single-input effect group")
                })?;
                let target = next.nodes.get_mut(&node.id).unwrap();
                target.operation = Operation::Transform(veylvik_document::nodes::Transform {
                    x: 0.0.into(),
                    y: 0.0.into(),
                });
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
        let mut light_data: Vec<Node> = Vec::new();
        let mut temporal = Vec::new();
        while let Some((id, expanded)) = todo.pop() {
            let node = &composition.nodes[&id];
            if expanded {
                *remaining = remaining
                    .checked_sub(1)
                    .ok_or_else(|| error("Temporal graph expansion exceeds 16384 nodes"))?;
                if matches!(
                    node.operation,
                    Operation::MotionBlur(_)
                        | Operation::TimeRemap(_)
                        | Operation::Ocio(_)
                        | Operation::OpticalFlow(_)
                ) {
                    let key = format!("\0temporal:{}", node.id);
                    temporal.push(TemporalBranch {
                        operation: node.operation.clone(),
                        plan: Box::new(Self::compile_branch(
                            composition,
                            node.inputs["image"],
                            depth + 1,
                            remaining,
                        )?),
                        key: key.clone(),
                    });
                    let mut replacement = node.clone();
                    replacement.operation =
                        Operation::Image(veylvik_document::nodes::Image { asset: key });
                    replacement.inputs.clear();
                    nodes.push(replacement);
                } else {
                    nodes.push(node.clone());
                }
            } else if visited.insert(id) {
                todo.push((id, true));
                if matches!(
                    node.operation,
                    Operation::MotionBlur(_)
                        | Operation::TimeRemap(_)
                        | Operation::Ocio(_)
                        | Operation::OpticalFlow(_)
                ) {
                    continue;
                }
                // A rig's inputs only carry data for its lights, so their sources
                // (such as footage under a planar track) are never decoded for it.
                if node.operation.descriptor().output
                    == veylvik_document::registry::PortKind::Lights
                {
                    for source in node.inputs.values() {
                        if !light_data.iter().any(|n| n.id == *source) {
                            light_data.push(composition.nodes[source].clone());
                        }
                    }
                    continue;
                }
                for port in node.operation.inputs().iter().rev() {
                    if let Some(source) = node.inputs.get(*port) {
                        todo.push((*source, false));
                    }
                }
            }
        }
        let mut passes = Vec::new();
        let mut outputs = BTreeMap::new();
        let mut domains = BTreeMap::new();
        let mut shared = BTreeMap::new();
        for (index, node) in nodes.iter().enumerate() {
            let aces = composition.working_space
                == veylvik_document::color_management::WorkingSpace::Acescg
                && matches!(
                    node.operation,
                    Operation::Merge(_)
                        | Operation::Blend(_)
                        | Operation::Exposure(_)
                        | Operation::ColorCorrect(_)
                        | Operation::Blur(_)
                        | Operation::LensBlur(_)
                        | Operation::Transform(_)
                        | Operation::Affine(_)
                );
            domains.insert(node.id, aces);
            // A light rig is read by the surfaces it lights; it has no pass or image.
            if node.operation.descriptor().output == veylvik_document::registry::PortKind::Lights {
                continue;
            }
            let mut inputs = [Input::Empty, Input::Empty, Input::Empty];
            for (i, port) in node.operation.inputs().iter().enumerate() {
                if node.operation.descriptor().port_kind(port)
                    == veylvik_document::registry::PortKind::Lights
                {
                    continue;
                }
                // An unconnected optional port stays an empty input.
                let Some(&source) = node.inputs.get(*port) else {
                    continue;
                };
                let mut value = outputs[&source];
                if domains[&source] != aces {
                    let converted = passes.len();
                    passes.push(Pass {
                        node: index,
                        kernel: if aces {
                            Kernel::ToAcescg
                        } else {
                            Kernel::FromAcescg
                        },
                        inputs: [Input::Value(value), Input::Empty, Input::Empty],
                        output: converted,
                    });
                    value = converted;
                }
                inputs[i] = Input::Value(value);
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
                let mut bytes = serde_json::to_vec(&node.operation).map_err(error)?;
                // Surfaces lit by different rigs are different results.
                if let Some(rig) = node.inputs.get("lights") {
                    let rig = nodes.iter().find(|n| n.id == *rig).map(|n| &n.operation);
                    bytes.extend(serde_json::to_vec(&rig).map_err(error)?);
                }
                Some((
                    bytes,
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
            let kernel = match veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::passes_for(n, node))
            {
                crate::nodes::Passes::One(kernel) => {
                    // A node that reads a file (such as a LUT) gets it on its next free input.
                    if let Some(asset) = node.operation.asset()
                        && let Some(slot) = inputs.iter_mut().find(|i| matches!(i, Input::Empty))
                    {
                        *slot = Input::Asset(asset.to_owned());
                    }
                    kernel
                }
                crate::nodes::Passes::Separable(first, second) => {
                    let intermediate = passes.len();
                    passes.push(Pass {
                        node: index,
                        kernel: first,
                        inputs,
                        output: intermediate,
                    });
                    inputs = [Input::Value(intermediate), Input::Empty, Input::Empty];
                    second
                }
                crate::nodes::Passes::Chain(steps) => {
                    let ports = inputs.clone();
                    let mut values: Vec<usize> = Vec::with_capacity(steps.len());
                    let resolve = |from: crate::nodes::From, values: &[usize]| match from {
                        crate::nodes::From::Port(i) => ports[i].clone(),
                        crate::nodes::From::Step(j) => Input::Value(values[j]),
                        crate::nodes::From::None => Input::Empty,
                    };
                    let (last, rest) = steps.split_last().expect("a chain has steps");
                    for step in rest {
                        let value = passes.len();
                        passes.push(Pass {
                            node: index,
                            kernel: step.kernel,
                            inputs: step.inputs.map(|from| resolve(from, &values)),
                            output: value,
                        });
                        values.push(value);
                    }
                    inputs = last.inputs.map(|from| resolve(from, &values));
                    last.kernel
                }
                crate::nodes::Passes::Special => match &node.operation {
                    Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack { .. }) => {
                        unreachable!("track aliases its source")
                    }
                    Operation::Path(veylvik_document::nodes::Path { .. })
                    | Operation::Text(veylvik_document::nodes::Text { .. }) => {
                        if composition.width > crate::MAX_VECTOR_DIMENSION
                            || composition.height > crate::MAX_VECTOR_DIMENSION
                        {
                            return Err(error("vector mask dimensions exceed 4096"));
                        }
                        inputs[0] = Input::Vector(index);
                        if matches!(
                            node.operation,
                            Operation::Text(veylvik_document::nodes::Text { .. })
                        ) {
                            Kernel::Text
                        } else {
                            Kernel::Image
                        }
                    }
                    Operation::Footage(veylvik_document::nodes::Footage { .. }) => {
                        inputs[0] = Input::Asset(footage_key(node.id));
                        Kernel::Image
                    }
                    Operation::Image(veylvik_document::nodes::Image { asset }) => {
                        inputs[0] = Input::Asset(asset.clone());
                        Kernel::Image
                    }
                    _ => unreachable!("only structural nodes are special"),
                },
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
        let mut final_value = outputs[&output];
        // Public frame I/O stays scene-linear Rec.709; internal compositing/grades
        // may use AP1. Procedural materials and keyed colours retain their authored
        // Rec.709 processing domain and receive explicit boundary conversions.
        if domains[&output] {
            let converted = passes.len();
            passes.push(Pass {
                node: nodes.len() - 1,
                kernel: Kernel::FromAcescg,
                inputs: [Input::Value(final_value), Input::Empty, Input::Empty],
                output: converted,
            });
            final_value = converted;
        }
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
            ocio: composition.ocio.clone(),
            temporal,
            frame_rate: composition.frame_rate,
            working_space: composition.working_space,
            root: output,
            curves,
            width: composition.width,
            height: composition.height,
            nodes,
            light_data,
            passes,
            output_slot: slots[final_value],
            slot_count,
        })
    }
    /// Whether a validated composition has the same reachable render dependencies.
    /// Labels, positions, frame rate and disconnected nodes do not affect a plan.
    /// The entire candidate is validated, including disconnected nodes.
    /// Nodes that can contribute to the output at `time`, following every input except those
    /// a node ignores then (see `RenderNode::idle_inputs`).
    pub(crate) fn visible_at(&self, time: veylvik_document::Time) -> Result<BTreeSet<NodeId>> {
        let index: BTreeMap<NodeId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let mut visible = BTreeSet::new();
        let mut pending = vec![self.root];
        while let Some(id) = pending.pop() {
            let Some(&i) = index.get(&id) else { continue };
            if !visible.insert(id) {
                continue;
            }
            let node = &self.nodes[i];
            let curves = &self.curves[i];
            let sample = |name: &str| -> Result<f32> {
                curves
                    .get(name)
                    .ok_or_else(|| error("compiled scalar parameter missing"))?
                    .at(time)
                    .map_err(Into::into)
            };
            let idle = veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::idle_inputs(n, &sample, time))?;
            pending.extend(
                node.inputs
                    .iter()
                    .filter(|(port, _)| !idle.contains(&port.as_str()))
                    .map(|(_, id)| *id),
            );
        }
        Ok(visible)
    }
    pub fn matches_composition(&self, composition: &Composition, output: NodeId) -> Result<bool> {
        composition.validate()?;
        if serde_json::to_vec(&self.ocio).map_err(error)?
            != serde_json::to_vec(&composition.ocio).map_err(error)?
            || self.working_space != composition.working_space
            || !self.temporal.is_empty()
            || self.root != output
            || self.width != composition.width
            || self.height != composition.height
        {
            return Ok(false);
        }
        for before in self.nodes.iter().chain(&self.light_data) {
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
            .filter_map(|n| n.operation.asset())
            .filter(|key| !key.starts_with('\0'))
            .chain(self.temporal.iter().flat_map(|b| b.plan.required_assets()))
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
            .filter(|n| {
                matches!(
                    n.operation,
                    Operation::Path(veylvik_document::nodes::Path { .. })
                        | Operation::Text(veylvik_document::nodes::Text { .. })
                )
            })
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
    veylvik_document::each_node!(operation, n => crate::nodes::RenderNode::identity(n))
}

fn shareable(operation: &Operation) -> bool {
    veylvik_document::each_node!(operation, n => crate::nodes::RenderNode::shareable(n))
}

pub(crate) fn footage_key(id: veylvik_document::NodeId) -> String {
    format!("\0footage:{id}")
}
