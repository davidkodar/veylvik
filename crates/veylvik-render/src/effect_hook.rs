//! The seam where a host-registered runner computes nodes made outside the renderer (OpenFX plugins), so this crate loads no plugin code: the renderer reads the node's inputs back, the runner returns its picture, and the renderer uploads it.
use crate::Result;
use veylvik_document::nodes::{OfxEffect, OfxValue};

/// Runs OpenFX effects. Studio and the CLI register one (over the plugin helper
/// process) with `Renderer::set_effect_host`.
pub trait EffectHost: Send + Sync {
    /// The effect's picture: `width * height` premultiplied scene-linear Rec.709 RGBA
    /// floats, top row first, like each input.
    fn render(&self, job: EffectJob<'_>) -> Result<Vec<f32>>;
}
pub struct EffectJob<'a> {
    pub effect: &'a OfxEffect,
    /// The settings' values at this moment.
    pub values: Vec<(String, OfxValue)>,
    /// The frame number at this moment (OpenFX counts time in frames).
    pub frame: f64,
    pub width: u32,
    pub height: u32,
    /// Each connected input: the plugin clip it feeds and its pixels.
    pub inputs: Vec<(&'a str, Vec<f32>)>,
}
