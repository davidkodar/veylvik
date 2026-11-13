//! ViTMatte on ONNX Runtime CUDA: refines alpha inside a trimap's unknown band from the image,
//! for hair and soft edges. Inputs are padded to a multiple of 32, as the model requires.
use crate::Result;
use ndarray::Array4;
use ort::{session::Session, value::Tensor};
use std::path::Path;

fn ort_error(e: ort::Error) -> crate::Error {
    crate::Error::Message(format!("matting runtime: {e}"))
}
pub struct Matting {
    session: Session,
}
impl Matting {
    pub fn load(dir: &Path) -> Result<Self> {
        ort::init().commit();
        let session = Session::builder()
            .map_err(ort_error)?
            .with_execution_providers([ort::execution_providers::CUDAExecutionProvider::default()
                .build()
                .error_on_failure()])
            .map_err(|e| ort_error(e.into()))?
            .commit_from_file(dir.join("model.onnx"))
            .map_err(|e| {
                crate::Error::Message(format!(
                    "cannot load the matting model on CUDA (no CPU fallback): {e}"
                ))
            })?;
        Ok(Self { session })
    }
    /// `pixels` holds `width` × `height` RGBA bytes: sRGB colour, and the trimap in alpha
    /// (0 background, 128 unknown, 255 subject). Returns alpha in 0..1, row-major.
    pub fn matte(&mut self, pixels: &[u8], width: usize, height: usize) -> Result<Vec<f32>> {
        if width == 0 || height == 0 || pixels.len() != width * height * 4 {
            return Err("matting input does not match its size".into());
        }
        let (pw, ph) = (width.div_ceil(32) * 32, height.div_ceil(32) * 32);
        // Colour normalised to −1..1, trimap rescaled to 0..1; padding stays zero.
        let input = Array4::from_shape_fn((1, 4, ph, pw), |(_, c, y, x)| {
            if x >= width || y >= height {
                return 0.0;
            }
            let v = f32::from(pixels[(y * width + x) * 4 + c]) / 255.0;
            if c < 3 { (v - 0.5) / 0.5 } else { v }
        });
        let out = self
            .session
            .run(ort::inputs!["pixel_values" => Tensor::from_array(input).map_err(ort_error)?])
            .map_err(ort_error)?;
        let alphas = out["alphas"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?;
        if alphas.shape() != [1, 1, ph, pw] {
            return Err(format!("unexpected matting output {:?}", alphas.shape()).into());
        }
        let mut alpha = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                alpha.push(alphas[[0, 0, y, x]].clamp(0.0, 1.0));
            }
        }
        Ok(alpha)
    }
}
