//! One active and one latest pending recovery write, independent of file dialogs.
use crate::project::Project;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use veylvik_document::{Composition, NodeId};
use veylvik_io::recovery::{Entry, Store};
enum Action {
    Save(Box<Composition>, PathBuf),
    Clear,
}
#[derive(Default)]
struct State {
    pending: Option<Action>,
    stop: bool,
    completed: Option<Result<(), String>>,
}
struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn new(
        store: Store,
        id: NodeId,
        origin: Option<NodeId>,
        ctx: egui::Context,
    ) -> std::io::Result<Self> {
        let lease = store.claim(id)?;
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("veylvik-recovery".into())
            .spawn(move || {
                let _lease = lease;
                loop {
                    let action = {
                        let (lock, wake) = &*state;
                        let mut state = lock.lock().unwrap();
                        while state.pending.is_none() && !state.stop {
                            state = wake.wait(state).unwrap();
                        }
                        match state.pending.take() {
                            Some(action) => action,
                            None => break,
                        }
                    };
                    let result = match action {
                        Action::Save(document, base) => store.write(id, &document, &base),
                        Action::Clear => store.remove(id).and_then(|()| {
                            if let Some(origin) = origin {
                                if store.available(origin)? {
                                    store.remove(origin)
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        }),
                    };
                    state.0.lock().unwrap().completed =
                        Some(result.map_err(|error| error.to_string()));
                    ctx.request_repaint();
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
    fn submit(&self, action: Action) {
        let mut state = self.shared.0.lock().unwrap();
        if !state.stop {
            state.pending = Some(action);
            self.shared.1.notify_one();
        }
    }
    fn take_result(&self) -> Option<Result<(), String>> {
        self.shared.0.lock().unwrap().completed.take()
    }
    fn finish(&mut self, clean: bool) {
        if self.thread.is_none() {
            return;
        }
        {
            let mut state = self.shared.0.lock().unwrap();
            if clean {
                state.pending = Some(Action::Clear);
            }
            state.stop = true;
            self.shared.1.notify_one();
        }
        if self.thread.take().unwrap().join().is_err() {
            self.shared.0.lock().unwrap().completed = Some(Err("recovery worker failed".into()));
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.finish(false);
    }
}
pub struct Recovery {
    pub store: Store,
    pub open: bool,
    entries: Vec<Entry>,
    worker: Option<Worker>,
    id: NodeId,
    origin: Option<NodeId>,
    clean: bool,
    last_revision: Option<u64>,
    last_request: Instant,
    error: Option<String>,
}
impl Recovery {
    pub fn new(store: Store, project: &Project, ctx: egui::Context) -> Self {
        let (worker, failure) =
            match Worker::new(store.clone(), project.session, project.recovery_origin, ctx) {
                Ok(worker) => (Some(worker), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "Recovery is unavailable: {error}. You can still save projects normally."
                    )),
                ),
            };
        let (entries, error) = match store.list() {
            Ok(entries) => (entries, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        let error = failure.or(error);
        Self {
            store,
            open: entries.iter().any(|entry| {
                entry.id != project.session && Some(entry.id) != project.recovery_origin
            }) || error.is_some(),
            entries,
            worker,
            id: project.session,
            origin: project.recovery_origin,
            clean: true,
            last_revision: None,
            last_request: Instant::now(),
            error,
        }
    }
    pub fn tick(&mut self, project: &Project) -> Option<String> {
        let worker = self.worker.as_ref()?;
        match worker.take_result() {
            Some(Err(error)) => {
                self.error = Some(error.clone());
                self.last_revision = None;
                self.last_request = Instant::now();
                return Some(format!("Recovery copy could not be updated: {error}"));
            }
            Some(Ok(())) => self.error = None,
            None => {}
        }
        if !project.dirty() {
            if !self.clean {
                worker.submit(Action::Clear);
            }
            self.clean = true;
            self.last_revision = None;
        } else {
            self.clean = false;
            if self.last_revision != Some(project.document.revision())
                && self.last_request.elapsed() >= Duration::from_secs(10)
            {
                worker.submit(Action::Save(
                    Box::new(project.document.snapshot().clone()),
                    project.base.clone(),
                ));
                self.last_revision = Some(project.document.revision());
                self.last_request = Instant::now();
            }
        }
        None
    }
    pub fn refresh(&mut self) {
        match self.store.list() {
            Ok(entries) => self.entries = entries,
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub fn finish(&mut self, clean: bool) {
        if let Some(worker) = &mut self.worker {
            worker.finish(clean);
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, enabled: bool) -> Option<NodeId> {
        let mut restore = None;
        if self.open {
            egui::Window::new("Recover unsaved work").open(&mut self.open).show(ctx,|ui|{
                ui.label("Recovery copies open as unsaved compositions. Save as a project after restoring.");
                ui.weak("Only applied edits are recovered; image files must still be available.");
                if let Some(error)=&self.error{ui.colored_label(egui::Color32::LIGHT_RED,error);}
                let mut any=false;
                for entry in self.entries.iter().filter(|entry|entry.id!=self.id && Some(entry.id)!=self.origin) {
                    any=true;
                    ui.horizontal(|ui|{
                        let age=entry.modified.elapsed().unwrap_or_default().as_secs()/60;
                        ui.label(format!("{} · {age} min ago",&entry.id.to_string()[..8]));
                        if ui.add_enabled(enabled,egui::Button::new("Restore")).clicked(){restore=Some(entry.id);}
                    });
                }
                if !any{ui.label("No previous recovery copies.");}
            });
        }
        restore
    }
}
pub fn default_store() -> Result<Store, String> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/state"))
                .filter(|path| path.is_absolute())
        })
        .ok_or("Cannot locate local state directory for recovery")?;
    Ok(Store::new(base.join("veylvik/recovery")))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_recovery_storage_does_not_prevent_project_editing() {
        let blocked =
            std::env::temp_dir().join(format!("veylvik-recovery-blocked-{}", NodeId::new_v4()));
        std::fs::write(&blocked, b"not a directory").unwrap();
        let mut project = Project::open(None).unwrap();
        let mut recovery = Recovery::new(
            Store::new(blocked.join("recovery")),
            &project,
            egui::Context::default(),
        );
        assert!(recovery.worker.is_none());
        assert!(recovery.open && recovery.error.is_some());
        project
            .apply(&[veylvik_document::Command::Resize {
                width: 100,
                height: 80,
            }])
            .unwrap();
        assert!(recovery.tick(&project).is_none());
        assert_eq!(project.document.snapshot().width, 100);
        std::fs::remove_file(blocked).unwrap();
    }
    #[test]
    fn recovery_worker_flushes_latest_snapshot_and_cleans_only_when_requested() {
        let root =
            std::env::temp_dir().join(format!("veylvik-recovery-worker-{}", NodeId::new_v4()));
        let store = Store::new(root.clone());
        let id = NodeId::new_v4();
        let doc = veylvik_document::demo();
        let mut worker = Worker::new(store.clone(), id, None, egui::Context::default()).unwrap();
        for width in [10, 20, 30] {
            let mut copy = doc.clone();
            copy.width = width;
            worker.submit(Action::Save(Box::new(copy), root.clone()));
        }
        worker.finish(false);
        assert_eq!(store.read(id).unwrap().width, 30);
        let second = NodeId::new_v4();
        let mut worker =
            Worker::new(store.clone(), second, Some(id), egui::Context::default()).unwrap();
        worker.submit(Action::Save(Box::new(doc), root.clone()));
        worker.finish(true);
        assert!(store.list().unwrap().is_empty());
        let active = store.claim(id).unwrap();
        store.write(id, &veylvik_document::demo(), &root).unwrap();
        let mut worker = Worker::new(
            store.clone(),
            NodeId::new_v4(),
            Some(id),
            egui::Context::default(),
        )
        .unwrap();
        worker.finish(true);
        drop(active);
        assert!(
            store.read(id).is_ok(),
            "cleanup must not consume a running editor's copy"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
