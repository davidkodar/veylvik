//! The seam where a host-registered runner computes nodes made outside the renderer (OpenFX plugins), so this crate loads no plugin code: the renderer reads the node's inputs back, the runner returns its picture, and the renderer uploads it.
use crate::Result;
use veylvik_document::nodes::{OfxEffect, OfxValue};

/// Runs OpenFX effects. Studio and the CLI register one (over the plugin helper
/// process) with `Renderer::set_effect_host`.
pub trait EffectHost: Send + Sync {
    /// The frames of each input clip the effect needs to render `job.frame`; only asked
    /// of effects that read other frames.
    fn frames_needed(&self, _job: FramesJob<'_>) -> Result<Vec<(String, Vec<f64>)>> {
        Ok(Vec::new())
    }
    /// Makes the effect's picture and hands it to `deliver` once: `width * height`
    /// premultiplied scene-linear Rec.709 RGBA floats, top row first, like each input.
    /// (Handed over where it lies, so it is copied once, to the graphics card.)
    fn render(&self, job: EffectJob<'_>, deliver: &mut dyn FnMut(&[f32])) -> Result<()>;
}
pub struct EffectJob<'a> {
    pub effect: &'a OfxEffect,
    /// The plugin's bundle as checked against where the composition may read: load
    /// this, never `effect.bundle` (which may name it relative to the project).
    pub bundle: &'a std::path::Path,
    /// The settings' values at this moment.
    pub values: Vec<(String, OfxValue)>,
    /// The frame number at this moment (OpenFX counts time in frames).
    pub frame: f64,
    pub width: u32,
    pub height: u32,
    /// The composition's length in frames.
    pub duration: u32,
    /// Each connected input: the plugin clip it feeds, the frame it is of (None: this
    /// one) and its pixels.
    pub inputs: Vec<(&'a str, Option<f64>, Vec<f32>)>,
}
/// What an effect that reads other frames is asked before rendering.
pub struct FramesJob<'a> {
    pub effect: &'a OfxEffect,
    /// The plugin's bundle as checked (see [`EffectJob::bundle`]).
    pub bundle: &'a std::path::Path,
    /// The plugin clips whose ports are wired.
    pub connected: Vec<&'a str>,
    pub values: Vec<(String, OfxValue)>,
    pub frame: f64,
    pub width: u32,
    pub height: u32,
    pub duration: u32,
}
