//! Exact-size buffer reuse. In-flight or mapped buffers never enter the idle pool.
use crate::{Result, error};
use std::collections::VecDeque;
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ResourceStats {
    pub buffers_created: u64,
    pub buffers_reused: u64,
    pub buffers_evicted: u64,
    pub resident_bytes: u64,
    pub idle_bytes: u64,
    pub peak_resident_bytes: u64,
}
struct Entry {
    buffer: wgpu::Buffer,
    size: u64,
    usage: wgpu::BufferUsages,
}
#[derive(Default)]
pub(crate) struct Pool {
    idle: VecDeque<Entry>,
    stats: ResourceStats,
    pub failed: bool,
}
impl Pool {
    pub fn stats(&self) -> ResourceStats {
        self.stats
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
        while self.stats.idle_bytes > limit {
            if !self.evict() {
                break;
            }
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
            .position(|e| e.size == size && e.usage == usage)
        {
            let entry = self.pool.idle.remove(index).unwrap();
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
            self.pool.failed = true;
            self.pool.trim(0);
        }
    }
}
