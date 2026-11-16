//! One background file operation at a time; file selection uses the desktop portal.
use crate::project::{Project, Saved};
use ashpd::desktop::{
    ResponseError,
    file_chooser::{FileFilter, SelectedFiles},
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
};
use veylvik_io::footage::Media;
#[derive(Clone, Copy)]
pub enum Choice {
    Open,
    PresetOpen,
    PresetSave,
    Import,
    ImportSequence,
    Save,
    Export,
    Video,
    Folder,
    Directory,
    Source,
    Font,
    Lut,
}
pub enum Completed {
    /// An eyedropper sample, to apply to the draft colour it was taken for.
    Picked(crate::eyedropper::Sampled),
    Tracked(crate::tracking::Tracked),
    PlanarTracked(crate::planar::Tracked),
    CameraSolved(Box<crate::solve_camera::Solved>),
    Segmented(Box<crate::subject::Segmented>),
    PresetLibrary(veylvik_io::presets::Listing),
    PresetLoaded(Box<veylvik_document::presets::Preset>),
    Cancelled,
    FontAdded(String),
    Outlined(Box<veylvik_document::Node>),
    TextFont {
        session: veylvik_document::NodeId,
        revision: u64,
        node: Box<veylvik_document::Node>,
    },
    Opened(Box<Project>),
    NewFromMedia {
        session: veylvik_document::NodeId,
        revision: u64,
        project: Box<Project>,
    },
    Imported {
        session: veylvik_document::NodeId,
        revision: u64,
        position: veylvik_document::NodePosition,
        path: PathBuf,
        image: Media,
        /// Whether an empty composition adopts the media's settings; `None` asks the user.
        matching: Option<bool>,
    },
    Reloaded {
        session: veylvik_document::NodeId,
        revision: u64,
        inputs: veylvik_render::ImageInputs,
    },
    Replaced {
        session: veylvik_document::NodeId,
        revision: u64,
        node: veylvik_document::NodeId,
        path: PathBuf,
        image: Media,
    },
    Saved(Saved),
    Exported(PathBuf),
    Portable(Box<crate::portable::Finished>),
    Queued(Box<crate::render_queue::Job>),
}
pub struct Tasks {
    sender: mpsc::SyncSender<Result<Completed, String>>,
    receiver: mpsc::Receiver<Result<Completed, String>>,
    thread: Option<JoinHandle<()>>,
    context: eframe::egui::Context,
}
impl Tasks {
    pub fn new(context: eframe::egui::Context) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        Self {
            sender,
            receiver,
            thread: None,
            context,
        }
    }
    pub fn busy(&self) -> bool {
        self.thread.is_some()
    }
    pub fn start(
        &mut self,
        work: impl FnOnce() -> Result<Completed, String> + Send + 'static,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("A file operation is already in progress".into());
        }
        let sender = self.sender.clone();
        let context = self.context.clone();
        let thread = std::thread::Builder::new()
            .name("veylvik-files".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| Err("File worker failed unexpectedly".into()));
                let _ = sender.send(result);
                context.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        self.thread = Some(thread);
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Result<Completed, String>> {
        let result = self.receiver.try_recv().ok()?;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Some(result)
    }
}
pub fn select(choice: Choice, folder: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let (title, label, pattern) = match choice {
        Choice::PresetOpen => ("Import effect preset", "Veylvik effect", "*.veyfx"),
        Choice::PresetSave => ("Export effect preset", "Veylvik effect", "*.veyfx"),
        Choice::Open => ("Open composition", "Veylvik composition", "*.veylvik"),
        Choice::Save => ("Save composition", "Veylvik composition", "*.veylvik"),
        Choice::Font => ("Choose font", "Font", "*.ttf"),
        Choice::Lut => ("Choose a 3D LUT", "Cube LUT", "*.cube"),
        Choice::ImportSequence => (
            "Choose the first numbered frame",
            "PNG or OpenEXR sequence",
            "*.png",
        ),
        Choice::Import => ("Import media", "Video, images and image sequences", "*.png"),
        Choice::Export => (
            "Export frame (.png or .exr)",
            "PNG or OpenEXR image",
            "*.png",
        ),
        Choice::Video => ("Export video", "MP4 video", "*.mp4"),
        Choice::Directory => ("Choose folder", "Folders", "*"),
        Choice::Source => ("Locate source", "Source files", "*"),
        Choice::Folder => ("Choose parent folder for image sequence", "Folders", "*"),
    };
    let filter = if matches!(choice, Choice::Font) {
        FileFilter::new(label)
            .glob("*.ttf")
            .glob("*.otf")
            .glob("*.ttc")
    } else if matches!(choice, Choice::Import) {
        FileFilter::new(label)
            .glob("*.png")
            .glob("*.exr")
            .glob("*.mp4")
            .glob("*.mov")
            .glob("*.m4v")
            .glob("*.mkv")
            .glob("*.webm")
            .glob("*.avi")
            .glob("*.mpg")
            .glob("*.mpeg")
            .glob("*.veyseq")
            .glob("sequence.json")
    } else if matches!(choice, Choice::Export | Choice::ImportSequence) {
        FileFilter::new(label).glob("*.png").glob("*.exr")
    } else {
        FileFilter::new(label).glob(pattern)
    };
    let result: ashpd::Result<SelectedFiles> = pollster::block_on(async {
        if matches!(
            choice,
            Choice::Open
                | Choice::PresetOpen
                | Choice::Import
                | Choice::Folder
                | Choice::Directory
                | Choice::Source
                | Choice::Font
                | Choice::Lut
        ) {
            SelectedFiles::open_file()
                .title(title)
                .modal(true)
                .multiple(false)
                .directory(matches!(choice, Choice::Folder | Choice::Directory))
                .current_folder(folder)?
                .filter(filter)
                .send()
                .await?
                .response()
        } else {
            SelectedFiles::save_file()
                .title(title)
                .modal(true)
                .current_name(name)
                .current_folder(folder)?
                .filter(filter)
                .send()
                .await?
                .response()
        }
    });
    match result {
        Ok(files) => {
            let uri = files
                .uris()
                .first()
                .ok_or("Desktop portal returned no file")?;
            Ok(Some(
                uri.to_file_path()
                    .map_err(|_| "Only local file paths are supported")?,
            ))
        }
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
        Err(error) => Err(format!("Desktop file portal: {error}")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_jobs_are_bounded_and_report_worker_failure() {
        let mut tasks = Tasks::new(eframe::egui::Context::default());
        let (tx, rx) = mpsc::channel();
        tasks
            .start(move || {
                rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                Err("test failure".into())
            })
            .unwrap();
        assert!(tasks.start(|| Ok(Completed::Cancelled)).is_err());
        tx.send(()).unwrap();
        let result = tasks
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(matches!(result, Err(message) if message == "test failure"));
        tasks.thread.take().unwrap().join().unwrap();
        assert!(!tasks.busy());
    }
}
