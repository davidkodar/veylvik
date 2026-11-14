//! One active render, one replaceable pending request, one replaceable completion.
use crate::{
    GpuFrame, ImageInputs, Plan, RenderRequest, RenderedFrame, Renderer, ResourceStats, Result,
    error,
};
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// A generation identifies a request, even when its document revision is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewTicket {
    pub generation: u64,
    pub revision: u64,
}
struct Job {
    revision: u64,
    plan: Arc<Plan>,
    request: RenderRequest,
    inputs: ImageInputs,
}
/// Completion for the newest request only. Time includes initialization if needed.
pub struct PreviewResult<F = RenderedFrame> {
    pub ticket: PreviewTicket,
    pub request: RenderRequest,
    pub frame: Result<F>,
    pub elapsed: Duration,
    pub resources: Option<ResourceStats>,
}
/// Keeps GPU work off the caller's thread and discards obsolete queued work.
///
/// Submitted GPU work runs to completion; cancellation suppresses its result.
/// Dropping the scheduler joins its worker, including an active render. Inputs and
/// plans are immutable snapshots; document mutation stays with the caller.
pub struct PreviewScheduler<F = RenderedFrame> {
    worker: LatestWorker<Job, PreviewResult<F>>,
}
impl PreviewScheduler {
    pub fn new() -> Result<Self> {
        Self::with_wake(|| {})
    }
    /// Called on the worker thread after publishing a completion. A UI can send
    /// an event from here; the callback must not block or panic.
    pub fn with_wake(wake: impl Fn() + Send + 'static) -> Result<Self> {
        let mut renderer: Option<Renderer> = None;
        let worker = LatestWorker::new(
            move |generation, job: Job| {
                let start = Instant::now();
                let frame = (|| {
                    if renderer.is_none() {
                        renderer = Some(Renderer::new()?);
                    }
                    renderer
                        .as_ref()
                        .unwrap()
                        .render_frame(&job.plan, job.request, &job.inputs)
                })();
                let resources = renderer.as_ref().and_then(|r| r.resource_stats().ok());
                // A timed-out/lost device must not receive more submissions.
                if renderer
                    .as_ref()
                    .is_some_and(|r| r.pool.lock().map_or(true, |p| p.failed))
                {
                    renderer = None;
                }
                PreviewResult {
                    ticket: PreviewTicket {
                        generation,
                        revision: job.revision,
                    },
                    request: job.request,
                    frame,
                    elapsed: start.elapsed(),
                    resources,
                }
            },
            wake,
        )?;
        Ok(Self { worker })
    }
}
/// Preview worker producing GPU-resident output on a host-owned renderer/device.
pub type GpuPreviewScheduler = PreviewScheduler<Arc<GpuFrame>>;
impl PreviewScheduler<Arc<GpuFrame>> {
    pub fn with_renderer(
        renderer: Arc<Renderer>,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let worker = LatestWorker::new_cancellable(
            move |generation, job: Job, cancelled| {
                let start = Instant::now();
                let frame = if job.plan.temporal.is_empty() {
                    renderer.render_gpu_cached(&job.plan, job.request, &job.inputs)
                } else {
                    renderer
                        .render_gpu_temporal(&job.plan, job.request, &job.inputs, true, cancelled)
                        .and_then(|frame| {
                            frame
                                .map(Arc::new)
                                .ok_or_else(|| error("Preview superseded"))
                        })
                };
                PreviewResult {
                    ticket: PreviewTicket {
                        generation,
                        revision: job.revision,
                    },
                    request: job.request,
                    frame,
                    elapsed: start.elapsed(),
                    resources: renderer.resource_stats().ok(),
                }
            },
            wake,
        )?;
        Ok(Self { worker })
    }
}
impl<F: Send + 'static> PreviewScheduler<F> {
    pub fn submit(
        &self,
        revision: u64,
        plan: Arc<Plan>,
        request: RenderRequest,
        inputs: ImageInputs,
    ) -> Result<PreviewTicket> {
        let generation = self.worker.submit(Job {
            revision,
            plan,
            request,
            inputs,
        })?;
        Ok(PreviewTicket {
            generation,
            revision,
        })
    }
    /// Invalidates pending and completed previews, without waiting for GPU work.
    pub fn cancel(&self) -> Result<()> {
        self.worker.cancel()
    }
    /// Nonblocking. A returned error means the worker has terminated unexpectedly.
    pub fn try_take(&self) -> Result<Option<PreviewResult<F>>> {
        self.worker.try_take()
    }
}
struct State<I, O> {
    generation: u64,
    pending: Option<(u64, I)>,
    completed: Option<O>,
    closed: bool,
}
struct Shared<I, O> {
    state: Mutex<State<I, O>>,
    ready: Condvar,
}
struct LatestWorker<I, O> {
    shared: Arc<Shared<I, O>>,
    thread: Option<JoinHandle<()>>,
}
impl<I: Send + 'static, O: Send + 'static> LatestWorker<I, O> {
    fn new(
        mut render: impl FnMut(u64, I) -> O + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        Self::new_cancellable(move |generation, input, _| render(generation, input), wake)
    }
    fn new_cancellable(
        mut render: impl FnMut(u64, I, &mut dyn FnMut() -> bool) -> O + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                generation: 0,
                pending: None,
                completed: None,
                closed: false,
            }),
            ready: Condvar::new(),
        });
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("veylvik-preview".into())
            .spawn(move || {
                // A panic cannot strand the host waiting indefinitely for a completion.
                let mut waking = false;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    loop {
                        let (generation, input) = {
                            let mut lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                            while !lock.closed && lock.pending.is_none() {
                                lock = state.ready.wait(lock).unwrap_or_else(|e| e.into_inner());
                            }
                            if lock.closed {
                                return;
                            }
                            lock.pending.take().unwrap()
                        };
                        let output = render(generation, input, &mut || {
                            let lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                            lock.closed || lock.generation != generation
                        });
                        let published = {
                            let mut lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                            if !lock.closed && lock.generation == generation {
                                lock.completed = Some(output);
                                true
                            } else {
                                false
                            }
                        };
                        if published {
                            waking = true;
                            wake();
                            waking = false;
                        }
                    }
                }));
                if result.is_err() {
                    let mut lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                    lock.closed = true;
                    lock.pending = None;
                    lock.completed = None;
                    drop(lock);
                    // Wake the host on render failure, but never repeat a failed callback.
                    if !waking {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&wake));
                    }
                }
            })
            .map_err(error)?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
    fn advance(state: &mut State<I, O>) -> Result<u64> {
        if state.closed {
            return Err(error("preview worker has stopped"));
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| error("preview generation exhausted"))?;
        state.completed = None;
        state.pending = None;
        Ok(state.generation)
    }
    fn submit(&self, input: I) -> Result<u64> {
        let mut lock = self
            .shared
            .state
            .lock()
            .map_err(|_| error("preview state poisoned"))?;
        let generation = Self::advance(&mut lock)?;
        lock.pending = Some((generation, input));
        self.shared.ready.notify_one();
        Ok(generation)
    }
    fn cancel(&self) -> Result<()> {
        let mut lock = self
            .shared
            .state
            .lock()
            .map_err(|_| error("preview state poisoned"))?;
        Self::advance(&mut lock)?;
        Ok(())
    }
    fn try_take(&self) -> Result<Option<O>> {
        let mut lock = self
            .shared
            .state
            .lock()
            .map_err(|_| error("preview state poisoned"))?;
        if lock.closed {
            return Err(error("preview worker has stopped"));
        }
        Ok(lock.completed.take())
    }
}
impl<I, O> Drop for LatestWorker<I, O> {
    fn drop(&mut self) {
        let mut lock = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        lock.closed = true;
        lock.pending = None;
        lock.completed = None;
        self.shared.ready.notify_one();
        drop(lock);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    const TIMEOUT: Duration = Duration::from_secs(5);
    #[test]
    fn latest_pending_replaces_obsolete_work_and_completion() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (wake_tx, wake_rx) = mpsc::channel();
        let worker = LatestWorker::new(
            move |id, value| {
                started_tx.send(value).unwrap();
                release_rx.recv_timeout(TIMEOUT).unwrap();
                (id, value)
            },
            move || {
                wake_tx.send(()).unwrap();
            },
        )
        .unwrap();
        worker.submit(1).unwrap();
        assert_eq!(started_rx.recv_timeout(TIMEOUT).unwrap(), 1);
        worker.submit(2).unwrap();
        let latest = worker.submit(3).unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(started_rx.recv_timeout(TIMEOUT).unwrap(), 3);
        assert!(worker.try_take().unwrap().is_none());
        release_tx.send(()).unwrap();
        wake_rx.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(worker.try_take().unwrap(), Some((latest, 3)));
        assert!(worker.try_take().unwrap().is_none());
        worker.submit(4).unwrap();
        started_rx.recv_timeout(TIMEOUT).unwrap();
        release_tx.send(()).unwrap();
        wake_rx.recv_timeout(TIMEOUT).unwrap();
        worker.cancel().unwrap();
        assert!(worker.try_take().unwrap().is_none());
    }
    #[test]
    fn cancellation_discards_active_result_and_queued_snapshot() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (wake_tx, wake_rx) = mpsc::channel();
        let worker = LatestWorker::new(
            move |_, value: Arc<u32>| {
                started_tx.send(*value).unwrap();
                release_rx.recv_timeout(TIMEOUT).unwrap();
                *value
            },
            move || {
                wake_tx.send(()).unwrap();
            },
        )
        .unwrap();
        worker.submit(Arc::new(1)).unwrap();
        started_rx.recv_timeout(TIMEOUT).unwrap();
        let queued = Arc::new(2);
        worker.submit(queued.clone()).unwrap();
        worker.cancel().unwrap();
        assert_eq!(Arc::strong_count(&queued), 1);
        release_tx.send(()).unwrap();
        worker.submit(Arc::new(3)).unwrap();
        assert_eq!(started_rx.recv_timeout(TIMEOUT).unwrap(), 3);
        assert!(worker.try_take().unwrap().is_none());
        release_tx.send(()).unwrap();
        wake_rx.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(worker.try_take().unwrap(), Some(3));
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn worker_failure_wakes_host_and_closes_submission() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = LatestWorker::new(
            |_, _: ()| -> () { panic!("test render failure") },
            move || {
                tx.send(()).unwrap();
            },
        )
        .unwrap();
        worker.submit(()).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(worker.try_take().is_err());
        assert!(worker.submit(()).is_err());
    }
    #[test]
    fn dropping_idle_worker_joins_it() {
        let worker = LatestWorker::new(|_, value: u32| value, || {}).unwrap();
        drop(worker);
    }
}
