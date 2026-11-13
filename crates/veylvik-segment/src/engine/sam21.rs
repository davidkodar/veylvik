//! SAM 2.1 (Hiera) from Diffusion Studio's fixed-shape export: fp16 inside, f32 at the edges.
//! The memory bank is SAM 2's own (prompted frame, six recent frames, sixteen object pointers)
//! and always full: until the shot fills it, the prompted frame's memory and pointer repeat.
use super::{ort_error, pixels, prompt_tensors, session, shape_error, tokens};
use crate::{Point, Result, SIDE, worker::Mask};
use ndarray::{Array, Array1, ArrayD, Axis};
use ort::{session::Session, value::Tensor};
use std::{collections::VecDeque, path::Path};

const BLOCKS: usize = 7;
const POINTERS: usize = 16;
const TOKENS: usize = 4096;
const MEMORY_DIM: usize = 64;
const HIDDEN: usize = 256;
/// Spatial memory then pointer tokens, as the attention graph's fixed input.
const BANK: usize = BLOCKS * TOKENS + POINTERS * HIDDEN / MEMORY_DIM;

struct Memory {
    tokens: Vec<f32>,
    pos: Vec<f32>,
    pointer: Vec<f32>,
}
pub struct Engine {
    encoder: Session,
    decoder: Session,
    attention: Session,
    memory_encoder: Session,
    pointer_tpos: Session,
    mean: [f32; 3],
    std: [f32; 3],
    /// BLOCKS rows of MEMORY_DIM.
    temporal: Vec<f32>,
    conditioning: Option<Memory>,
    /// Newest first.
    recent: VecDeque<Memory>,
    /// Frames since the prompted one.
    step: usize,
}
fn constants(dir: &Path) -> Result<([f32; 3], [f32; 3], Vec<f32>)> {
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("constants.json"))?)?;
    if value["image_size"] != SIDE {
        return Err("this SAM 2.1 build does not use 1024 × 1024 input".into());
    }
    let triple = |name: &str| -> Result<[f32; 3]> {
        let v: Vec<f32> = serde_json::from_value(value[name].clone())?;
        v.try_into()
            .map_err(|_| format!("{name} needs three values").into())
    };
    let rows: Vec<Vec<f32>> =
        serde_json::from_value(value["memory_temporal_positional_encoding"].clone())?;
    if rows.len() != BLOCKS || rows.iter().any(|r| r.len() != MEMORY_DIM) {
        return Err("unexpected temporal encoding table".into());
    }
    Ok((triple("image_mean")?, triple("image_std")?, rows.concat()))
}
impl super::Tracker for Engine {
    fn frame(&mut self, rgb: &[u8], prompt: Option<&[Point]>) -> Result<Mask> {
        let encoded = self
            .encoder
            .run(ort::inputs![
                "pixel_values" => Tensor::from_array(pixels(rgb, self.mean, self.std)).map_err(ort_error)?
            ])
            .map_err(ort_error)?;
        let get = |name: &str| -> Result<ArrayD<f32>> {
            Ok(encoded[name]
                .try_extract_array::<f32>()
                .map_err(ort_error)?
                .to_owned())
        };
        let (f0, f1, f2) = (get("feats0")?, get("feats1")?, get("feats2")?);
        let (unconditioned, pos) = (get("feats2_no_mem")?, get("vision_pos_embed")?);
        drop(encoded);
        let conditioned = match prompt {
            Some(points) => {
                crate::validate_points(points)?;
                self.conditioning = None;
                self.recent.clear();
                self.step = 0;
                unconditioned
            }
            None => {
                self.step += 1;
                self.attend(&f2, &pos)?
            }
        };
        let (points, labels) = prompt_tensors(prompt)?;
        let decoded = self
            .decoder
            .run(ort::inputs![
                "feats0" => Tensor::from_array(f0).map_err(ort_error)?,
                "feats1" => Tensor::from_array(f1).map_err(ort_error)?,
                "feats2_cond" => Tensor::from_array(conditioned).map_err(ort_error)?,
                "input_points" => Tensor::from_array(points).map_err(ort_error)?,
                "input_labels" => Tensor::from_array(labels).map_err(ort_error)?
            ])
            .map_err(ort_error)?;
        let mask = decoded["high_res_mask"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .to_owned();
        if mask.shape() != [1, 1, SIDE, SIDE] {
            return Err(format!("unexpected mask shape {:?}", mask.shape()).into());
        }
        let score = decoded["object_score_logits"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .iter()
            .copied()
            .next()
            .unwrap_or(f32::NEG_INFINITY);
        let pointer: Vec<f32> = decoded["object_pointer"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .iter()
            .copied()
            .collect();
        drop(decoded);
        if pointer.len() != HIDDEN {
            return Err("unexpected object pointer size".into());
        }
        let encoded = self
            .memory_encoder
            .run(ort::inputs![
                "feats2" => Tensor::from_array(f2).map_err(ort_error)?,
                "high_res_mask" => Tensor::from_array(mask.clone()).map_err(ort_error)?,
                "object_score_logits" => Tensor::from_array(Array::from_elem((1, 1), score)).map_err(ort_error)?,
                "binarize" => Tensor::from_array(ndarray::arr0(if prompt.is_some() { 1.0f32 } else { 0.0 })).map_err(ort_error)?
            ])
            .map_err(ort_error)?;
        let flat = |name: &str| -> Result<Vec<f32>> {
            Ok(encoded[name]
                .try_extract_array::<f32>()
                .map_err(ort_error)?
                .iter()
                .copied()
                .collect())
        };
        let memory = Memory {
            tokens: flat("memory_tokens")?,
            pos: flat("memory_pos")?,
            pointer,
        };
        drop(encoded);
        if memory.tokens.len() != TOKENS * MEMORY_DIM || memory.pos.len() != memory.tokens.len() {
            return Err("unexpected memory size".into());
        }
        if prompt.is_some() {
            self.conditioning = Some(memory);
        } else {
            self.recent.push_front(memory);
            self.recent.truncate(POINTERS - 1);
        }
        Ok(Mask {
            logits: mask.index_axis(Axis(0), 0).iter().copied().collect(),
            score,
        })
    }
}
impl Engine {
    pub fn load(dir: &Path) -> Result<Self> {
        let (mean, std, temporal) = constants(dir)?;
        Ok(Self {
            encoder: session(dir, "vision_encoder")?,
            decoder: session(dir, "mask_decoder")?,
            attention: session(dir, "memory_attention")?,
            memory_encoder: session(dir, "memory_encoder")?,
            pointer_tpos: session(dir, "pointer_tpos")?,
            mean,
            std,
            temporal,
            conditioning: None,
            recent: VecDeque::new(),
            step: 0,
        })
    }
    /// Conditions the frame's features on the full, padded memory bank.
    fn attend(&mut self, f2: &ArrayD<f32>, pos: &ArrayD<f32>) -> Result<ArrayD<f32>> {
        let conditioning = self
            .conditioning
            .as_ref()
            .ok_or("track a subject from a prompted frame first")?;
        // Spatial blocks: the prompted frame (temporal row 6) repeated into unused slots, then
        // recent frames oldest first, a frame k steps back using row k − 1.
        let recent = self.recent.len().min(BLOCKS - 1);
        let mut blocks = vec![(conditioning, BLOCKS - 1); BLOCKS - recent];
        for k in (1..=recent).rev() {
            blocks.push((&self.recent[k - 1], k - 1));
        }
        let mut memory = Vec::with_capacity(BANK * MEMORY_DIM);
        let mut memory_pos = Vec::with_capacity(BANK * MEMORY_DIM);
        for (block, row) in blocks {
            memory.extend_from_slice(&block.tokens);
            let encoding = &self.temporal[row * MEMORY_DIM..(row + 1) * MEMORY_DIM];
            memory_pos.extend(
                block
                    .pos
                    .iter()
                    .enumerate()
                    .map(|(i, v)| v + encoding[i % MEMORY_DIM]),
            );
        }
        // Pointers: the prompted frame's, then recent ones; repeats of the prompted frame's
        // fill the sixteen slots. Each is split into four 64-wide tokens sharing a position.
        let mut pointers = vec![(&conditioning.pointer, self.step)];
        for (k, m) in self.recent.iter().take(POINTERS - 1).enumerate() {
            pointers.push((&m.pointer, k + 1));
        }
        while pointers.len() < POINTERS {
            pointers.push((&conditioning.pointer, self.step));
        }
        let diffs: Vec<f32> = pointers
            .iter()
            .map(|(_, d)| *d as f32 / (POINTERS - 1) as f32)
            .collect();
        let out = self
            .pointer_tpos
            .run(ort::inputs![
                "normalized_diffs" => Tensor::from_array(Array1::from_vec(diffs)).map_err(ort_error)?
            ])
            .map_err(ort_error)?;
        let positions: Vec<f32> = out["pointer_pos"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .iter()
            .copied()
            .collect();
        drop(out);
        if positions.len() != POINTERS * MEMORY_DIM {
            return Err("unexpected pointer position size".into());
        }
        for (i, (pointer, _)) in pointers.iter().enumerate() {
            memory.extend_from_slice(pointer);
            for _ in 0..HIDDEN / MEMORY_DIM {
                memory_pos.extend_from_slice(&positions[i * MEMORY_DIM..(i + 1) * MEMORY_DIM]);
            }
        }
        let tensor = |data: Vec<f32>| -> Result<Tensor<f32>> {
            Tensor::from_array(
                Array::from_shape_vec((BANK, 1, MEMORY_DIM), data).map_err(shape_error)?,
            )
            .map_err(ort_error)
        };
        let out = self
            .attention
            .run(ort::inputs![
                "current_vision_features" => Tensor::from_array(tokens(f2)?).map_err(ort_error)?,
                "current_vision_position_embeddings" => Tensor::from_array(tokens(pos)?).map_err(ort_error)?,
                "memory" => tensor(memory)?,
                "memory_pos" => tensor(memory_pos)?
            ])
            .map_err(ort_error)?;
        Ok(out["conditioned_feats"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .to_owned())
    }
}
