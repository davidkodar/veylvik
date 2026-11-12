//! One active and one queued immutable image capture; no UI-thread GPU readback.
use serde_json::Value;
use std::sync::{Arc, mpsc};
use veylvik_control::{ToolOutput, ipc::Request};
use veylvik_render::{GpuFrame, Renderer};
pub struct Snapshot {
    pub frame: Arc<GpuFrame>,
    pub metadata: Value,
    pub max_edge: u32,
}
struct Job {
    request: Request,
    renderer: Arc<Renderer>,
    snapshot: Snapshot,
}
pub struct Worker {
    sender: Option<mpsc::SyncSender<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new() -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let thread = std::thread::Builder::new()
            .name("veylvik-preview-image".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        encode(&job.renderer, job.snapshot)
                    }))
                    .unwrap_or_else(|_| Err("preview image worker failed unexpectedly".into()));
                    job.request.respond_output(result);
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
        })
    }
    pub fn submit(&self, request: Request, renderer: Arc<Renderer>, snapshot: Snapshot) {
        let job = Job {
            request,
            renderer,
            snapshot,
        };
        if let Err(error) = self.sender.as_ref().unwrap().try_send(job) {
            let job = match error {
                mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => job,
            };
            job.request
                .respond(Err("preview image worker is busy or unavailable".into()));
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn encode(renderer: &Renderer, snapshot: Snapshot) -> Result<ToolOutput, String> {
    let frame = renderer
        .read_frame(&snapshot.frame)
        .map_err(|error| error.to_string())?;
    let thumbnail = veylvik_io::preview::thumbnail(
        frame.region.width,
        frame.region.height,
        &frame.pixels,
        snapshot.max_edge,
    )
    .map_err(|error| error.to_string())?;
    let mut metadata = snapshot.metadata;
    metadata["width"] = thumbnail.width.into();
    metadata["height"] = thumbnail.height.into();
    Ok(ToolOutput::png(metadata, &thumbnail.png))
}
