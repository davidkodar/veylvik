//! Interactive preview on top of the renderer's public interface: one worker renders the latest request, and a bounded, disposable cache holds played frames.
mod pictures;
pub mod playback;
mod scheduler;
pub mod screen;
pub use scheduler::{GpuPreviewScheduler, PreviewResult, PreviewScheduler, PreviewTicket};
use veylvik_render::{Error, Result};
fn error(message: impl ToString) -> Error {
    Error::Message(message.to_string())
}
