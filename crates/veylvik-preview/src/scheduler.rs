//! One active render, one replaceable pending request, one replaceable completion.
use crate::{Result, error};
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use veylvik_render::{
    GpuFrame, ImageInputs, Plan, RenderRequest, RenderedFrame, Renderer, ResourceStats,
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
fn render_gpu_job(
    renderer: &Renderer,
    job: &Job,
    divisor: u32,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Arc<GpuFrame>> {
    let frame = if !job.plan.has_temporal() {
        renderer
            .render_gpu(&job.plan, job.request, &job.inputs)
            .map(Arc::new)
    } else {
        renderer
            .render_gpu_temporal(&job.plan, job.request, &job.inputs, true, cancelled)
            .and_then(|f| f.map(Arc::new).ok_or_else(|| error("Preview superseded")))
    }?;
    let frame = renderer.apply_ocio_view(&job.plan, &job.inputs, frame)?;
    renderer.reduce_preview(
        frame,
        if job.request.region.is_some() {
            1
        } else {
            divisor
        },
    )
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
    playback: crate::playback::Control,
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
                    let renderer = renderer.as_ref().unwrap();
                    let frame =
                        Arc::new(renderer.render_gpu(&job.plan, job.request, &job.inputs)?);
                    let frame = renderer.apply_ocio_view(&job.plan, &job.inputs, frame)?;
                    renderer.read_frame(&frame)
                })();
                let resources = renderer.as_ref().and_then(|r| r.resource_stats().ok());
                // A timed-out/lost device must not receive more submissions.
                if renderer.as_ref().is_some_and(|r| r.device_failed()) {
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
        Ok(Self {
            worker,
            playback: crate::playback::Control::default(),
        })
    }
}
/// Preview worker producing GPU-resident output on a host-owned renderer/device.
pub type GpuPreviewScheduler = PreviewScheduler<Arc<GpuFrame>>;
impl PreviewScheduler<Arc<GpuFrame>> {
    pub fn with_renderer(
        renderer: Arc<Renderer>,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let playback = crate::playback::Control::default();
        let mut cache = crate::playback::Store::new(playback.clone());
        let worker = LatestWorker::new_cancellable(
            move |generation, job: Job, cancelled| {
                let start = Instant::now();
                let frame = (|| {
                    cache.prepare(&job.plan, &job.inputs)?;
                    // The playback cache holds frames by time alone, so a free 3D view
                    // (which films the same time differently) bypasses it both ways.
                    // The reflections view is another picture of the same time, too.
                    let cacheable = job.request.region.is_none()
                        && job.request.scene_view.is_none()
                        && !job.request.scene_reflections;
                    let name = if cacheable {
                        cache.name(&renderer, &job.plan, job.request, &job.inputs)
                    } else {
                        None
                    };
                    if cacheable
                        && let Some(frame) = cache.lookup(&renderer, job.request.time, name)?
                    {
                        return Ok(frame);
                    }
                    cache.make_room(
                        &renderer,
                        job.plan
                            .gpu_working_bytes()
                            .saturating_add(job.inputs.byte_size()),
                    )?;
                    let mut frame = render_gpu_job(&renderer, &job, cache.divisor(), cancelled);
                    if frame
                        .as_ref()
                        .is_err_and(|e| e.to_string().contains("budget"))
                    {
                        cache.release_gpu();
                        frame = render_gpu_job(&renderer, &job, cache.divisor(), cancelled);
                    }
                    let frame = frame?;
                    if cacheable {
                        cache.insert(&renderer, job.request.time, name, frame.clone(), cancelled);
                    }
                    Ok(frame)
                })();
                cache.publish(job.revision);
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
        Ok(Self { worker, playback })
    }
}
impl<F: Send + 'static> PreviewScheduler<F> {
    pub fn playback_cache(&self) -> crate::playback::Control {
        self.playback.clone()
    }
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

#[cfg(test)]
mod playback_tests {
    use super::*;
    use veylvik_document::Time;
    #[test]
    #[ignore = "requires Vulkan"]
    fn gpu_scheduler_reuses_frames_through_a_new_plan_and_invalidates_resolution() {
        let renderer = Arc::new(Renderer::new().unwrap());
        let scheduler = GpuPreviewScheduler::with_renderer(renderer, || {}).unwrap();
        let control = scheduler.playback_cache();
        let make_plan = || {
            Arc::new(
                Plan::compile(&veylvik_document::blank(32, 16, Time::new(24, 1).unwrap()).unwrap())
                    .unwrap(),
            )
        };
        let plan = make_plan();
        let request = RenderRequest::full(Time::new(0, 1).unwrap());
        let render = |plan: Arc<Plan>| {
            let ticket = scheduler
                .submit(7, plan, request, ImageInputs::default())
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(result) = scheduler.try_take().unwrap() {
                    assert_eq!(result.ticket, ticket);
                    return result.frame.unwrap();
                }
                assert!(Instant::now() < deadline, "preview timed out");
                thread::sleep(Duration::from_millis(5));
            }
        };
        let first = render(plan.clone());
        let second = render(plan.clone());
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(control.status().hits, 1);
        control.set_resolution(2);
        let half = render(plan.clone());
        assert!(!Arc::ptr_eq(&first, &half));
        assert_eq!(control.status().size, [16, 8]);
        assert_eq!(control.status().hits, 0);
        let half_again = render(plan);
        assert_eq!(control.status().hits, 1);
        // The same composition compiled again is another plan making the same picture:
        // it is found, not made again (a new plan used to empty the store).
        let again = render(make_plan());
        assert!(Arc::ptr_eq(&half_again, &again));
        assert_eq!(control.status().hits, 1);
        scheduler.cancel().unwrap();
        assert!(scheduler.try_take().unwrap().is_none());
    }
}
