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
    Model,
}
pub enum Completed {
    /// An eyedropper sample, to apply to the draft colour it was taken for.
    Picked(crate::eyedropper::Sampled),
    Tracked(crate::tracking::Tracked),
    PlanarTracked(crate::planar::Tracked),
    CameraSolved(Box<crate::solve_camera::Solved>),
    ModelChosen(Box<crate::model_import::Chosen>),
    Segmented(Box<crate::subject::Segmented>),
    PresetLibrary(veylvik_io::presets::Listing),
    PresetLoaded(Box<veylvik_document::presets::Preset>),
    Cancelled,
    /// The finished result of a live MCP job, reported through `inspect.job`.
    Live(&'static str, serde_json::Value),
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
/// The main window's handle for the next file dialog, so the desktop shows the
/// dialog as a child of the window (floating over it at its own size) and not as an
/// unrelated window, which a compositor may tile or make fullscreen.
static PARENT: std::sync::Mutex<Option<ashpd::WindowIdentifier>> = std::sync::Mutex::new(None);
static NO_PARENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Keep a handle ready; called from the UI thread, which owns the window. Each
/// dialog uses one up, and the next frame makes another. A desktop that cannot give
/// one is asked only once.
pub fn offer_parent(frame: &eframe::Frame) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    if NO_PARENT.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Ok(mut slot) = PARENT.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    let (Ok(window), Ok(display)) = (frame.window_handle(), frame.display_handle()) else {
        NO_PARENT.store(true, std::sync::atomic::Ordering::Relaxed);
        return;
    };
    *slot = pollster::block_on(ashpd::WindowIdentifier::from_raw_handle(
        &window.as_raw(),
        Some(&display.as_raw()),
    ));
    if slot.is_none() {
        NO_PARENT.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
fn parent() -> Option<ashpd::WindowIdentifier> {
    PARENT.lock().ok()?.take()
}
/// Where dialogs begin when the project has no folder of its own: projects, exports.
static DEFAULTS: std::sync::Mutex<(Option<PathBuf>, Option<PathBuf>)> =
    std::sync::Mutex::new((None, None));
/// Set the folders from preferences; a path that is not a folder is ignored.
pub fn set_default_folders(projects: &str, exports: &str) {
    let folder = |text: &str| {
        Some(PathBuf::from(text.trim())).filter(|p| !text.trim().is_empty() && p.is_dir())
    };
    if let Ok(mut defaults) = DEFAULTS.lock() {
        *defaults = (folder(projects), folder(exports));
    }
}
/// The folder a dialog for `choice` begins in, given the one the project offers.
pub(crate) fn starting_folder(
    choice: &Choice,
    offered: &Path,
    started_in: Option<&Path>,
    defaults: &(Option<PathBuf>, Option<PathBuf>),
) -> PathBuf {
    let exporting = matches!(choice, Choice::Export | Choice::Video | Choice::Folder);
    if exporting && let Some(exports) = &defaults.1 {
        return exports.clone();
    }
    // A project without a place of its own offers the folder Studio was started in.
    if started_in == Some(offered)
        && let Some(projects) = &defaults.0
    {
        return projects.clone();
    }
    offered.to_owned()
}
pub fn select(choice: Choice, folder: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let begin = {
        let defaults = DEFAULTS.lock().map(|d| d.clone()).unwrap_or_default();
        starting_folder(
            &choice,
            folder,
            std::env::current_dir().ok().as_deref(),
            &defaults,
        )
    };
    let folder = begin.as_path();
    let (title, label, pattern) = match choice {
        Choice::PresetOpen => ("Import effect preset", "Veylvik effect", "*.veyfx"),
        Choice::PresetSave => ("Export effect preset", "Veylvik effect", "*.veyfx"),
        Choice::Open => ("Open composition", "Veylvik composition", "*.veylvik"),
        Choice::Save => ("Save composition", "Veylvik composition", "*.veylvik"),
        Choice::Font => ("Choose font", "Font", "*.ttf"),
        Choice::Lut => ("Choose a 3D LUT", "Cube LUT", "*.cube"),
        Choice::Model => ("Choose a 3D model", "glTF model", "*.glb"),
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
    } else if matches!(choice, Choice::Model) {
        FileFilter::new(label).glob("*.glb").glob("*.gltf")
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
                | Choice::Model
        ) {
            SelectedFiles::open_file()
                .identifier(parent())
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
                .identifier(parent())
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

#[cfg(test)]
mod folder_tests {
    use super::*;
    #[test]
    fn dialogs_begin_in_the_chosen_folders_only_when_the_project_has_none() {
        let (cwd, project) = (Path::new("/home/me"), Path::new("/work/film"));
        let set = (
            Some(PathBuf::from("/projects")),
            Some(PathBuf::from("/renders")),
        );
        let none = (None, None);
        // Nothing set: always the folder offered.
        assert_eq!(starting_folder(&Choice::Open, cwd, Some(cwd), &none), cwd);
        assert_eq!(
            starting_folder(&Choice::Video, project, Some(cwd), &none),
            project
        );
        // An unsaved project offers the folder Studio was started in: use the projects folder.
        assert_eq!(
            starting_folder(&Choice::Open, cwd, Some(cwd), &set),
            Path::new("/projects")
        );
        assert_eq!(
            starting_folder(&Choice::Save, cwd, Some(cwd), &set),
            Path::new("/projects")
        );
        assert_eq!(
            starting_folder(&Choice::Import, cwd, Some(cwd), &set),
            Path::new("/projects")
        );
        // A project with a place of its own keeps it for opening, saving and importing.
        assert_eq!(
            starting_folder(&Choice::Save, project, Some(cwd), &set),
            project
        );
        assert_eq!(
            starting_folder(&Choice::Import, project, Some(cwd), &set),
            project
        );
        // Exports go to the exports folder whenever one is set.
        for choice in [Choice::Export, Choice::Video, Choice::Folder] {
            assert_eq!(
                starting_folder(&choice, project, Some(cwd), &set),
                Path::new("/renders")
            );
        }
        let only_projects = (Some(PathBuf::from("/projects")), None);
        assert_eq!(
            starting_folder(&Choice::Video, project, Some(cwd), &only_projects),
            project
        );
    }
}
