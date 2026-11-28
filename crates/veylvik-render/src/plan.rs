//! Compile authored nodes to explicit passes, then assign image lifetimes.
use crate::{Result, error};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{CompiledScalar, Composition, Node, NodeId, Operation};

/// Every kernel on one line: the variant, its entry point in the WGSL, how its threads
/// are laid out when it runs, and whether it is a compute program of its own (`program`)
/// or work done another way (`none`: the 3D renderer, the effect host, a node's own code).
macro_rules! kernels {
    ($($(#[$doc:meta])* $variant:ident => $entry:literal, $dispatch:ident, $kind:ident;)+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Kernel {
            $($(#[$doc])* $variant,)+
        }
        impl Kernel {
            /// Every kernel, in table order.
            const EVERY: &'static [Self] = &[$(Self::$variant),+];
            pub(crate) fn entry(self) -> &'static str {
                match self { $(Self::$variant => $entry),+ }
            }
            /// How the kernel's threads are laid out when it runs: the one answer both
            /// the pass loop's dispatch and area dispatch read.
            pub(crate) fn dispatch(self) -> Dispatch {
                match self { $(Self::$variant => Dispatch::$dispatch),+ }
            }
            /// Whether it is a compute program of its own, compiled from the WGSL.
            pub(crate) fn is_program(self) -> bool {
                match self { $(Self::$variant => kernels!(@program $kind)),+ }
            }
        }
    };
    (@program program) => { true };
    (@program none) => { false };
}
kernels! {
    /// A Render scene node, drawn by the registered scene renderer, not a compute kernel.
    RenderScene => "render_scene", Special, none;
    /// Depth maps of a scene for particles it lights: made by the 3D renderer, as Render
    /// scene is (no program of its own).
    ParticleShadows => "particle_shadows", Special, none;
    /// Where a flock's particles are at the frame, simulated by the renderer over many
    /// steps of the two kernels below (no program of its own).
    ParticleSimulation => "particle_simulation", Special, none;
    /// A node computed outside the renderer by the registered effect host (an OpenFX
    /// plugin), not a compute kernel.
    External => "external", Special, none;
    /// A custom shader node: its pipeline is compiled from the node's code.
    Custom => "custom", Frame, none;
    ToAcescg => "to_acescg", Area, program;
    FromAcescg => "from_acescg", Area, program;
    Text => "text_image", Area, program;
    Solid => "solid", Area, program;
    Noise => "noise", Area, program;
    FractalNoise => "fractal_noise", Area, program;
    Fire => "fire", Area, program;
    Dust => "dust", Area, program;
    DripRoots => "drip_roots", Area, program;
    Drip => "drip", Area, program;
    Atmosphere => "atmosphere", Area, program;
    Material => "material", Area, program;
    Condensation => "condensation", Area, program;
    Patina => "patina", Area, program;
    MaterialLayers => "material_layers", Area, program;
    GlintSeeds => "glint_seeds", GlintCells, program;
    SunGlints => "sun_glints", Area, program;
    Glass => "glass", Area, program;
    SurfaceReflection => "surface_reflection", Area, program;
    MetallicShine => "metallic_shine", Area, program;
    Exposure => "exposure", Area, program;
    ColorCorrect => "color_correct", Area, program;
    Fade => "fade", Area, program;
    ChromaKey => "chroma_key", Area, program;
    LumaKey => "luma_key", Area, program;
    Transform => "transform", Area, program;
    Displace => "displace", Area, program;
    MorphologyHorizontal => "morphology_horizontal", Area, program;
    MorphologyVertical => "morphology_vertical", Area, program;
    Shadow => "shadow", Area, program;
    ContactShadow => "contact_shadow", Area, program;
    BlurHorizontal => "blur_horizontal", Rows, program;
    BlurVertical => "blur_vertical", Area, program;
    GrainNoiseHorizontal => "grain_noise_horizontal", Rows, program;
    GrainApply => "grain_apply", Area, program;
    LensDistortion => "lens_distortion", Area, program;
    LensDepthPrefilterHorizontal => "lens_depth_prefilter_horizontal", Area, program;
    LensDepthPrefilterVertical => "lens_depth_prefilter_vertical", Area, program;
    LensDepthBlur => "lens_depth_blur", Area, program;
    BloomBright => "bloom_bright", Area, program;
    BloomTightHorizontal => "bloom_tight_horizontal", Area, program;
    BloomTightVertical => "bloom_tight_vertical", Area, program;
    BloomWideDown => "bloom_wide_down", Area, program;
    BloomWideCellsHorizontal => "bloom_wide_cells_horizontal", Area, program;
    BloomWideCellsVertical => "bloom_wide_cells_vertical", Area, program;
    BloomStreakDown => "bloom_streak_down", Area, program;
    BloomStreakCellsHorizontal => "bloom_streak_cells_horizontal", Area, program;
    BloomStreakCellsVertical => "bloom_streak_cells_vertical", Area, program;
    BloomCombine => "bloom_combine", Area, program;
    LightWrapCombine => "light_wrap_combine", Area, program;
    ChromaMatte => "chroma_matte", Area, program;
    KeyShrinkHorizontal => "key_shrink_horizontal", Area, program;
    KeyShrinkVertical => "key_shrink_vertical", Area, program;
    KeySoftenHorizontal => "key_soften_horizontal", Area, program;
    KeySoftenVertical => "key_soften_vertical", Area, program;
    ChromaApply => "chroma_apply", Area, program;
    Cryptomatte => "cryptomatte", Area, program;
    ZoomBlur => "zoom_blur", Area, program;
    OwnSizeFit => "own_size_fit", Area, program;
    OwnSizeStretch => "own_size_stretch", Area, program;
    GridWarp => "grid_warp", Area, program;
    ParticlePrepare => "particle_prepare", Frame, program;
    ParticleFields => "particle_fields", Frame, program;
    ParticleSplat => "particle_splat", Particles, program;
    ParticleOcclusion => "particle_occlusion", Particles, program;
    ParticleCarry => "particle_carry", Frame, program;
    ParticleGridOut => "particle_grid_out", Frame, program;
    ParticleResolve => "particle_resolve", Frame, program;
    ParticlePositions => "particle_positions", Linked, program;
    ParticleLinks => "particle_links", Linked, program;
    ParticleFlockGrid => "particle_flock_grid", Frame, program;
    ParticleFlockScan => "particle_flock_scan", Frame, program;
    ParticleFlockList => "particle_flock_list", Frame, program;
    ParticleStep => "particle_step", Frame, program;
    ParticleRelax => "particle_relax", Frame, program;
    ParticleInstances => "particle_instances", Frame, program;
    LensPrefilterHorizontal => "lens_prefilter_horizontal", Area, program;
    LensPrefilterVertical => "lens_prefilter_vertical", Area, program;
    LensBlur => "lens_blur", Area, program;
    BevelHeightHorizontal => "bevel_height_horizontal", Area, program;
    BevelHeightVertical => "bevel_height_vertical", Area, program;
    EnvironmentPyramid => "environment_pyramid", Area, program;
    FrostHorizontal => "frost_horizontal", Rows, program;
    FrostVertical => "frost_vertical", Area, program;
    TexturePack => "texture_pack", Area, program;
    SurfaceMaps => "surface_maps", Area, program;
    Merge => "merge", Area, program;
    Blend => "blend", Area, program;
    SurfaceBlend => "surface_blend", Area, program;
    Image => "image", Area, program;
    Affine => "affine", Area, program;
    PlanarWarp => "planar_warp", Area, program;
    Ellipse => "ellipse", Area, program;
    Rectangle => "rectangle", Area, program;
    Repeat => "repeat", Area, program;
    Mask => "mask", Area, program;
    CombineMask => "combine_mask", Area, program;
    ChannelMask => "channel_mask", Area, program;
    Colorize => "colorize", Area, program;
    Gradient => "gradient", Area, program;
    MattePack => "matte_pack", Area, program;
    KnownSubject => "known_subject", Area, program;
    KnownBackground => "known_background", Area, program;
    MatteBoxHorizontal => "matte_box_horizontal", Area, program;
    MatteBoxVertical => "matte_box_vertical", Area, program;
    MatteProject => "matte_project", Area, program;
    MatteJoin => "matte_join", Area, program;
    FuseForeground => "fuse_foreground", Area, program;
    FuseBackground => "fuse_background", Area, program;
    FuseBoxHorizontal => "fuse_box_horizontal", Area, program;
    FuseBoxVertical => "fuse_box_vertical", Area, program;
    FuseEstimate => "fuse_estimate", Area, program;
    FuseFinish => "fuse_finish", Area, program;
    Lut => "lut", Area, program;
}
#[cfg(test)]
mod kernel_table_tests {
    use super::*;
    /// Every WGSL file a kernel program can come from.
    const SOURCES: &[&str] = &[
        include_str!("kernels.wgsl"),
        include_str!("surface.wgsl"),
        include_str!("environment.wgsl"),
        include_str!("working_space.wgsl"),
        include_str!("matte.wgsl"),
        include_str!("keyer.wgsl"),
        include_str!("grain.wgsl"),
        include_str!("lens.wgsl"),
        include_str!("lens_depth.wgsl"),
        include_str!("bloom.wgsl"),
        include_str!("warp.wgsl"),
        include_str!("lut.wgsl"),
        include_str!("shadow.wgsl"),
        include_str!("particles.wgsl"),
        include_str!("material_layers.wgsl"),
        include_str!("particle_system.wgsl"),
        include_str!("particle_solids.wgsl"),
        include_str!("particle_flock.wgsl"),
        include_str!("particle_instances.wgsl"),
    ];
    #[test]
    fn every_kernel_program_names_an_entry_point_the_wgsl_has() {
        let missing: Vec<&str> = Kernel::programs()
            .map(Kernel::entry)
            .filter(|entry| {
                let declared = format!("fn {entry}(");
                !SOURCES.iter().any(|source| source.contains(&declared))
            })
            .collect();
        assert!(missing.is_empty(), "no such entry point: {missing:?}");
        // Entry points are distinct, so no two kernels run the same program by mistake.
        let mut entries: Vec<&str> = Kernel::EVERY.iter().map(|k| k.entry()).collect();
        entries.sort_unstable();
        let count = entries.len();
        entries.dedup();
        assert_eq!(entries.len(), count);
    }
}
impl Kernel {
    /// The kernels with a compute program of their own, compiled ahead when asked.
    pub(crate) fn programs() -> impl Iterator<Item = Self> {
        Self::EVERY.iter().copied().filter(|k| k.is_program())
    }
}
#[derive(Clone, PartialEq)]
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
    pub kind: Kind,
    pub plan: Box<Plan>,
    pub key: String,
}
/// How a kernel's threads are laid out (see `Kernel::dispatch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dispatch {
    /// A thread a pixel, eight by eight; may run over its node's box only.
    Area,
    /// A thread a pixel over the whole frame, always.
    Frame,
    /// One 64-pixel row segment a workgroup.
    Rows,
    /// A thread a particle, in rows of `values.y` threads.
    Particles,
    /// A thread a particle that lines may join.
    Linked,
    /// A thread a glint cell of twice `values.x` pixels.
    GlintCells,
    /// No compute dispatch: the pass is made by `crate::special`.
    Special,
}
/// What a temporal branch is, decided once when planning: what reads the branch, with
/// the settings `crate::moments` needs to work out its moments.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    MotionBlur(veylvik_document::nodes::MotionBlur),
    TimeRemap(veylvik_document::nodes::TimeRemap),
    OpticalFlow(veylvik_document::nodes::OpticalFlow),
    Ocio(veylvik_document::nodes::Ocio),
    /// A particle system's emitter in its recent past.
    History(veylvik_document::nodes::Particles),
    /// A picture made at its own size.
    OwnSize,
    /// Another particle system's grains counted into a grid, at the same moment.
    Shade,
    /// One input of an OpenFX effect that reads other frames.
    Frames {
        node: NodeId,
        port: &'static str,
        effect: veylvik_document::nodes::OfxEffect,
    },
}
impl Kind {
    /// The kind of a node that replaces itself by a branch of its input (motion blur,
    /// time remap, optical flow, OCIO).
    /// Whether `operation` makes a branch in place of its node (see `replacing`), without
    /// copying it.
    fn replaces(operation: &Operation) -> bool {
        matches!(
            operation,
            Operation::MotionBlur(_)
                | Operation::TimeRemap(_)
                | Operation::OpticalFlow(_)
                | Operation::Ocio(_)
        )
    }
    fn replacing(operation: &Operation) -> Option<Self> {
        Some(match operation {
            Operation::MotionBlur(n) => Kind::MotionBlur(n.clone()),
            Operation::TimeRemap(n) => Kind::TimeRemap(n.clone()),
            Operation::OpticalFlow(n) => Kind::OpticalFlow(n.clone()),
            Operation::Ocio(n) => Kind::Ocio(n.clone()),
            _ => return None,
        })
    }
}
#[path = "space.rs"]
mod space;
pub struct Plan {
    pub(crate) ocio: veylvik_document::color_management::OcioViews,
    pub(crate) temporal: Vec<TemporalBranch>,
    pub(crate) frame_rate: veylvik_document::Time,
    /// The composition's length in frames.
    pub(crate) frame_count: u32,
    working_space: veylvik_document::color_management::WorkingSpace,
    root: NodeId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) nodes: Vec<Node>,
    /// Nodes read as data, never rendered: inputs of light rigs (planar tracks), and
    /// Own size nodes (a scene reads their size).
    pub(crate) light_data: Vec<Node>,
    /// Nodes that expressions in this plan read; a change to them changes the plan.
    link_data: Vec<Node>,
    pub(crate) curves: Vec<BTreeMap<&'static str, CompiledScalar>>,
    pub(crate) passes: Vec<Pass>,
    /// Nodes that use another node's pass because they are the same thing (owner, user):
    /// the owner's pass has to run whenever a user can reach the picture.
    shared_with: Vec<(NodeId, NodeId)>,
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
            veylvik_document::registry::PortKind::Forces => {
                return Err(error(
                    "A Force has no image of its own; view the Particles it drives",
                ));
            }
            veylvik_document::registry::PortKind::Scene => {
                return Err(error(
                    "A 3D scene has no image of its own; view the Render scene node that films it",
                ));
            }
            veylvik_document::registry::PortKind::Sound => {
                return Err(error(
                    "A Sound has no image; it plays on the timeline and in exports",
                ));
            }
            // Shown fitted into the frame.
            veylvik_document::registry::PortKind::Picture => {}
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
        // Own size nodes whose picture must meet the frame: fitted for an ordinary
        // input or the view, stretched whole for a particle sprite.
        let mut sized_nodes: Vec<NodeId> = Vec::new();
        let mut fitted: BTreeSet<NodeId> = BTreeSet::new();
        let mut stretched: BTreeSet<NodeId> = BTreeSet::new();
        if matches!(composition.nodes[&output].operation, Operation::OwnSize(_)) {
            fitted.insert(output);
        }
        while let Some((id, expanded)) = todo.pop() {
            let node = &composition.nodes[&id];
            // What feeds an Own size node is a plan of that size, rendered apart; the
            // node itself is never a pass here.
            if let Operation::OwnSize(own) = &node.operation {
                if visited.insert(id) {
                    let mut sized = composition.clone();
                    (sized.width, sized.height) = (own.width, own.height);
                    let plan =
                        Self::compile_branch(&sized, node.inputs["image"], depth + 1, remaining)?;
                    // Footage is placed for the plan that reads it, so it follows the
                    // branch's size. A plain Image is its file's pixels at the
                    // composition's size and cannot.
                    if plan.holds_plain_image() {
                        return Err(error(format!(
                            "{}: an Own size branch cannot hold a plain Image node, whose pixels are the composition's size; use Footage, which is placed",
                            node.label
                        )));
                    }
                    temporal.push(TemporalBranch {
                        kind: Kind::OwnSize,
                        plan: Box::new(plan),
                        key: own_size_key(id),
                    });
                    // The scene reads the node itself for the picture's size.
                    light_data.push(node.clone());
                    sized_nodes.push(id);
                }
                continue;
            }
            if expanded {
                *remaining = remaining
                    .checked_sub(1)
                    .ok_or_else(|| error("Temporal graph expansion exceeds 16384 nodes"))?;
                if let Some(kind) = Kind::replacing(&node.operation) {
                    let key = format!("\0temporal:{}", node.id);
                    temporal.push(TemporalBranch {
                        kind,
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
                    // An OpenFX plugin that reads other frames gets each input as a
                    // branch, rendered at the frames it names.
                    for (port, source) in ofx_branches(node) {
                        let Operation::OfxEffect(effect) = &node.operation else {
                            unreachable!("only OpenFX effects have OpenFX branches")
                        };
                        temporal.push(TemporalBranch {
                            kind: Kind::Frames {
                                node: node.id,
                                port,
                                effect: effect.clone(),
                            },
                            plan: Box::new(Self::compile_branch(
                                composition,
                                source,
                                depth + 1,
                                remaining,
                            )?),
                            key: ofx_key(node.id, port),
                        });
                    }
                    if let Operation::Particles(particles) = &node.operation
                        && crate::nodes::particles::shaded_by_another(particles, node)
                    {
                        let other = particles.shaded_by.expect("shaded by another");
                        temporal.push(TemporalBranch {
                            kind: Kind::Shade,
                            plan: Box::new(Self::compile_branch(
                                &shading_copy(composition, node, particles, other)?,
                                other,
                                depth + 1,
                                remaining,
                            )?),
                            key: shade_key(node.id),
                        });
                    }
                    if let Some(source) = history_source(node)
                        && let Operation::Particles(particles) = &node.operation
                    {
                        temporal.push(TemporalBranch {
                            kind: Kind::History(particles.clone()),
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
                if Kind::replaces(&node.operation) {
                    continue;
                }
                // Copies at particles read the particles as data: their places are worked
                // out for the scene render, and their own picture is never made.
                if let Operation::ParticleInstances(_) = &node.operation {
                    let mut more: Vec<NodeId> =
                        node.inputs.get("particles").copied().into_iter().collect();
                    while let Some(id) = more.pop() {
                        if light_data.iter().any(|n| n.id == id) {
                            continue;
                        }
                        let n = &composition.nodes[&id];
                        light_data.push(n.clone());
                        match &n.operation {
                            Operation::Particles(_) => {
                                more.extend(
                                    ["scene", "forces"]
                                        .iter()
                                        .filter_map(|port| n.inputs.get(*port).copied()),
                                );
                                // The pictures they read are made as pictures and handed
                                // to the scene render (see INSTANCE_PICTURES).
                                for port in INSTANCE_PICTURES {
                                    if let Some(source) = n.inputs.get(port) {
                                        todo.push((*source, false));
                                    }
                                }
                            }
                            _ => more.extend(n.inputs.values().copied()),
                        }
                    }
                    if let Some(scene) = node.inputs.get("scene") {
                        todo.push((*scene, false));
                    }
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
                    if ofx_branches(node).iter().any(|(p, _)| p == port) {
                        continue;
                    }
                    if let Some(source) = node.inputs.get(*port) {
                        if matches!(composition.nodes[source].operation, Operation::OwnSize(_))
                            && !node.operation.descriptor().takes_picture(port)
                        {
                            if is_sprite(node, port) {
                                stretched.insert(*source);
                            } else {
                                fitted.insert(*source);
                            }
                        }
                        todo.push((*source, false));
                    }
                }
            }
        }
        // Those nodes come first: they read only their own branch's picture.
        for id in sized_nodes.into_iter().rev() {
            if fitted.contains(&id) || stretched.contains(&id) {
                nodes.insert(0, composition.nodes[&id].clone());
            }
        }
        let mut passes: Vec<Pass> = Vec::new();
        let mut shared_with = Vec::new();
        let mut stretches = BTreeMap::new();
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
            // An Own size picture, placed in the frame for what needs the frame's size.
            if let Operation::OwnSize(_) = &node.operation {
                for (wanted, kernel) in [
                    (fitted.contains(&node.id), Kernel::OwnSizeFit),
                    (stretched.contains(&node.id), Kernel::OwnSizeStretch),
                ] {
                    if !wanted {
                        continue;
                    }
                    let value = passes.len();
                    passes.push(Pass {
                        node: index,
                        kernel,
                        inputs: [
                            Input::Asset(own_size_key(node.id)),
                            Input::Empty,
                            Input::Empty,
                            Input::Empty,
                        ],
                        output: value,
                        cards: Vec::new(),
                    });
                    if kernel == Kernel::OwnSizeFit {
                        outputs.insert(node.id, value);
                    } else {
                        stretches.insert(node.id, value);
                    }
                }
                continue;
            }
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
                if ofx_branches(node).iter().any(|(p, _)| p == port) {
                    inputs[i] = Input::Asset(ofx_key(node.id, port));
                    continue;
                }
                // An unconnected optional port stays an empty input.
                let Some(&source) = node.inputs.get(*port) else {
                    continue;
                };
                let mut value = match stretches.get(&source) {
                    Some(whole) if is_sprite(node, port) => *whole,
                    _ => outputs[&source],
                };
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
            // (Only with something to pass on: an optional first input may be unwired.)
            if identity(&node.operation)
                && let Input::Value(value) = inputs[0]
            {
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
                if let Some(owner) = passes.get(*value) {
                    shared_with.push((nodes[owner.node].id, node.id));
                }
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
                        crate::nodes::From::Shade => Input::Asset(shade_key(node.id)),
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
                            let pictured = match &n.operation {
                                Operation::ImageCard(_) | Operation::Environment3d(_) => {
                                    n.inputs.get("image")
                                }
                                Operation::Shape3d(_) => n.inputs.get("texture"),
                                Operation::CameraProjection(_) => n.inputs.get("image"),
                                _ => None,
                            };
                            // Copies at particles: the pictures their particles read, at
                            // the frame's size, beside the cards.
                            if let Operation::ParticleInstances(_) = &n.operation
                                && let Some(particles) = n
                                    .inputs
                                    .get("particles")
                                    .and_then(|id| composition.nodes.get(id))
                            {
                                for port in INSTANCE_PICTURES {
                                    if let Some(&source) = particles.inputs.get(port)
                                        && !cards.iter().any(|(s, _)| *s == source)
                                        && let Some(&value) = outputs.get(&source)
                                    {
                                        cards.push((source, Input::Value(value)));
                                    }
                                }
                            }
                            // A picture of its own size is read as it is.
                            if let Some(&source) = pictured
                                && !cards.iter().any(|(s, _)| *s == source)
                                && matches!(
                                    composition.nodes.get(&source).map(|s| &s.operation),
                                    Some(Operation::OwnSize(_))
                                )
                            {
                                cards.push((source, Input::Asset(own_size_key(source))));
                            }
                            if let Some(&source) = pictured
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
            frame_count: composition.frame_count,
            working_space: composition.working_space,
            root: output,
            curves,
            width: composition.width,
            height: composition.height,
            nodes,
            link_data,
            light_data,
            shared_with,
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
    pub fn visible_at(&self, time: veylvik_document::Time) -> Result<BTreeSet<NodeId>> {
        let index: BTreeMap<NodeId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        // Nodes read as data (particles that copies are placed at, a rig's sources) are
        // not passes, but what they read may be: the walk goes through them.
        let data: BTreeMap<NodeId, &veylvik_document::Node> =
            self.light_data.iter().map(|n| (n.id, n)).collect();
        let mut visible = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut pending = vec![self.root];
        while let Some(id) = pending.pop() {
            let Some(&i) = index.get(&id) else {
                if let Some(node) = data.get(&id)
                    && seen.insert(id)
                {
                    pending.extend(node.inputs.values().copied());
                }
                continue;
            };
            if !seen.insert(id) {
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
            // A node whose picture is clear whatever it is given is not run, and nothing
            // above it is looked at; what reads it finds a clear picture.
            if veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::empty(n, &sample, time))?
            {
                continue;
            }
            visible.insert(id);
            let idle = veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::idle_inputs(n, &sample, time))?;
            pending.extend(
                node.inputs
                    .iter()
                    .filter(|(port, _)| !idle.contains(&port.as_str()))
                    .map(|(_, id)| *id),
            );
        }
        // A node that stands in for an identical one runs on that one's behalf.
        for (owner, user) in &self.shared_with {
            if visible.contains(user) {
                visible.insert(*owner);
            }
        }
        Ok(visible)
    }
    /// The file a footage source may be read from when its key is not at hand: a host
    /// names its own media by keys only it knows.
    pub fn fallback(&self, key: &str) -> Option<&str> {
        self.nodes.iter().find_map(|node| match &node.operation {
            veylvik_document::Operation::Footage(footage)
                if !footage.fallback.is_empty()
                    && node.operation.source_key().as_deref() == Some(key) =>
            {
                Some(footage.fallback.as_str())
            }
            _ => None,
        })
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
            .flat_map(|p| p.inputs.iter().chain(p.cards.iter().map(|(_, i)| i)))
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
    /// The Gaussian splat files the plan's scenes show.
    pub fn required_splats(&self) -> BTreeSet<&str> {
        self.nodes
            .iter()
            .chain(&self.light_data)
            .filter_map(|n| match &n.operation {
                Operation::GaussianSplats(g) if !g.file.is_empty() => Some(g.file.as_str()),
                _ => None,
            })
            .collect()
    }
    /// The baked flocks the plan's particles read (their `bake` files).
    pub fn required_flocks(&self) -> BTreeSet<&str> {
        self.nodes
            .iter()
            .chain(&self.light_data)
            .filter_map(|n| match &n.operation {
                Operation::Particles(p) if !p.bake.is_empty() => Some(p.bake.as_str()),
                _ => None,
            })
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
    /// Whether the plan, or a branch of it, reads a plain Image node's file.
    fn holds_plain_image(&self) -> bool {
        self.nodes.iter().any(
            |n| matches!(&n.operation, Operation::Image(image) if !image.asset.starts_with('\0')),
        ) || self.temporal.iter().any(|b| b.plan.holds_plain_image())
    }
    /// Whether the plan has temporal branches (motion blur, retiming, flow, OCIO).
    pub fn has_temporal(&self) -> bool {
        !self.temporal.is_empty()
    }
    /// Whether the plan runs an OpenFX effect anywhere, its branches included.
    pub fn runs_effects(&self) -> bool {
        self.passes
            .iter()
            .any(|pass| pass.kernel == Kernel::External)
            || self
                .temporal
                .iter()
                .any(|branch| branch.plan.runs_effects())
    }
    /// Whether the plan films a 3D scene anywhere, its branches included.
    pub fn films_scenes(&self) -> bool {
        self.passes
            .iter()
            .any(|pass| matches!(pass.kernel, Kernel::RenderScene | Kernel::ParticleShadows))
            || self
                .temporal
                .iter()
                .any(|branch| branch.plan.films_scenes())
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
            if let Kind::Ocio(v) = &branch.kind {
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
/// The pictures a Particles node reads that copies at its particles (Particle
/// instances) use too: the emitter they are born on, the target they fly to, the field
/// that steers them and the obstacle they bounce off. Planned as pictures and handed to
/// the Render scene that films the copies.
pub(crate) const INSTANCE_PICTURES: [&str; 4] = ["emitter", "target", "field", "obstacle"];
/// The source of the input a node reads through its history, when it asks for one.
pub(crate) fn history_source(node: &Node) -> Option<NodeId> {
    match &node.operation {
        // A flock reads its emitter once, as it is at the flock's start (see `crate::moments`).
        Operation::Particles(particles)
            if particles.history > 0 || crate::nodes::particles::simulating(particles) =>
        {
            node.inputs.get(HISTORY_PORT).copied()
        }
        _ => None,
    }
}
/// The wired ports of an OpenFX effect that reads other frames, with their sources.
pub(crate) fn ofx_branches(node: &Node) -> Vec<(&'static str, NodeId)> {
    match &node.operation {
        Operation::OfxEffect(effect) if effect.other_frames && !effect.plugin.is_empty() => {
            veylvik_document::nodes::OFX_PORTS
                .iter()
                .zip(&effect.clips)
                .filter(|(_, clip)| !clip.is_empty())
                .filter_map(|(port, _)| node.inputs.get(*port).map(|source| (*port, *source)))
                .collect()
        }
        _ => Vec::new(),
    }
}
/// The input key of an OpenFX effect's port read as a branch, at its own frame.
pub(crate) fn ofx_key(id: NodeId, port: &str) -> String {
    format!("\0temporal:{id}:{port}")
}
/// The input key of that branch at another frame.
pub(crate) fn ofx_frame_key(id: NodeId, port: &str, frame: f64) -> String {
    format!("\0temporal:{id}:{port}@{}", frame.to_bits())
}
pub(crate) fn shade_key(id: veylvik_document::NodeId) -> String {
    format!("\0shade:{id}")
}
/// The composition with `other` made only to count its grains for `node`: it must be
/// another particle system filmed as `node`'s are (the same scene and camera), or the
/// two grids would not lie over each other.
fn shading_copy(
    composition: &Composition,
    node: &Node,
    particles: &veylvik_document::nodes::Particles,
    other: NodeId,
) -> Result<Composition> {
    let refuse = |why: &str| Err(error(format!("{}: Shaded by {why}", node.label)));
    let Some(found) = composition.nodes.get(&other) else {
        return refuse("names a node that is not in the composition");
    };
    let Operation::Particles(theirs) = &found.operation else {
        return refuse(&format!(
            "names {}, which is not a particle system",
            found.label
        ));
    };
    if other == node.id {
        return refuse("cannot name the system itself (Self shadow does that)");
    }
    if crate::nodes::particles::simulating(theirs) {
        return refuse(&format!(
            "names {}, a flock, which cannot shade another system",
            found.label
        ));
    }
    let filmed = |n: &Node, p: &veylvik_document::nodes::Particles| {
        (
            n.inputs.get("scene").copied(),
            format!("{:?}", p.camera),
            p.with_camera,
        )
    };
    if filmed(found, theirs) != filmed(node, particles) {
        return refuse(&format!(
            "names {}, which is not filmed by the same scene and camera (connect the same scene to both, with the same Camera)",
            found.label
        ));
    }
    let mut copy = composition.clone();
    if let Some(n) = copy.nodes.get_mut(&other)
        && let Operation::Particles(theirs) = &mut n.operation
    {
        theirs.counting = true;
        theirs.shaded_by = None;
    }
    Ok(copy)
}
pub(crate) fn history_key(id: veylvik_document::NodeId) -> String {
    format!("\0temporal:{id}")
}
/// The input key of a node's own data image (`RenderNode::data`).
pub(crate) fn data_key(id: veylvik_document::NodeId) -> String {
    format!("\0data:{id}")
}
/// Whether `port` of `node` is a particle sprite, which takes a whole picture.
fn is_sprite(node: &Node, port: &str) -> bool {
    matches!(node.operation, Operation::Particles(_)) && port == "sprite"
}
/// The input key of an Own size node's picture.
pub(crate) fn own_size_key(id: veylvik_document::NodeId) -> String {
    format!("\0own:{id}")
}
pub(crate) fn footage_key(id: veylvik_document::NodeId) -> String {
    format!("\0footage:{id}")
}
