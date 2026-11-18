//! Compile authored nodes to explicit passes, then assign image lifetimes.
use crate::{Result, error};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{CompiledScalar, Composition, Node, NodeId, Operation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kernel {
    /// A Render scene node, drawn by the registered scene renderer, not a compute kernel.
    RenderScene,
    /// A custom shader node: its pipeline is compiled from the node's code.
    Custom,
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
    MaterialLayers,
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
    ContactShadow,
    BlurHorizontal,
    BlurVertical,
    GrainNoise,
    GrainApply,
    LensDistortion,
    LensDepthPrefilterHorizontal,
    LensDepthPrefilterVertical,
    LensDepthBlur,
    BloomBright,
    BloomWideHorizontal,
    BloomWideVertical,
    BloomTightHorizontal,
    BloomTightVertical,
    BloomStreakVertical,
    BloomCombine,
    GridWarp,
    ParticlePrepare,
    ParticleFields,
    ParticleSplat,
    ParticleResolve,
    LensPrefilterHorizontal,
    LensPrefilterVertical,
    LensBlur,
    BevelHeightHorizontal,
    BevelHeightVertical,
    EnvironmentPyramid,
    FrostHorizontal,
    FrostVertical,
    TexturePack,
    SurfaceMaps,
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
    pub(crate) const ALL: [Self; 87] = [
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
        Self::MaterialLayers,
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
        Self::ContactShadow,
        Self::BlurHorizontal,
        Self::BlurVertical,
        Self::GrainNoise,
        Self::GrainApply,
        Self::LensDistortion,
        Self::LensDepthPrefilterHorizontal,
        Self::LensDepthPrefilterVertical,
        Self::LensDepthBlur,
        Self::BloomBright,
        Self::BloomWideHorizontal,
        Self::BloomWideVertical,
        Self::BloomTightHorizontal,
        Self::BloomTightVertical,
        Self::BloomStreakVertical,
        Self::BloomCombine,
        Self::GridWarp,
        Self::ParticlePrepare,
        Self::ParticleFields,
        Self::ParticleSplat,
        Self::ParticleResolve,
        Self::LensPrefilterHorizontal,
        Self::LensPrefilterVertical,
        Self::LensBlur,
        Self::BevelHeightHorizontal,
        Self::BevelHeightVertical,
        Self::EnvironmentPyramid,
        Self::FrostHorizontal,
        Self::FrostVertical,
        Self::TexturePack,
        Self::SurfaceMaps,
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
            Self::RenderScene => "render_scene",
            Self::Custom => "custom",
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
            Self::MaterialLayers => "material_layers",
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
            Self::ContactShadow => "contact_shadow",
            Self::BlurHorizontal => "blur_horizontal",
            Self::BlurVertical => "blur_vertical",
            Self::GrainNoise => "grain_noise",
            Self::GrainApply => "grain_apply",
            Self::LensDistortion => "lens_distortion",
            Self::LensDepthPrefilterHorizontal => "lens_depth_prefilter_horizontal",
            Self::LensDepthPrefilterVertical => "lens_depth_prefilter_vertical",
            Self::LensDepthBlur => "lens_depth_blur",
            Self::BloomBright => "bloom_bright",
            Self::BloomWideHorizontal => "bloom_wide_horizontal",
            Self::BloomWideVertical => "bloom_wide_vertical",
            Self::BloomTightHorizontal => "bloom_tight_horizontal",
            Self::BloomTightVertical => "bloom_tight_vertical",
            Self::BloomStreakVertical => "bloom_streak_vertical",
            Self::BloomCombine => "bloom_combine",
            Self::GridWarp => "grid_warp",
            Self::ParticlePrepare => "particle_prepare",
            Self::ParticleFields => "particle_fields",
            Self::ParticleSplat => "particle_splat",
            Self::ParticleResolve => "particle_resolve",
            Self::LensPrefilterHorizontal => "lens_prefilter_horizontal",
            Self::LensPrefilterVertical => "lens_prefilter_vertical",
            Self::LensBlur => "lens_blur",
            Self::BevelHeightHorizontal => "bevel_height_horizontal",
            Self::BevelHeightVertical => "bevel_height_vertical",
            Self::EnvironmentPyramid => "environment_pyramid",
            Self::FrostHorizontal => "frost_horizontal",
            Self::FrostVertical => "frost_vertical",
            Self::TexturePack => "texture_pack",
            Self::SurfaceMaps => "surface_maps",
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
    pub inputs: [Input; BOUND],
    pub output: usize,
    /// Image card sources a Render scene reads: (source node, its image).
    pub cards: Vec<(NodeId, Input)>,
}
/// Images a pass binds (`source`, `second`, `third`, `fourth`).
pub(crate) const BOUND: usize = 4;
/// Ports a node may have; chain steps pick any of them, a single pass the first four.
const MAX_PORTS: usize = 8;
fn bound(ports: &[Input; MAX_PORTS]) -> [Input; BOUND] {
    std::array::from_fn(|i| ports[i].clone())
}
fn widen(bound: [Input; BOUND]) -> [Input; MAX_PORTS] {
    let mut ports: [Input; MAX_PORTS] = std::array::from_fn(|_| Input::Empty);
    for (port, input) in ports.iter_mut().zip(bound) {
        *port = input;
    }
    ports
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
    /// Nodes that expressions in this plan read; a change to them changes the plan.
    link_data: Vec<Node>,
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
        // Documents arrive resolved; a composition built by hand gets its links here.
        if composition.needs_resolving() {
            let mut resolved = composition.clone();
            resolved.resolve_expressions().map_err(error)?;
            return Self::compile_branch(&resolved, output, 0, &mut 16384);
        }
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
        match composition.nodes[&output].operation.descriptor().output {
            veylvik_document::registry::PortKind::Image => {}
            veylvik_document::registry::PortKind::Lights => {
                return Err(error(
                    "A Light rig has no image of its own; view a surface it lights",
                ));
            }
            veylvik_document::registry::PortKind::Scene => {
                return Err(error(
                    "A 3D scene has no image of its own; view the Render scene node that films it",
                ));
            }
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
                    // A node that wants an input's recent past gets it as a branch
                    // rendered at other times; the node itself stays.
                    if let Some(source) = history_source(node) {
                        temporal.push(TemporalBranch {
                            operation: node.operation.clone(),
                            plan: Box::new(Self::compile_branch(
                                composition,
                                source,
                                depth + 1,
                                remaining,
                            )?),
                            key: history_key(node.id),
                        });
                    }
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
                    // An input read through its history is planned as a branch instead.
                    if *port == HISTORY_PORT && history_source(node).is_some() {
                        continue;
                    }
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
            // Light rigs and 3D scene nodes are data read by the nodes they feed;
            // they have no pass or image of their own.
            if node.operation.descriptor().output != veylvik_document::registry::PortKind::Image {
                continue;
            }
            if node.operation.inputs().len() > MAX_PORTS {
                return Err(error("a node has more input ports than a pass can address"));
            }
            let mut inputs: [Input; MAX_PORTS] = std::array::from_fn(|_| Input::Empty);
            for (i, port) in node.operation.inputs().iter().enumerate() {
                // Light rigs and scenes are read as data, not bound as images.
                if node.operation.descriptor().port_kind(port)
                    != veylvik_document::registry::PortKind::Image
                {
                    continue;
                }
                if *port == HISTORY_PORT && history_source(node).is_some() {
                    inputs[i] = Input::Asset(history_key(node.id));
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
                        inputs: [
                            Input::Value(value),
                            Input::Empty,
                            Input::Empty,
                            Input::Empty,
                        ],
                        output: converted,
                        cards: Vec::new(),
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
            let mut cards = Vec::new();
            let kernel = match veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::passes_for(n, node))
            {
                crate::nodes::Passes::One(kernel) => {
                    // A node that reads a file (such as a LUT) gets it on its next free input.
                    if let Some(asset) = node.operation.asset()
                        && let Some(slot) = inputs[..BOUND]
                            .iter_mut()
                            .find(|i| matches!(i, Input::Empty))
                    {
                        *slot = Input::Asset(asset.to_owned());
                    }
                    // A node with data of its own (such as a warp's mesh) reads it there too.
                    if veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::has_data(n))
                        && let Some(slot) = inputs[..BOUND]
                            .iter_mut()
                            .find(|i| matches!(i, Input::Empty))
                    {
                        *slot = Input::Asset(data_key(node.id));
                    }
                    kernel
                }
                crate::nodes::Passes::Separable(first, second) => {
                    let intermediate = passes.len();
                    passes.push(Pass {
                        node: index,
                        kernel: first,
                        inputs: bound(&inputs),
                        output: intermediate,
                        cards: Vec::new(),
                    });
                    inputs = widen([
                        Input::Value(intermediate),
                        Input::Empty,
                        Input::Empty,
                        Input::Empty,
                    ]);
                    second
                }
                crate::nodes::Passes::Chain(steps) => {
                    let ports = inputs.clone();
                    let mut values: Vec<usize> = Vec::with_capacity(steps.len());
                    let resolve = |from: crate::nodes::From, values: &[usize]| match from {
                        crate::nodes::From::Port(i) => ports[i].clone(),
                        crate::nodes::From::Step(j) => Input::Value(values[j]),
                        crate::nodes::From::None => Input::Empty,
                        crate::nodes::From::Data => Input::Asset(data_key(node.id)),
                    };
                    let (last, rest) = steps.split_last().expect("a chain has steps");
                    for step in rest {
                        let value = passes.len();
                        passes.push(Pass {
                            node: index,
                            kernel: step.kernel,
                            inputs: step.inputs.map(|from| resolve(from, &values)),
                            output: value,
                            cards: Vec::new(),
                        });
                        values.push(value);
                    }
                    inputs = widen(last.inputs.map(|from| resolve(from, &values)));
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
                    Operation::RenderScene(_) => {
                        // Every image card's and environment's picture, in Rec.709, alive
                        // until the scene draws.
                        let mut pending: Vec<NodeId> = node.inputs.values().copied().collect();
                        let mut seen = BTreeSet::new();
                        while let Some(id) = pending.pop() {
                            if !seen.insert(id) {
                                continue;
                            }
                            let Some(n) = composition.nodes.get(&id) else {
                                continue;
                            };
                            if let (
                                Operation::ImageCard(_) | Operation::Environment3d(_),
                                Some(&source),
                            ) = (&n.operation, n.inputs.get("image"))
                                && !cards.iter().any(|(s, _)| *s == source)
                                && let Some(&value) = outputs.get(&source)
                            {
                                let mut value = value;
                                if domains.get(&source).copied().unwrap_or(false) {
                                    let converted = passes.len();
                                    passes.push(Pass {
                                        node: index,
                                        kernel: Kernel::FromAcescg,
                                        inputs: [
                                            Input::Value(value),
                                            Input::Empty,
                                            Input::Empty,
                                            Input::Empty,
                                        ],
                                        output: converted,
                                        cards: Vec::new(),
                                    });
                                    value = converted;
                                }
                                cards.push((source, Input::Value(value)));
                            }
                            let descriptor = n.operation.descriptor();
                            pending.extend(n.inputs.iter().filter_map(|(port, source)| {
                                (descriptor.port_kind(port)
                                    == veylvik_document::registry::PortKind::Scene)
                                    .then_some(*source)
                            }));
                        }
                        Kernel::RenderScene
                    }
                    _ => unreachable!("only structural nodes are special"),
                },
            };
            let value = passes.len();
            passes.push(Pass {
                node: index,
                kernel,
                inputs: bound(&inputs),
                output: value,
                cards,
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
                inputs: [
                    Input::Value(final_value),
                    Input::Empty,
                    Input::Empty,
                    Input::Empty,
                ],
                output: converted,
                cards: Vec::new(),
            });
            final_value = converted;
        }
        let mut uses = vec![0usize; passes.len()];
        for pass in &passes {
            for input in pass.inputs.iter().chain(pass.cards.iter().map(|(_, i)| i)) {
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
            for input in pass
                .inputs
                .iter_mut()
                .chain(pass.cards.iter_mut().map(|(_, i)| i))
            {
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
        // A custom shader that does not compile is an error here, with its line.
        for node in &nodes {
            if let Operation::Shader(shader) = &node.operation {
                crate::custom::check(&shader.code)
                    .map_err(|e| error(format!("{}: {e}", node.label)))?;
            }
        }
        let link_data = composition
            .linked_nodes(nodes.iter().chain(&light_data).map(|n| n.id))
            .into_iter()
            .map(|id| composition.nodes[&id].clone())
            .collect();
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
            link_data,
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
        for before in self
            .nodes
            .iter()
            .chain(&self.light_data)
            .chain(&self.link_data)
        {
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
    /// Media the plan reads, by source key (`Operation::source_key`): a file path, or
    /// for one layer of a multi-layer OpenEXR, the path, a NUL and the layer.
    pub fn required_assets(&self) -> BTreeSet<std::borrow::Cow<'_, str>> {
        self.nodes
            .iter()
            .filter_map(|n| n.operation.source_key())
            .filter(|key| !key.starts_with('\0'))
            .chain(self.temporal.iter().flat_map(|b| b.plan.required_assets()))
            .collect()
    }
    /// The model files the plan's 3D models read.
    pub fn required_models(&self) -> BTreeSet<&str> {
        self.nodes
            .iter()
            .filter_map(|n| match &n.operation {
                Operation::Model3d(m) if !m.model.is_empty() => Some(m.model.as_str()),
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
    /// The composition's size in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }
    /// Whether the plan has temporal branches (motion blur, retiming, flow, OCIO).
    pub fn has_temporal(&self) -> bool {
        !self.temporal.is_empty()
    }
    /// Every OCIO transform the plan uses: the viewer's, and those of OCIO nodes in it
    /// and its temporal branches.
    pub fn ocio_transforms(&self) -> Vec<&veylvik_document::color_management::OcioTransform> {
        let mut transforms: Vec<_> = self.ocio.view.iter().collect();
        for node in &self.nodes {
            if let Operation::Ocio(v) = &node.operation {
                transforms.push(&v.transform);
            }
        }
        for branch in &self.temporal {
            if let Operation::Ocio(v) = &branch.operation {
                transforms.push(&v.transform);
            }
            transforms.extend(branch.plan.ocio_transforms());
        }
        transforms
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

/// The port a node may read through its recent past (see `crate::history`).
pub(crate) const HISTORY_PORT: &str = "emitter";
/// The source of the input a node reads through its history, when it asks for one.
pub(crate) fn history_source(node: &Node) -> Option<NodeId> {
    match &node.operation {
        Operation::Particles(particles) if particles.history > 0 => {
            node.inputs.get(HISTORY_PORT).copied()
        }
        _ => None,
    }
}
pub(crate) fn history_key(id: veylvik_document::NodeId) -> String {
    format!("\0temporal:{id}")
}
/// The input key of a node's own data image (`RenderNode::data`).
pub(crate) fn data_key(id: veylvik_document::NodeId) -> String {
    format!("\0data:{id}")
}
pub(crate) fn footage_key(id: veylvik_document::NodeId) -> String {
    format!("\0footage:{id}")
}
