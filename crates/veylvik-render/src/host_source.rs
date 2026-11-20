//! A host's own media as footage: the host hands over its frame as a buffer on the shared device and names it, and the same name is the same frame, so a picture made of it is kept until the host says its media changed.
use crate::{FrameSource, GpuFrame, HostPixels, Image, Renderer, Result, SourceInfo, error};
use std::sync::{Arc, Mutex};
use veylvik_document::Time;

/// One frame as the host holds it: `width` by `height` pixels of RGBA float32, rows top
/// first, in a storage buffer on the renderer's device. The host keeps the buffer; it is
/// read before the call that asked for it returns.
pub struct HostFrame {
    pub buffer: wgpu::Buffer,
    pub width: u32,
    pub height: u32,
    pub pixels: HostPixels,
}
/// What a host answers about one piece of its media.
pub trait HostMedia: Send + Sync {
    fn info(&self) -> SourceInfo;
    /// A name for the frame at `time`, the same for as long as the frame is the same and
    /// another when it is not: a replaced or relinked file, a new proxy, a grade at the
    /// source. Asked before every use, so it should be cheap.
    fn identity(&self, time: Time) -> u64;
    /// The frame at `time`, no larger than `target` when the host can give a smaller one.
    fn frame(&self, time: Time, target: Option<[u32; 2]>) -> Result<HostFrame>;
}
type Kept = (u64, Option<[u32; 2]>, Arc<GpuFrame>);
/// Footage read from a host. Frames it has taken are kept, newest first, within
/// `budget` bytes of graphics memory (always the newest one).
pub struct HostSource<M> {
    media: M,
    budget: u64,
    kept: Mutex<Vec<Kept>>,
    lasting: bool,
}
impl<M: HostMedia> HostSource<M> {
    pub fn new(media: M, budget: u64) -> Self {
        Self {
            media,
            budget,
            kept: Mutex::new(Vec::new()),
            lasting: false,
        }
    }
    /// The host promises that an identity it gives means one frame and no other: not
    /// another frame of this media, not a frame of any other media it has, and not
    /// another frame in a later session. Then pictures made of its frames are known
    /// again across sources and kept between sessions. Without the promise an identity
    /// only tells this source's own frames apart, which is always safe.
    pub fn lasting(mut self) -> Self {
        self.lasting = true;
        self
    }
    pub fn media(&self) -> &M {
        &self.media
    }
    /// Let go of every frame taken from the host.
    pub fn forget(&self) {
        if let Ok(mut kept) = self.kept.lock() {
            kept.clear();
        }
    }
    /// Bytes of graphics memory held in frames taken from the host.
    pub fn held(&self) -> u64 {
        self.kept.lock().map_or(0, |kept| {
            kept.iter().map(|(_, _, frame)| bytes(frame)).sum()
        })
    }
}
fn bytes(frame: &GpuFrame) -> u64 {
    u64::from(frame.width()) * u64::from(frame.height()) * 16
}
impl<M: HostMedia> FrameSource for HostSource<M> {
    fn info(&self) -> Option<SourceInfo> {
        Some(self.media.info())
    }
    fn frame(&self, _: Time) -> Result<Arc<Image>> {
        Err(error(
            "a host's media is read on the graphics card only; it has no copy in ordinary memory",
        ))
    }
    fn gpu_frame(&self, time: Time, renderer: &Renderer) -> Result<Option<Arc<GpuFrame>>> {
        self.gpu_frame_sized(time, renderer, None)
    }
    fn gpu_frame_sized(
        &self,
        time: Time,
        renderer: &Renderer,
        target: Option<[u32; 2]>,
    ) -> Result<Option<Arc<GpuFrame>>> {
        let identity = self.media.identity(time);
        let mut kept = self
            .kept
            .lock()
            .map_err(|_| error("host frames poisoned"))?;
        if let Some(at) = kept
            .iter()
            .position(|(i, t, frame)| *i == identity && *t == target && renderer.owns(frame))
        {
            let found = kept.remove(at);
            let frame = found.2.clone();
            kept.insert(0, found);
            return Ok(Some(frame));
        }
        let theirs = self.media.frame(time, target)?;
        let frame = renderer.frame_from_host(
            &theirs.buffer,
            theirs.width,
            theirs.height,
            &theirs.pixels,
        )?;
        // Only a host that has promised its names mean one frame everywhere has its
        // frames named by them; otherwise a frame is known within this source alone.
        let frame = Arc::new(if self.lasting {
            let size = target.map_or(0, |[w, h]| u64::from(w) << 32 | u64::from(h));
            frame.named(crate::fnv(&identity.to_le_bytes(), size))
        } else {
            frame
        });
        kept.insert(0, (identity, target, frame.clone()));
        let mut held = 0;
        let mut keep = 0;
        for (_, _, frame) in kept.iter() {
            held += bytes(frame);
            if keep > 0 && held > self.budget {
                break;
            }
            keep += 1;
        }
        kept.truncate(keep);
        Ok(Some(frame))
    }
    fn byte_budget(&self) -> u64 {
        self.budget
    }
    // A frame is kept under the host's name for it and handed out again.
    fn steady(&self) -> bool {
        true
    }
}
