//! Exact-size buffer reuse. In-flight or mapped buffers never enter the idle pool.
use crate::{Result, error};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ResourceStats {
    pub buffers_created: u64,
    pub source_uploads: u64,
    pub source_upload_bytes: u64,
    pub source_reuses: u64,
    pub buffers_reused: u64,
    pub buffers_evicted: u64,
    pub resident_bytes: u64,
    pub idle_bytes: u64,
    pub held_bytes: u64,
    pub peak_resident_bytes: u64,
}
struct Entry {
    buffer: wgpu::Buffer,
    size: u64,
    usage: wgpu::BufferUsages,
    source: Option<u64>,
}
#[derive(Default)]
pub(crate) struct Pool {
    idle: VecDeque<Entry>,
    stats: ResourceStats,
    retired: Arc<AtomicU64>,
    pub failed: bool,
}
impl Pool {
    pub fn stats(&mut self) -> ResourceStats {
        self.reap();
        self.stats
    }
    fn reap(&mut self) {
        let retired = self.retired.swap(0, Ordering::AcqRel);
        self.stats.resident_bytes -= retired;
        self.stats.held_bytes -= retired;
    }
    fn evict(&mut self) -> bool {
        let Some(entry) = self.idle.pop_front() else {
            return false;
        };
        entry.buffer.destroy();
        self.stats.resident_bytes -= entry.size;
        self.stats.idle_bytes -= entry.size;
        self.stats.buffers_evicted += 1;
        true
    }
    pub fn trim(&mut self, limit: u64) {
        self.reap();
        while self.stats.idle_bytes > limit {
            if !self.evict() {
                break;
            }
        }
    }
    pub fn hold_external(&mut self, size: u64) -> ExternalReservation {
        self.stats.resident_bytes += size;
        self.stats.held_bytes += size;
        self.stats.peak_resident_bytes = self
            .stats
            .peak_resident_bytes
            .max(self.stats.resident_bytes);
        ExternalReservation {
            size,
            retired: self.retired.clone(),
        }
    }
    pub fn frame(&mut self, budget: u64) -> FrameBuffers<'_> {
        FrameBuffers {
            pool: self,
            budget,
            entries: Vec::new(),
            reusable: true,
        }
    }
}
pub(crate) struct FrameBuffers<'a> {
    pool: &'a mut Pool,
    budget: u64,
    entries: Vec<Entry>,
    reusable: bool,
}
impl FrameBuffers<'_> {
    pub fn pin(&mut self, buffer: &wgpu::Buffer) -> PinnedBuffer {
        let index = self
            .entries
            .iter()
            .position(|entry| &entry.buffer == buffer)
            .unwrap();
        let entry = self.entries.swap_remove(index);
        self.pool.stats.held_bytes += entry.size;
        PinnedBuffer {
            buffer: entry.buffer,
            size: entry.size,
            retired: self.pool.retired.clone(),
        }
    }

    /// Immutable sources may reuse their contents only while the completed
    /// allocation remains idle. Ordinary buffer reuse clears this identity.
    pub fn source(
        &mut self,
        device: &wgpu::Device,
        identity: u64,
        size: u64,
        label: &str,
    ) -> Result<(wgpu::Buffer, bool)> {
        if let Some(index) = self
            .pool
            .idle
            .iter()
            .position(|entry| entry.source == Some(identity))
        {
            let entry = self.pool.idle.remove(index).unwrap();
            self.pool.stats.idle_bytes -= entry.size;
            self.pool.stats.buffers_reused += 1;
            self.pool.stats.source_reuses += 1;
            let buffer = entry.buffer.clone();
            self.entries.push(entry);
            return Ok((buffer, false));
        }
        let buffer = self.take(
            device,
            size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label,
        )?;
        self.entries.last_mut().unwrap().source = Some(identity);
        self.pool.stats.source_uploads += 1;
        self.pool.stats.source_upload_bytes += size;
        Ok((buffer, true))
    }

    pub fn take(
        &mut self,
        device: &wgpu::Device,
        size: u64,
        usage: wgpu::BufferUsages,
        label: &str,
    ) -> Result<wgpu::Buffer> {
        let entry = if let Some(index) = self
            .pool
            .idle
            .iter()
            .position(|e| e.size == size && e.usage == usage && e.source.is_none())
            .or_else(|| {
                self.pool
                    .idle
                    .iter()
                    .position(|e| e.size == size && e.usage == usage)
            }) {
            let mut entry = self.pool.idle.remove(index).unwrap();
            entry.source = None;
            self.pool.stats.idle_bytes -= size;
            self.pool.stats.buffers_reused += 1;
            entry
        } else {
            while self.pool.stats.resident_bytes + size > self.budget {
                if !self.pool.evict() {
                    return Err(error("GPU buffer budget exhausted"));
                }
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            });
            self.pool.stats.resident_bytes += size;
            self.pool.stats.buffers_created += 1;
            self.pool.stats.peak_resident_bytes = self
                .pool
                .stats
                .peak_resident_bytes
                .max(self.pool.stats.resident_bytes);
            Entry {
                buffer,
                size,
                usage,
                source: None,
            }
        };
        let handle = entry.buffer.clone();
        self.entries.push(entry);
        Ok(handle)
    }
    pub fn submitted(&mut self) {
        self.reusable = false;
    }
    pub fn completed(&mut self) {
        self.reusable = true;
    }
}
impl Drop for FrameBuffers<'_> {
    fn drop(&mut self) {
        for entry in self.entries.drain(..) {
            if self.reusable {
                self.pool.stats.idle_bytes += entry.size;
                self.pool.idle.push_back(entry);
            } else {
                entry.buffer.destroy();
                self.pool.stats.resident_bytes -= entry.size;
                self.pool.stats.buffers_evicted += 1;
            }
        }
        if !self.reusable {
            // Completion is unknown. Require a new renderer rather than accumulate
            // additional submissions behind a timed-out or lost device.
            crate::gpu_session::unusable(self.pool);
        }
    }
}

/// Output allocations are never recycled while visible to a consumer. Retirement
/// uses an atomic counter so dropping a frame cannot deadlock the render mutex.
pub(crate) struct PinnedBuffer {
    pub buffer: wgpu::Buffer,
    size: u64,
    retired: Arc<AtomicU64>,
}
impl Drop for PinnedBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.retired.fetch_add(self.size, Ordering::Release);
    }
}

pub(crate) struct ExternalReservation {
    size: u64,
    retired: Arc<AtomicU64>,
}
impl Drop for ExternalReservation {
    fn drop(&mut self) {
        self.retired.fetch_add(self.size, Ordering::Release);
    }
}
