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
    Special,
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
pub(crate) trait RenderNode {
    fn passes(&self) -> Passes {
        Passes::Special
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
    /// Passes its input through unchanged with these parameters.
    fn identity(&self) -> bool {
        false
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
mod text;
mod tracking;
mod vector;
