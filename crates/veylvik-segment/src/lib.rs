//! Subject segmentation: click-prompted, memory-tracked mattes from local models that run in an
//! isolated worker process, so a GPU or runtime fault can never take down the host.
pub mod analysis;
#[cfg(feature = "engine")]
mod edgetam;
pub mod matte;
pub mod models;
pub mod protocol;
pub mod worker;
use serde::{Deserialize, Serialize};

/// Side of the square model input; frames are resized to it without keeping the aspect ratio,
/// as the SAM 2 family was trained.
pub const SIDE: usize = 1024;
/// Bytes of one model input frame: SIDE² sRGB triples.
pub const FRAME_BYTES: usize = SIDE * SIDE * 3;

/// A click in frame coordinates (0..1 from the top left); `positive` adds the area, otherwise it
/// is excluded.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub positive: bool,
}
/// Most clicks a prompt may carry.
pub const MAX_POINTS: usize = 32;
/// Checks a prompt: 1..=MAX_POINTS finite points inside the frame, at least one positive.
pub fn validate_points(points: &[Point]) -> Result<()> {
    if points.is_empty() || points.len() > MAX_POINTS {
        return Err(format!("give 1 to {MAX_POINTS} points").into());
    }
    if points
        .iter()
        .any(|p| !(0.0..=1.0).contains(&p.x) || !(0.0..=1.0).contains(&p.y))
    {
        return Err("points must lie inside the frame (0..1)".into());
    }
    if !points.iter().any(|p| p.positive) {
        return Err("at least one point must mark the subject".into());
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Message(message.to_owned())
    }
}
pub type Result<T> = std::result::Result<T, Error>;
