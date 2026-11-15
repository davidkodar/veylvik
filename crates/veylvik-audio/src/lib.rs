//! Audio preview: the composition's sound mixed exactly like export, then played from any
//! composition time through the default output device. No GPU and no UI.
mod mix;
mod player;
pub use mix::{MAX_SECONDS, Pcm, mix};
pub use player::{Device, Player, backend, devices};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
