//! One background file operation at a time; file selection uses the desktop portal.
use crate::project::{Project, Saved};
use ashpd::desktop::{
    ResponseError,
    file_chooser::{FileFilter, SelectedFiles},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread::JoinHandle,
};
use veylvik_render::Image;
#[derive(Clone, Copy)]
pub enum Choice {
    Open,
    Import,
    Save,
    Export,
    Folder,
    Font,
}
pub enum Completed {
    Cancelled,
    Outlined(Box<veylvik_document::Node>),
    Opened(Box<Project>),
    Imported { path: PathBuf, image: Arc<Image> },
    Saved(Saved),
    Exported(PathBuf),
    Sequence(serde_json::Value),
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
        Choice::Open => ("Open composition", "Veylvik composition", "*.veylvik"),
        Choice::Save => ("Save composition", "Veylvik composition", "*.veylvik"),
        Choice::Font => ("Choose font for text outlines", "Font", "*.ttf"),
        Choice::Import => ("Import image", "PNG image", "*.png"),
        Choice::Export => (
            "Export frame (.png or .exr)",
            "PNG or OpenEXR image",
            "*.png",
        ),
        Choice::Folder => ("Choose parent folder for image sequence", "Folders", "*"),
    };
    let filter = if matches!(choice, Choice::Font) {
        FileFilter::new(label)
            .glob("*.ttf")
            .glob("*.otf")
            .glob("*.ttc")
    } else if matches!(choice, Choice::Export) {
        FileFilter::new(label).glob("*.png").glob("*.exr")
    } else {
        FileFilter::new(label).glob(pattern)
    };
    let result: ashpd::Result<SelectedFiles> = pollster::block_on(async {
        if matches!(
            choice,
            Choice::Open | Choice::Import | Choice::Folder | Choice::Font
        ) {
            SelectedFiles::open_file()
                .title(title)
                .modal(true)
                .multiple(false)
                .directory(matches!(choice, Choice::Folder))
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
