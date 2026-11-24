//! One background file operation at a time; file selection uses the desktop portal.
use ashpd::desktop::{
    ResponseError,
    file_chooser::{FileFilter, SelectedFiles},
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
};
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
    /// A Gaussian splat .ply file (asked of a host as a model).
    Splats,
    /// A Nuke script to rebuild as a new composition (only with project files).
    Nuke,
}
/// What a finished file job does on the UI thread: its own step, run only while the
/// project it was started for is still open (and, if it says so, unchanged), so each
/// feature keeps its whole flow and the "is it still this project?" rule lives here once.
pub struct Completed {
    then: Then,
    guard: Option<Guard>,
}
type Then = Box<dyn FnOnce(&mut crate::Studio, &eframe::egui::Context) + Send>;
/// The project a result belongs to: a session, and a revision when any edit makes the
/// result wrong. `live` names a live MCP job to tell when the result is thrown away.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Guard {
    session: veylvik_document::NodeId,
    revision: Option<u64>,
    stale: &'static str,
    live: Option<&'static str>,
}
impl Guard {
    fn holds(&self, session: veylvik_document::NodeId, revision: u64) -> bool {
        self.session == session && self.revision.is_none_or(|r| r == revision)
    }
}
impl Completed {
    pub fn then(
        then: impl FnOnce(&mut crate::Studio, &eframe::egui::Context) + Send + 'static,
    ) -> Self {
        Self {
            then: Box::new(then),
            guard: None,
        }
    }
    /// The person closed the dialog: nothing happens.
    pub fn cancelled() -> Self {
        Self::then(|studio, _| {
            studio.after_save = None;
            studio.status = "Cancelled".into();
            studio.error = false;
        })
    }
    /// Run only while `session` is still open and at `revision`; else say `stale`.
    pub fn unless_changed(
        mut self,
        session: veylvik_document::NodeId,
        revision: u64,
        stale: &'static str,
    ) -> Self {
        self.guard = Some(Guard {
            session,
            revision: Some(revision),
            stale,
            live: None,
        });
        self
    }
    /// Run only while `session` is still open, whatever was edited since; else say `stale`.
    pub fn in_session(mut self, session: veylvik_document::NodeId, stale: &'static str) -> Self {
        self.guard = Some(Guard {
            session,
            revision: None,
            stale,
            live: None,
        });
        self
    }
    /// A live MCP job waits on this result: tell it when the result is thrown away.
    pub fn for_live(mut self, tool: &'static str) -> Self {
        if let Some(guard) = &mut self.guard {
            guard.live = Some(tool);
        }
        self
    }
    pub(crate) fn finish(self, studio: &mut crate::Studio, ctx: &eframe::egui::Context) {
        if let Some(guard) = self.guard
            && !guard.holds(studio.project.session, studio.project.document.revision())
        {
            if !guard.stale.is_empty() {
                studio.fail(guard.stale);
            }
            if let Some(tool) = guard.live {
                studio.finish_live_job(tool, Err(studio.status.clone()));
            }
            return;
        }
        (self.then)(studio, ctx)
    }
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
static HOST: std::sync::RwLock<Option<std::sync::Arc<dyn crate::host::Host>>> =
    std::sync::RwLock::new(None);
/// Whoever shows the editor answers for files from now on; `None` gives them back to the
/// system's own dialog.
pub fn set_host(host: Option<std::sync::Arc<dyn crate::host::Host>>) {
    if let Ok(mut slot) = HOST.write() {
        *slot = host;
    }
}
pub(crate) fn host() -> Option<std::sync::Arc<dyn crate::host::Host>> {
    HOST.read().ok().and_then(|slot| slot.clone())
}
type Named = (String, Option<PathBuf>);
static LABELS: std::sync::Mutex<std::collections::BTreeMap<String, Named>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
/// The name the host gave media it handed out under `key`.
pub fn host_label(key: &str) -> Option<String> {
    LABELS
        .lock()
        .ok()
        .and_then(|labels| labels.get(key).map(|named| named.0.clone()))
}
/// The file the host named for media it handed out under `key`.
pub fn host_file(key: &str) -> Option<PathBuf> {
    LABELS
        .lock()
        .ok()
        .and_then(|labels| labels.get(key).and_then(|named| named.1.clone()))
}
/// The frames of a media key, when a host is there and knows it.
pub fn host_media(key: &str) -> Option<std::sync::Arc<dyn veylvik_render::FrameSource>> {
    host().and_then(|host| host.media(key))
}
impl From<&Choice> for crate::host::Purpose {
    fn from(choice: &Choice) -> Self {
        use crate::host::Purpose;
        match choice {
            Choice::Open => Purpose::Open,
            Choice::PresetOpen => Purpose::PresetOpen,
            Choice::PresetSave => Purpose::PresetSave,
            Choice::Import => Purpose::Import,
            Choice::ImportSequence => Purpose::ImportSequence,
            Choice::Save => Purpose::Save,
            Choice::Export => Purpose::Export,
            Choice::Video => Purpose::Video,
            Choice::Folder => Purpose::Folder,
            Choice::Directory => Purpose::Directory,
            Choice::Source => Purpose::Source,
            Choice::Font => Purpose::Font,
            Choice::Lut => Purpose::Lut,
            Choice::Model | Choice::Splats => Purpose::Model,
            Choice::Nuke => Purpose::Open,
        }
    }
}
/// What the user chose: a file, or media the host keeps itself.
pub enum Chosen {
    Path(PathBuf),
    Media {
        key: String,
        label: String,
        source: std::sync::Arc<dyn veylvik_render::FrameSource>,
    },
}
/// Ask for a file where the answer may also be the host's own media.
pub fn pick(choice: Choice, folder: &Path, name: &str) -> Result<Option<Chosen>, String> {
    let Some(host) = host() else {
        return Ok(select(choice, folder, name)?.map(Chosen::Path));
    };
    let request = crate::host::Pick {
        purpose: (&choice).into(),
        folder: folder.to_owned(),
        name: name.to_owned(),
    };
    Ok(match host.pick(&request)? {
        None => None,
        Some(crate::host::Picked::File(path)) => Some(Chosen::Path(path)),
        Some(crate::host::Picked::Media { key, label, file }) => {
            let source = host
                .media(&key)
                .ok_or_else(|| format!("the host named media it does not have: {key}"))?;
            if let Ok(mut labels) = LABELS.lock() {
                labels.insert(key.clone(), (label.clone(), file));
            }
            Some(Chosen::Media { key, label, source })
        }
    })
}
pub fn select(choice: Choice, folder: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    if host().is_some() {
        // Only a file on disk will do here.
        return match pick(choice, folder, name)? {
            None => Ok(None),
            Some(Chosen::Path(path)) => Ok(Some(path)),
            Some(Chosen::Media { label, .. }) => Err(format!(
                "{label} is kept by the host, and a file on disk is needed here"
            )),
        };
    }
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
        Choice::Nuke => ("Import a Nuke script", "Nuke script", "*.nk"),
        Choice::Splats => (
            "Choose Gaussian splats",
            "Gaussian splats (.ply or .splat)",
            "*.ply",
        ),
        Choice::ImportSequence => (
            "Choose the first numbered frame",
            "PNG or OpenEXR sequence",
            "*.png",
        ),
        Choice::Import => (
            "Import media",
            "Video, images, sound, SVG drawings and image sequences",
            "*.png",
        ),
        Choice::Export => (
            "Export frame (.png or .exr)",
            "PNG or OpenEXR image",
            "*.png",
        ),
        Choice::Video => ("Export video", "MP4 or MOV video", "*.mp4"),
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
            .glob("*.jpg")
            .glob("*.jpeg")
            .glob("*.tif")
            .glob("*.tiff")
            .glob("*.webp")
            .glob("*.svg")
            .glob("*.wav")
            .glob("*.mp3")
            .glob("*.flac")
            .glob("*.ogg")
            .glob("*.opus")
            .glob("*.m4a")
            .glob("*.aac")
            .glob("*.aiff")
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
    } else if matches!(choice, Choice::Splats) {
        FileFilter::new(label).glob("*.ply").glob("*.splat")
    } else if matches!(choice, Choice::Video) {
        FileFilter::new(label).glob("*.mp4").glob("*.mov")
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
                | Choice::Splats
                | Choice::Nuke
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
    fn a_result_belongs_to_the_project_and_revision_it_was_started_for() {
        let session = veylvik_document::NodeId::from_u128(1);
        let other = veylvik_document::NodeId::from_u128(2);
        let edited = Guard {
            session,
            revision: Some(4),
            stale: "changed",
            live: None,
        };
        assert!(edited.holds(session, 4));
        assert!(!edited.holds(session, 5), "an edit since makes it stale");
        assert!(!edited.holds(other, 4), "another project makes it stale");
        let any = Guard {
            revision: None,
            ..edited
        };
        assert!(any.holds(session, 9) && !any.holds(other, 9));
    }
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
        assert!(tasks.start(|| Ok(Completed::cancelled())).is_err());
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
