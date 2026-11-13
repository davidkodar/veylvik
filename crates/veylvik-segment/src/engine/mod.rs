//! Model engines on ONNX Runtime CUDA, used only inside the worker process. Trackers share one
//! interface so any installed tracking model drives the same analysis; no CPU fallback.
mod edgetam;
mod sam21;
pub mod vitmatte;
use crate::{Point, Result, SIDE, models::Model, worker::Mask};
use ndarray::{Array, Array4, ArrayD};
use ort::session::Session;
use std::path::Path;

/// A click-prompted video tracker: a prompt starts a new track on that frame; later frames
/// without one are propagated through the model's memory.
pub trait Tracker {
    fn frame(&mut self, rgb: &[u8], prompt: Option<&[Point]>) -> Result<Mask>;
}
/// Loads the tracker for an installed model.
pub fn load(model: &Model, dir: &Path) -> Result<Box<dyn Tracker>> {
    Ok(match model.id {
        "edgetam" => Box::new(edgetam::Engine::load(dir)?),
        "sam2.1-base-plus" | "sam2.1-large" => Box::new(sam21::Engine::load(dir)?),
        other => return Err(format!("{other} is not a tracking model").into()),
    })
}

pub(crate) fn ort_error(e: ort::Error) -> crate::Error {
    crate::Error::Message(format!("segmentation runtime: {e}"))
}
pub(crate) fn shape_error(e: ndarray::ShapeError) -> crate::Error {
    crate::Error::Message(format!("segmentation tensor shape: {e}"))
}
/// A CUDA session for `dir/name.onnx`; refuses to run without CUDA.
pub(crate) fn session(dir: &Path, name: &str) -> Result<Session> {
    ort::init().commit();
    Session::builder()
        .map_err(ort_error)?
        .with_execution_providers([ort::execution_providers::CUDAExecutionProvider::default()
            .build()
            .error_on_failure()])
        .map_err(|e| ort_error(e.into()))?
        .commit_from_file(dir.join(format!("{name}.onnx")))
        .map_err(|e| {
            crate::Error::Message(format!("cannot load {name} on CUDA (no CPU fallback): {e}"))
        })
}
pub(crate) fn floats(path: &Path, count: usize) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() != count * 4 {
        return Err(format!("{} has an unexpected size", path.display()).into());
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}
/// SIDE² sRGB bytes as a normalised [1,3,SIDE,SIDE] tensor.
pub(crate) fn pixels(rgb: &[u8], mean: [f32; 3], std: [f32; 3]) -> Array4<f32> {
    Array::from_shape_fn((1, 3, SIDE, SIDE), |(_, c, y, x)| {
        (f32::from(rgb[(y * SIDE + x) * 3 + c]) / 255.0 - mean[c]) / std[c]
    })
}
/// [1,C,H,W] → [H·W,1,C], the token layout memory attention expects.
pub(crate) fn tokens(map: &ArrayD<f32>) -> Result<ArrayD<f32>> {
    let s = map.shape();
    let (c, hw) = (s[1], s[2] * s[3]);
    let flat = map
        .view()
        .into_shape_with_order((c, hw))
        .map_err(shape_error)?;
    flat.t()
        .as_standard_layout()
        .into_owned()
        .into_shape_with_order((hw, 1, c))
        .map(|a| a.into_dyn())
        .map_err(shape_error)
}
/// Prompt points in model pixels with SAM labels (1 subject, 0 excluded); a single padding
/// point (label −1) without a prompt.
pub(crate) fn prompt_tensors(
    prompt: Option<&[Point]>,
) -> Result<(Array4<f32>, ndarray::Array3<i32>)> {
    match prompt {
        Some(points) => {
            let n = points.len();
            let coords = points
                .iter()
                .flat_map(|p| [p.x * SIDE as f32, p.y * SIDE as f32])
                .collect();
            let labels = points.iter().map(|p| i32::from(p.positive)).collect();
            Ok((
                Array::from_shape_vec((1, 1, n, 2), coords).map_err(shape_error)?,
                Array::from_shape_vec((1, 1, n), labels).map_err(shape_error)?,
            ))
        }
        None => Ok((Array::zeros((1, 1, 1, 2)), Array::from_elem((1, 1, 1), -1))),
    }
}
