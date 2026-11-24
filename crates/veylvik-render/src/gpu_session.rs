//! The one way into GPU work that draws on the renderer's buffers: the pool is locked, refused once an earlier GPU error left the device in doubt, and kept within what the budget leaves; the work runs with the device watching for validation and out-of-memory errors, and one such error marks the renderer unusable. Every entry point (render, read-back, video conversion and encoding, texture import, scratch compute, drawing) goes through here, so the policy is written once.
use crate::{Renderer, Result, error, pool::Pool};
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
        if let Some(e) = allocation_error.or(validation_error) {
            unusable(pool);
            return Err(error(e));
        }
        result
    }
}
/// After a GPU failure nothing more is sent to the device: buffers are let go and every
/// later call is refused (see `UNUSABLE`).
pub(crate) fn unusable(pool: &mut Pool) {
    pool.trim(0);
    pool.failed = true;
}
