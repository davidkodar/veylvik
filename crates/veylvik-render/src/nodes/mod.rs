//! Per-node GPU behaviour, grouped like `veylvik_document::nodes`: which kernels a node
//! runs, how its animated values fill the kernel uniforms, and its caching policy.
use crate::{Kernel, Params, Result};
use veylvik_document::{Node, Time};

/// Kernels a node compiles to; `Special` nodes are handled structurally by the planner
/// (media sources, vector coverage, track aliases).
pub(crate) enum Passes {
    One(Kernel),
    /// Horizontal then vertical pass through an intermediate image.
    Separable(Kernel, Kernel),
    /// A pipeline of passes; the last step's output is the node's output. Fixed
    /// chains borrow a static list; nodes whose wiring changes the work build one.
    Chain(std::borrow::Cow<'static, [Step]>),
    Special,
}
impl Passes {
    pub(crate) const fn chain(steps: &'static [Step]) -> Self {
        Self::Chain(std::borrow::Cow::Borrowed(steps))
    }
}
/// Where a chain step reads an image from.
#[derive(Clone, Copy)]
pub(crate) enum From {
    /// The node's input port at this index.
    Port(usize),
    /// An earlier step's output.
    Step(usize),
    /// The node's own data image (`RenderNode::data`).
    Data,
    None,
}
#[derive(Clone)]
pub(crate) struct Step {
    pub kernel: Kernel,
    /// Bound in order to `source`, `second`, `third` and `fourth`.
    pub inputs: [From; 4],
}
/// What a node's uniforms may depend on besides its own animated values.
#[derive(Clone, Copy)]
pub(crate) struct Cx<'a> {
    pub node: &'a Node,
    pub nodes: &'a [Node],
    pub width: u32,
    pub height: u32,
    pub time: Time,
}
/// Where a picture can be other than clear: nowhere, within a rectangle of the frame
/// (`[left, top, right, bottom]` in pixels, right and bottom not included), or anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    Nothing,
    Area([i32; 4]),
    Everything,
}
impl Extent {
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Everything, _) | (_, Self::Everything) => Self::Everything,
            (Self::Nothing, e) | (e, Self::Nothing) => e,
            (Self::Area(a), Self::Area(b)) => Self::Area([
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]),
        }
    }
    /// Where both can be other than clear.
    pub fn intersect(self, other: Self) -> Self {
        match (self, other) {
            (Self::Nothing, _) | (_, Self::Nothing) => Self::Nothing,
            (Self::Everything, e) | (e, Self::Everything) => e,
            (Self::Area(a), Self::Area(b)) => {
                let area = [
                    a[0].max(b[0]),
                    a[1].max(b[1]),
                    a[2].min(b[2]),
                    a[3].min(b[3]),
                ];
                if area[0] >= area[2] || area[1] >= area[3] {
                    Self::Nothing
                } else {
                    Self::Area(area)
                }
            }
        }
    }
    /// Wider on every side by `pixels`.
    pub fn grown(self, pixels: i32) -> Self {
        match self {
            Self::Area(a) => {
                Self::Area([a[0] - pixels, a[1] - pixels, a[2] + pixels, a[3] + pixels])
            }
            other => other,
        }
    }
    pub fn contains(self, x: i32, y: i32) -> bool {
        match self {
            Self::Nothing => false,
            Self::Everything => true,
            Self::Area(a) => x >= a[0] && y >= a[1] && x < a[2] && y < a[3],
        }
    }
}
/// How much of an input an area of the output is made from: the same area, that area
/// and so many pixels round it, or possibly any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Same,
    Around(u32),
    Everything,
}
impl Reach {
    /// Through one node and then another.
    pub fn then(self, next: Self) -> Self {
        match (self, next) {
            (Self::Everything, _) | (_, Self::Everything) => Self::Everything,
            (Self::Same, r) | (r, Self::Same) => r,
            (Self::Around(a), Self::Around(b)) => Self::Around(a.saturating_add(b)),
        }
    }
    /// By either of two ways: the further of them.
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Everything, _) | (_, Self::Everything) => Self::Everything,
            (Self::Same, r) | (r, Self::Same) => r,
            (Self::Around(a), Self::Around(b)) => Self::Around(a.max(b)),
        }
    }
}
pub(crate) trait RenderNode {
    /// Where the output can be other than clear, given where each input can be. A node
    /// that does not say is taken to fill the frame: more work later, never a wrong picture.
    fn extent(
        &self,
        _sample: &dyn Fn(&str) -> Result<f32>,
        _input: &dyn Fn(&str) -> Extent,
    ) -> Result<Extent> {
        Ok(Extent::Everything)
    }
    /// How much of the input at `port` an area of the output is made from. A node that
    /// does not say is taken to need all of it.
    fn reach(&self, _sample: &dyn Fn(&str) -> Result<f32>, _port: &str) -> Result<Reach> {
        Ok(Reach::Everything)
    }
    fn passes(&self) -> Passes {
        Passes::Special
    }
    /// Passes for this node's wiring; nodes with optional inputs can skip work
    /// for an unconnected port.
    fn passes_for(&self, _node: &Node) -> Passes {
        self.passes()
    }
    /// Fills kernel uniforms; `sample` evaluates a registered scalar at the frame time.
    fn params(
        &self,
        _cx: &Cx<'_>,
        _sample: &dyn Fn(&str) -> Result<f32>,
        _p: &mut Params,
    ) -> Result<()> {
        Ok(())
    }
    /// Pure output that identical inputs and parameters may share between nodes.
    /// External and vector sources keep their own ownership policy, so nodes opt in.
    fn shareable(&self) -> bool {
        false
    }
    /// A small image of the node's own data, bound after its inputs (a warp's mesh).
    /// The same data must give the same image, so frames stay cached.
    fn data(&self, _cx: &Cx<'_>) -> Result<Option<std::sync::Arc<crate::Image>>> {
        Ok(None)
    }
    /// Whether `data` gives an image (decided without building it).
    fn has_data(&self) -> bool {
        false
    }
    /// Passes its input through unchanged with these parameters.
    fn identity(&self) -> bool {
        false
    }
    /// The output is clear at this time whatever the inputs are (a layer faded right
    /// out, a particle system with none alive): then the node is not run at all, nothing
    /// upstream of it is, and what reads it reads a clear picture.
    fn empty(&self, _sample: &dyn Fn(&str) -> Result<f32>, _time: Time) -> Result<bool> {
        Ok(false)
    }
    /// Input ports whose content cannot affect the output at this time, such as a fully
    /// transparent blend's foreground; footage reached only through them is not decoded.
    fn idle_inputs(
        &self,
        _sample: &dyn Fn(&str) -> Result<f32>,
        _time: Time,
    ) -> Result<&'static [&'static str]> {
        Ok(&[])
    }
}

mod color;
mod composite;
mod filters;
mod generators;
mod keying;
mod materials;
mod media;
pub(crate) mod particles;
mod scene;
mod text;
mod tracking;
mod vector;
