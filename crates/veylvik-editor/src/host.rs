//! What the editor asks of whoever shows it. A host answers for the things that are its own (which files the user means, and its own media); Studio, with no host, answers with the system's file dialog and the disk.
use std::{path::PathBuf, sync::Arc};
use veylvik_render::FrameSource;

/// What a file is wanted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Media to add to the composition: video, images, sound, drawings.
    Import,
    /// The first frame of a numbered image sequence.
    ImportSequence,
    /// A file to take the place of a source that is missing.
    Source,
    Font,
    Lut,
    Model,
    PresetOpen,
    PresetSave,
    /// A composition to open, or where to save one.
    Open,
    Save,
    /// Where to write a frame, a video, or a folder of frames.
    Export,
    Video,
    Folder,
    /// Any folder.
    Directory,
}
/// One question to the host: a file, or a folder, for a purpose.
#[derive(Debug, Clone)]
pub struct Pick {
    pub purpose: Purpose,
    /// Where the editor would start looking.
    pub folder: PathBuf,
    /// A name to offer when saving; empty otherwise.
    pub name: String,
}
/// The host's answer.
#[derive(Clone)]
pub enum Picked {
    /// A file or folder on disk.
    File(PathBuf),
    /// Media the host keeps itself, by the host's own name for it. The editor stores the
    /// key in the composition and asks [`Host::media`] for its frames.
    Media { key: String, label: String },
}
pub trait Host: Send + Sync {
    /// Ask the user. Called off the interface thread, so it may wait for them; `None`
    /// means they changed their mind.
    fn pick(&self, request: &Pick) -> Result<Option<Picked>, String>;
    /// The frames of media this host handed out a key for: asked when the key is picked
    /// and whenever a composition that names it is opened.
    fn media(&self, _key: &str) -> Option<Arc<dyn FrameSource>> {
        None
    }
}
