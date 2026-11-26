//! The one way into GPU work that draws on the renderer's buffers: the pool is locked, refused once an earlier GPU error left the device in doubt, and kept within what the budget leaves; the work runs with the device watching for validation and out-of-memory errors. A validation error marks the renderer unusable; running out of memory does not (the device is whole), it frees the pool and is reported as `Error::OutOfMemory`, which `retrying` meets by freeing every cache and trying once more. Every entry point (render, read-back, video conversion and encoding, texture import, scratch compute, drawing) goes through here, so the policy is written once.
use crate::{Error, Renderer, Result, error, pool::Pool};
use std::sync::MutexGuard;

/// What every refused call says once the renderer is unusable.
pub(crate) const UNUSABLE: &str = "renderer unavailable after a GPU failure; create a new renderer";

impl Renderer {
    /// The resource pool, locked for GPU work; refused after a GPU failure.
    pub(crate) fn working_pool(&self) -> Result<MutexGuard<'_, Pool>> {
        let pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        if pool.failed {
            return Err(error(UNUSABLE));
        }
        Ok(pool)
    }
    /// The pool, locked and trimmed to what the budget leaves less `reserve` bytes (held
    /// apart for memory outside the pool), with the bytes the budget leaves.
    pub(crate) fn admit(&self, reserve: u64) -> Result<(MutexGuard<'_, Pool>, u64)> {
        let mut pool = self.working_pool()?;
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available.saturating_sub(reserve)));
        Ok((pool, available))
    }
    /// Trims the pool to the budget again after work.
    pub(crate) fn settle(&self, pool: &mut Pool) {
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
    }
    /// Runs `work` with the device watching for validation and out-of-memory errors; one
    /// marks the renderer unusable and is returned in place of the work's result.
    pub(crate) fn guarded<T>(
        &self,
        pool: &mut Pool,
        work: impl FnOnce(&mut Pool) -> Result<T>,
    ) -> Result<T> {
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = work(pool);
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = caught(pool, allocation_error, validation_error) {
            return Err(e);
        }
        result
    }
    /// Lets go of everything this renderer keeps between calls: kept frames and node
    /// results, drawn shapes, idle buffers and the 3D renderer's caches.
    pub fn free_gpu_memory(&self) -> Result<()> {
        self.clear_frame_cache()?;
        if let Some(scene) = self.scene_renderer.read().ok().and_then(|s| s.clone()) {
            scene.release();
        }
        self.working_pool()?.trim(0);
        Ok(())
    }
    /// Runs `work`; if the card ran out of memory, frees what this renderer keeps and
    /// runs it once more. A second shortage is returned as it is.
    pub(crate) fn retrying<T>(&self, mut work: impl FnMut() -> Result<T>) -> Result<T> {
        match work() {
            Err(e) if e.is_out_of_memory() => {
                self.free_gpu_memory()?;
                work()
            }
            other => other,
        }
    }
}
/// What an error scope caught. Running out of memory leaves the device whole: idle
/// buffers are let go and the renderer stays usable. Any other error marks it unusable.
pub(crate) fn caught(
    pool: &mut Pool,
    allocation: Option<wgpu::Error>,
    validation: Option<wgpu::Error>,
) -> Option<Error> {
    if let Some(e) = allocation {
        pool.trim(0);
        return Some(Error::OutOfMemory(e.to_string()));
    }
    validation.map(|e| {
        unusable(pool);
        error(e)
    })
}
/// After a GPU failure nothing more is sent to the device: buffers are let go and every
/// later call is refused (see `UNUSABLE`).
pub(crate) fn unusable(pool: &mut Pool) {
    pool.trim(0);
    pool.failed = true;
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(text: &str) -> wgpu::ErrorSource {
        text.to_string().into()
    }
    #[test]
    fn running_out_of_memory_leaves_the_renderer_usable_and_anything_else_does_not() {
        let mut pool = Pool::default();
        let out = caught(
            &mut pool,
            Some(wgpu::Error::OutOfMemory {
                source: source("full"),
            }),
            None,
        );
        assert!(out.is_some_and(|e| e.is_out_of_memory()));
        assert!(!pool.failed);
        // a validation error raised by the objects whose allocation failed is part of the same shortage
        let both = caught(
            &mut pool,
            Some(wgpu::Error::OutOfMemory {
                source: source("full"),
            }),
            Some(wgpu::Error::Validation {
                source: source("invalid buffer"),
                description: "invalid buffer".into(),
            }),
        );
        assert!(both.is_some_and(|e| e.is_out_of_memory()));
        assert!(!pool.failed);
        let bad = caught(
            &mut pool,
            None,
            Some(wgpu::Error::Validation {
                source: source("bad"),
                description: "bad".into(),
            }),
        );
        assert!(bad.is_some_and(|e| !e.is_out_of_memory()));
        assert!(pool.failed);
        assert!(caught(&mut Pool::default(), None, None).is_none());
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_shortage_is_met_by_freeing_memory_and_trying_once_more() {
        let renderer = Renderer::new().unwrap();
        let mut calls = 0;
        let result = renderer.retrying(|| {
            calls += 1;
            if calls == 1 {
                Err(Error::OutOfMemory("full".into()))
            } else {
                Ok(7)
            }
        });
        assert_eq!((result.unwrap(), calls), (7, 2));
        // a second shortage is reported, and only one retry is made
        let mut calls = 0;
        let result: Result<()> = renderer.retrying(|| {
            calls += 1;
            Err(Error::OutOfMemory("full".into()))
        });
        assert!(result.is_err_and(|e| e.is_out_of_memory()) && calls == 2);
        assert!(!renderer.device_failed());
        // other errors are not retried
        let mut calls = 0;
        let result: Result<()> = renderer.retrying(|| {
            calls += 1;
            Err(error("something else"))
        });
        assert!(result.is_err() && calls == 1);
    }
}
