//! EdgeTAM on ONNX Runtime CUDA, with the SAM 2 memory bank kept on the host: the prompted frame
//! conditions every later one, alongside the six most recent frames and sixteen object pointers.
use super::{floats, ort_error, pixels, prompt_tensors, session, shape_error, tokens};
use crate::{Point, Result, SIDE, worker::Mask};
use ndarray::{Array, Array1, ArrayD, Axis};
use ort::{session::Session, value::Tensor};
use std::{collections::VecDeque, path::Path};

/// Memory blocks the model was trained with: the conditioning frame plus six recent ones.
const MEMORY_BLOCKS: usize = 7;
const MAX_POINTERS: usize = 16;
const MEMORY_DIM: usize = 64;
const HIDDEN: usize = 256;
/// Feature map side at the lowest resolution (SIDE / 16).
const FEATURES: usize = 64;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

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
    no_memory: Vec<f32>,
    /// MEMORY_BLOCKS rows of MEMORY_DIM.
    temporal: Vec<f32>,
    conditioning: Option<Memory>,
    /// Newest first.
    recent: VecDeque<Memory>,
}
impl super::Tracker for Engine {
    fn frame(&mut self, rgb: &[u8], prompt: Option<&[Point]>) -> Result<Mask> {
        Engine::frame(self, rgb, prompt)
    }
}
impl Engine {
    pub fn load(dir: &Path) -> Result<Self> {
        let session = |name: &str| session(dir, name);
        Ok(Self {
            encoder: session("vision_encoder")?,
            decoder: session("mask_decoder")?,
            attention: session("memory_attention")?,
            memory_encoder: session("memory_encoder")?,
            no_memory: floats(&dir.join("no_memory_embedding.f32"), HIDDEN)?,
            temporal: floats(
                &dir.join("memory_temporal_positional_encoding.f32"),
                MEMORY_BLOCKS * MEMORY_DIM,
            )?,
            conditioning: None,
            recent: VecDeque::new(),
        })
    }
    /// Segments a SIDE² sRGB frame. A prompt makes it the conditioning frame of a new track.
    pub fn frame(&mut self, rgb: &[u8], prompt: Option<&[Point]>) -> Result<Mask> {
        let pixels = pixels(rgb, MEAN, STD);
        let encoded = self
            .encoder
            .run(ort::inputs!["pixel_values" => Tensor::from_array(pixels).map_err(ort_error)?])
            .map_err(ort_error)?;
        let get = |name: &str| -> Result<ArrayD<f32>> {
            Ok(encoded[name]
                .try_extract_array::<f32>()
                .map_err(ort_error)?
                .to_owned())
        };
        let (f0, f1, f2, pos2) = (get("feats0")?, get("feats1")?, get("feats2")?, get("pos2")?);
        drop(encoded);
        let (points, labels) = prompt_tensors(prompt)?;
        let conditioned = match prompt {
            Some(points) => {
                crate::validate_points(points)?;
                self.conditioning = None;
                self.recent.clear();
                // Without memory, SAM 2 adds the learned no-memory embedding instead.
                let mut features = f2.clone();
                for ((_, c, _, _), v) in features
                    .view_mut()
                    .into_dimensionality::<ndarray::Ix4>()
                    .map_err(shape_error)?
                    .indexed_iter_mut()
                {
                    *v += self.no_memory[c];
                }
                features
            }
            None => self.attend(&f2, &pos2)?,
        };
        let decoded = self
            .decoder
            .run(ort::inputs![
                "feats0" => Tensor::from_array(f0).map_err(ort_error)?,
                "feats1" => Tensor::from_array(f1).map_err(ort_error)?,
                "feats2" => Tensor::from_array(conditioned).map_err(ort_error)?,
                "input_points" => Tensor::from_array(points).map_err(ort_error)?,
                "input_labels" => Tensor::from_array(labels).map_err(ort_error)?
            ])
            .map_err(ort_error)?;
        let masks = decoded["high_res_masks"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .to_owned();
        if masks.shape() != [1, 1, SIDE, SIDE] {
            return Err(format!("unexpected mask shape {:?}", masks.shape()).into());
        }
        let pointer: Vec<f32> = decoded["object_pointer"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .iter()
            .copied()
            .collect();
        let score = decoded["object_score_logits"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .iter()
            .copied()
            .next()
            .unwrap_or(f32::NEG_INFINITY);
        drop(decoded);
        if pointer.len() != HIDDEN {
            return Err("unexpected object pointer size".into());
        }
        let encoded = self
            .memory_encoder
            .run(ort::inputs![
                "vision_features" => Tensor::from_array(f2).map_err(ort_error)?,
                "pred_masks_high_res" => Tensor::from_array(masks.clone()).map_err(ort_error)?,
                "is_mask_from_pts" => Tensor::from_array(Array1::from_elem(1, prompt.is_some())).map_err(ort_error)?
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
            pos: flat("memory_pos_enc")?,
            pointer,
        };
        drop(encoded);
        if memory.tokens.len() != memory.pos.len()
            || !memory.tokens.len().is_multiple_of(MEMORY_DIM)
        {
            return Err("unexpected memory size".into());
        }
        if prompt.is_some() {
            self.conditioning = Some(memory);
        } else {
            self.recent.push_front(memory);
            self.recent.truncate(MAX_POINTERS);
        }
        Ok(Mask {
            logits: masks.index_axis(Axis(0), 0).iter().copied().collect(),
            score,
        })
    }
    /// Conditions the frame's lowest-resolution features on the memory bank.
    fn attend(&mut self, f2: &ArrayD<f32>, pos2: &ArrayD<f32>) -> Result<ArrayD<f32>> {
        let conditioning = self
            .conditioning
            .as_ref()
            .ok_or("track a subject from a prompted frame first")?;
        // Upstream order: the conditioning frame (temporal row 6), then recent frames oldest
        // first; a frame k steps back uses row k - 1.
        let mut blocks = vec![(conditioning, MEMORY_BLOCKS - 1)];
        for k in (1..=self.recent.len().min(MEMORY_BLOCKS - 1)).rev() {
            blocks.push((&self.recent[k - 1], k - 1));
        }
        let (mut spatial, mut spatial_pos) = (Vec::new(), Vec::new());
        for (memory, row) in blocks {
            spatial.extend_from_slice(&memory.tokens);
            let encoding = &self.temporal[row * MEMORY_DIM..(row + 1) * MEMORY_DIM];
            spatial_pos.extend(
                memory
                    .pos
                    .iter()
                    .enumerate()
                    .map(|(i, v)| v + encoding[i % MEMORY_DIM]),
            );
        }
        // Each 256-wide pointer becomes four 64-wide tokens; EdgeTAM gives them no position.
        let mut pointers = conditioning.pointer.clone();
        for memory in self.recent.iter().take(MAX_POINTERS - 1) {
            pointers.extend_from_slice(&memory.pointer);
        }
        let (spatial_len, pointer_len) = (spatial.len() / MEMORY_DIM, pointers.len() / MEMORY_DIM);
        let tensor = |data: Vec<f32>, len: usize| -> Result<Tensor<f32>> {
            Tensor::from_array(
                Array::from_shape_vec((len, 1, MEMORY_DIM), data).map_err(shape_error)?,
            )
            .map_err(ort_error)
        };
        let out = self
            .attention
            .run(ort::inputs![
                "current_vision_features" => Tensor::from_array(tokens(f2)?).map_err(ort_error)?,
                "current_vision_position_embeddings" => Tensor::from_array(tokens(pos2)?).map_err(ort_error)?,
                "spatial_memory" => tensor(spatial, spatial_len)?,
                "spatial_memory_position_embeddings" => tensor(spatial_pos, spatial_len)?,
                "pointer_memory" => tensor(pointers, pointer_len)?,
                "pointer_memory_position_embeddings" => tensor(vec![0.0; pointer_len * MEMORY_DIM], pointer_len)?
            ])
            .map_err(ort_error)?;
        let conditioned = out["conditioned_features"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .to_owned();
        let hw = FEATURES * FEATURES;
        conditioned
            .into_shape_with_order((hw, HIDDEN))
            .map_err(shape_error)?
            .t()
            .as_standard_layout()
            .into_owned()
            .into_shape_with_order((1, HIDDEN, FEATURES, FEATURES))
            .map(|a| a.into_dyn())
            .map_err(shape_error)
    }
}
