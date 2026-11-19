//! A project that stands for a clip in a host editor: Studio says so beside the pages and sends the picture back on one click, saving first; the reading of the link and the render both run off the UI thread, one at a time.
use crate::Studio;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use veylvik_io::link::{self, Link, Sent};

enum Message {
    Progress(u32, u32),
    Done(Result<Sent, String>),
}
#[derive(Default)]
pub(crate) struct HostLink {
    /// The project the link below was read for.
    read_for: Option<PathBuf>,
    reading: Option<mpsc::Receiver<Result<Option<Link>, String>>>,
    link: Option<Link>,
    /// Send as soon as the project is saved.
    wanted: bool,
    sending: Option<mpsc::Receiver<Message>>,
    progress: (u32, u32),
    cancel: Arc<AtomicBool>,
}
impl HostLink {
    /// The host's name, when the open project is linked to one.
    pub(crate) fn host(&self) -> Option<&str> {
        self.link.as_ref().map(|l| l.host.as_str())
    }
}
impl Studio {
    /// Once a frame: read the link of a newly opened project, start a send that waited
    /// for its save, and take in a running send's progress.
    pub(crate) fn link_tick(&mut self, ctx: &egui::Context) {
        if self.host_link.read_for != self.project.path {
            self.host_link.read_for = self.project.path.clone();
            self.host_link.link = None;
            self.host_link.reading = self.project.path.clone().map(|path| {
                let (tx, rx) = mpsc::channel();
                let wake = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(link::load(&path).map_err(|e| e.to_string()));
                    wake.request_repaint();
                });
                rx
            });
        }
        if let Some(read) = self
            .host_link
            .reading
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.host_link.reading = None;
            match read {
                Ok(found) => {
                    if let Some(found) = &found {
                        // Arrive on the frame the host was showing.
                        let last = i64::from(self.project.document.snapshot().frame_count) - 1;
                        self.project.frame = (i64::from(found.before)
                            + i64::from(found.playhead.unwrap_or(0)))
                        .clamp(0, last.max(0));
                        self.status = format!(
                            "Linked to {}: this is \u{201c}{}\u{201d} from its timeline",
                            found.host, found.clip
                        );
                        self.error = false;
                        self.request_preview();
                    }
                    self.host_link.link = found;
                }
                Err(error) => self.fail(error),
            }
        }
        if self.host_link.wanted
            && self.host_link.sending.is_none()
            && !self.files.busy()
            && !self.project.dirty()
            && let Some(path) = self.project.path.clone()
        {
            self.host_link.wanted = false;
            self.host_link.progress = (0, 0);
            let cancel = Arc::new(AtomicBool::new(false));
            self.host_link.cancel = cancel.clone();
            let (tx, rx) = mpsc::channel();
            let wake = ctx.clone();
            std::thread::spawn(move || {
                let report = tx.clone();
                let waker = wake.clone();
                let sent = link::send(&path, None, move |p| {
                    let _ = report.send(Message::Progress(p.completed, p.total));
                    waker.request_repaint();
                    !cancel.load(Ordering::Relaxed)
                })
                .map_err(|e| e.to_string());
                let _ = tx.send(Message::Done(sent));
                wake.request_repaint();
            });
            self.host_link.sending = Some(rx);
        }
        let mut done = None;
        if let Some(rx) = &self.host_link.sending {
            for message in rx.try_iter() {
                match message {
                    Message::Progress(at, of) => self.host_link.progress = (at, of),
                    Message::Done(result) => done = Some(result),
                }
            }
        }
        if let Some(result) = done {
            self.host_link.sending = None;
            let host = self.host_link.host().unwrap_or("the editor").to_owned();
            match result {
                Ok(sent) => {
                    self.status = match sent.note {
                        Some(note) => format!("Sent {} frames. {note}", sent.frames),
                        None => format!("{host} has the new picture ({} frames)", sent.frames),
                    };
                    self.error = false;
                }
                Err(error) => self.fail(error),
            }
        }
    }
    /// Send the picture to the host: the saved project is what is rendered, so unsaved
    /// work is saved first and the send follows it.
    pub(crate) fn update_host(&mut self) {
        if self.host_link.link.is_none() {
            self.fail("This project is not linked to an editor");
            return;
        }
        if self.host_link.sending.is_some() {
            return;
        }
        self.host_link.wanted = true;
        if self.project.dirty() || self.draft_dirty {
            self.save_project(false);
        }
    }
    /// Beside the pages: what the project is linked to, and the way back.
    pub(crate) fn link_bar(&mut self, ui: &mut egui::Ui) {
        let Some(found) = &self.host_link.link else {
            return;
        };
        let (host, clip) = (found.host.clone(), found.clip.clone());
        ui.separator();
        ui.label(
            egui::RichText::new(format!("Linked to {host}"))
                .color(crate::design::accent(ui.ctx()))
                .strong(),
        )
        .on_hover_text(format!(
            "This project is the clip \u{201c}{clip}\u{201d} on {host}'s timeline. Its markers show where the clip starts and ends; the frames outside them are handles for trimming."
        ));
        if self.host_link.sending.is_some() {
            let (at, of) = self.host_link.progress;
            ui.add(
                egui::ProgressBar::new(if of > 0 { at as f32 / of as f32 } else { 0.0 })
                    .desired_width(150.0)
                    .text(format!("Sending {at} of {of}")),
            );
            if ui.button("Cancel").clicked() {
                self.host_link.cancel.store(true, Ordering::Relaxed);
            }
        } else if crate::design::primary(ui, &format!("Update in {host}"))
            .on_hover_text(format!(
                "Save, render this composition and show it on {host}'s timeline in place of the clip"
            ))
            .clicked()
        {
            self.update_host();
        }
    }
}
