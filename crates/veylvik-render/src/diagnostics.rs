//! Nonblocking inspection for interactive hosts sharing the render worker's device.
use crate::{CacheStats, Renderer, ResourceStats, Result, error};
use std::sync::{Mutex, MutexGuard, TryLockError};
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Diagnostics {
    pub resources: ResourceStats,
    pub frames: CacheStats,
    pub vectors: CacheStats,
    pub budget_bytes: u64,
    pub idle_limit_bytes: u64,
    pub frame_cache_limit_bytes: u64,
    pub vector_cache_limit_bytes: u64,
}
fn available<T>(mutex: &Mutex<T>) -> Result<Option<MutexGuard<'_, T>>> {
    match mutex.try_lock() {
        Ok(guard) => Ok(Some(guard)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Poisoned(_)) => Err(error(
            "renderer diagnostics unavailable after a worker failure",
        )),
    }
}
impl Renderer {
    /// Consistent resource/cache counters without waiting for rendering.
    /// Returns `None` when any required lock is in use. Driver and vector backend
    /// scratch allocations are not included in the declared buffer accounting.
    pub fn try_diagnostics(&self) -> Result<Option<Diagnostics>> {
        let Some(frames) = available(&self.cache)? else {
            return Ok(None);
        };
        let Some(vectors) = available(&self.vector_cache)? else {
            return Ok(None);
        };
        let Some(mut pool) = available(&self.pool)? else {
            return Ok(None);
        };
        Ok(Some(Diagnostics {
            resources: pool.stats(),
            frames: frames.stats(),
            vectors: vectors.stats(),
            budget_bytes: self.budget_bytes,
            idle_limit_bytes: self.pool_limit_bytes,
            frame_cache_limit_bytes: self.cache_limit_bytes,
            vector_cache_limit_bytes: self.vector_cache_limit_bytes,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires Vulkan"]
    fn diagnostics_never_wait_for_busy_resources_and_report_completed_frames() {
        let renderer = Renderer::new().unwrap();
        {
            let _pool = renderer.pool.lock().unwrap();
            assert!(renderer.try_diagnostics().unwrap().is_none());
        }
        {
            let _cache = renderer.cache.lock().unwrap();
            assert!(renderer.try_diagnostics().unwrap().is_none());
        }
        {
            let _vectors = renderer.vector_cache.lock().unwrap();
            assert!(renderer.try_diagnostics().unwrap().is_none());
        }
        let mut doc = veylvik_document::demo();
        doc.width = 13;
        doc.height = 9;
        let plan = crate::Plan::compile(&doc).unwrap();
        let request = crate::RenderRequest::full(veylvik_document::Time::new(0, 1).unwrap());
        let inputs = crate::ImageInputs::default();
        let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
        let same = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
        assert!(std::sync::Arc::ptr_eq(&frame, &same));
        let stats = renderer.try_diagnostics().unwrap().unwrap();
        assert_eq!(stats.frames.hits, 1);
        assert_eq!(stats.frames.misses, 1);
        assert_eq!(stats.frames.entries, 1);
        assert!(stats.resources.held_bytes >= 13 * 9 * 16);
        assert!(stats.resources.resident_bytes <= stats.budget_bytes);
        assert_eq!(stats.frames.bytes, 13 * 9 * 16);
        drop(frame);
        drop(same);
        renderer.clear_frame_cache().unwrap();
        renderer.trim_resources().unwrap();
        let stats = renderer.try_diagnostics().unwrap().unwrap();
        assert_eq!(stats.resources.resident_bytes, 0);
        assert_eq!(stats.resources.held_bytes, 0);
        assert_eq!(stats.frames.entries, 0);
    }
}
