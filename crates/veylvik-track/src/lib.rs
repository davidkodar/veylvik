//! Footage analysis on the GPU: point and planar tracking built only on `veylvik_render::compute`.
pub mod estimate;
pub mod planar;
pub mod point;
mod refine;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Render(#[from] veylvik_render::Error),
    #[error(transparent)]
    Document(#[from] veylvik_document::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
fn error(message: impl ToString) -> Error {
    Error::Message(message.to_string())
}
