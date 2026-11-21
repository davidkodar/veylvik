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
    /// The viewer's look when the frame may be answered with, and kept as, a picture as
    /// the screen shows it; none when it must be the renderer's own frame.
    look: Option<crate::screen::Look>,
    /// Made to be kept, not to be shown: nobody waits for it.
    background: bool,
    /// The last of a list to prepare: once it is done, what is still being packed is
    /// taken into the store, so the store counts every frame of the list.
    last: bool,
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
    /// The frame is a played picture as the screen shows it: exposure and the display
    /// transform are already in it, so it is drawn with a neutral look and its values
    /// are not the renderer's.
    pub screen_ready: bool,
}
/// Keeps GPU work off the caller's thread and discards obsolete queued work.
///
/// Submitted GPU work runs to completion; cancellation suppresses its result.
/// Dropping the scheduler joins its worker, including an active render. Inputs and
/// plans are immutable snapshots; document mutation stays with the caller.
pub struct PreviewScheduler<F = RenderedFrame> {
    worker: LatestWorker<Job, PreviewResult<F>>,
    playback: crate::playback::Control,
    /// What rendering a frame has taken lately, in milliseconds (f64 bits; 0 = not yet).
    cost: Arc<std::sync::atomic::AtomicU64>,
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
                    screen_ready: false,
                }
            },
            wake,
        )?;
        Ok(Self {
            worker,
            playback: crate::playback::Control::default(),
            cost: Default::default(),
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
        let cost: Arc<std::sync::atomic::AtomicU64> = Default::default();
        let measured = cost.clone();
        let mut cache = crate::playback::Store::new(playback.clone());
        let worker = LatestWorker::new_cancellable(
            move |generation, job: Job, cancelled| {
                let start = Instant::now();
                let mut screen_ready = false;
                let frame = (|| {
                    cache.prepare(&job.plan, &job.inputs)?;
                    // The playback cache holds frames by time alone, so a free 3D view
                    // (which films the same time differently) bypasses it both ways.
                    // The reflections view is another picture of the same time, too.
                    let cacheable = job.request.region.is_none()
                        && job.request.scene_view.is_none()
                        && !job.request.scene_reflections;
                    let names = if cacheable {
                        cache.name(&renderer, &job.plan, job.request, &job.inputs)
                    } else {
                        (None, None)
                    };
                    let name = names.0;
                    // A picture kept as the screen shows it, when the request allows one.
                    let playing = if cacheable { job.look } else { None };
                    // Made only to be kept, and there is no room: nothing to do.
                    if job.background && cache.full() {
                        return Err(error("the store is full"));
                    }
                    // Made only to be kept: one that is kept already needs nothing.
                    if job.background
                        && let Some(look) = playing
                        && cache.has_picture(job.request.time, names, look)
                    {
                        return Err(error("kept already"));
                    }
                    if cacheable
                        && let Some(frame) = cache.lookup(&renderer, job.request.time, name)?
                    {
                        if job.background
                            && let Some(look) = playing
                        {
                            // Made to be kept: nothing kept gives way for it.
                            cache.keep_picture(
                                &renderer,
                                job.request.time,
                                names,
                                look,
                                &frame,
                                false,
                            );
                        }
                        return Ok(frame);
                    }
                    if let Some(look) = playing
                        && let Some(frame) =
                            cache.picture(&renderer, job.request.time, names, look)?
                    {
                        screen_ready = true;
                        return Ok(frame);
                    }
                    cache.make_room(
                        &renderer,
                        job.plan
                            .gpu_working_bytes()
                            .saturating_add(job.inputs.byte_size()),
                    )?;
                    let rendering = Instant::now();
                    let mut frame = render_gpu_job(&renderer, &job, cache.divisor(), cancelled);
                    if frame.is_ok() {
                        // Smoothed, so one slow first use of a kernel does not decide.
                        let took = rendering.elapsed().as_secs_f64() * 1000.0;
                        let old =
                            f64::from_bits(measured.load(std::sync::atomic::Ordering::Relaxed));
                        let new = if old == 0.0 {
                            took
                        } else {
                            old * 0.8 + took * 0.2
                        };
                        measured.store(new.to_bits(), std::sync::atomic::Ordering::Relaxed);
                    }
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
                        if let Some(look) = playing {
                            // Played frames push the oldest out; prepared ones never do.
                            cache.keep_picture(
                                &renderer,
                                job.request.time,
                                names,
                                look,
                                &frame,
                                !job.background,
                            );
                        }
                    }
                    Ok(frame)
                })();
                if job.last {
                    cache.settle();
                }
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
                    screen_ready,
                }
            },
            wake,
        )?;
        Ok(Self {
            worker,
            playback,
            cost,
        })
    }
}
impl<F: Send + 'static> PreviewScheduler<F> {
    pub fn playback_cache(&self) -> crate::playback::Control {
        self.playback.clone()
    }
    /// What rendering one frame has taken lately (smoothed), once anything was rendered.
    pub fn frame_cost(&self) -> Option<Duration> {
        let ms = f64::from_bits(self.cost.load(std::sync::atomic::Ordering::Relaxed));
        (ms > 0.0).then(|| Duration::from_secs_f64(ms / 1000.0))
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
            look: self.playback.playing(),
            background: false,
            last: false,
        })?;
        Ok(PreviewTicket {
            generation,
            revision,
        })
    }
    /// Frames to make when nothing is asked for the viewer, in this order, each kept as
    /// the screen shows it with `look` and never shown. It replaces any list given
    /// before. A frame asked for the viewer always goes first; a frame already being
    /// made is finished, so the viewer waits at most one render.
    pub fn prepare(
        &self,
        revision: u64,
        plan: Arc<Plan>,
        inputs: ImageInputs,
        times: impl IntoIterator<Item = veylvik_document::Time>,
        look: crate::screen::Look,
    ) -> Result<()> {
        let mut jobs: std::collections::VecDeque<Job> = times
            .into_iter()
            .map(|time| Job {
                revision,
                plan: plan.clone(),
                request: RenderRequest::full(time),
                inputs: inputs.clone(),
                look: Some(look),
                background: true,
                last: false,
            })
            .collect();
        if let Some(job) = jobs.back_mut() {
            job.last = true;
        }
        self.worker.set_background(jobs)
    }
    /// Hold back the frames given to [`PreviewScheduler::prepare`], or let them go on.
    pub fn pause_preparing(&self, paused: bool) -> Result<()> {
        self.worker.pause_background(paused)
    }
    /// Frames still to prepare, and how many have been prepared since the start.
    pub fn preparing(&self) -> (usize, u64) {
        self.worker.background()
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
    /// Work for when nothing is pending, oldest first; its results are not published.
    background: std::collections::VecDeque<I>,
    paused: bool,
    made: u64,
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
                background: Default::default(),
                paused: false,
                made: 0,
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
                        let (generation, input, background) = {
                            let mut lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                            while !lock.closed
                                && lock.pending.is_none()
                                && (lock.paused || lock.background.is_empty())
                            {
                                lock = state.ready.wait(lock).unwrap_or_else(|e| e.into_inner());
                            }
                            if lock.closed {
                                return;
                            }
                            // What is asked for the viewer always goes first.
                            match lock.pending.take() {
                                Some((generation, input)) => (generation, input, false),
                                None => {
                                    let input = lock.background.pop_front().unwrap();
                                    (lock.generation, input, true)
                                }
                            }
                        };
                        if background {
                            // Nobody waits for this one. It gives way, where the work can
                            // be left, as soon as something is asked for the viewer.
                            drop(render(generation, input, &mut || {
                                let lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                                lock.closed || lock.pending.is_some()
                            }));
                            let mut lock = state.state.lock().unwrap_or_else(|e| e.into_inner());
                            lock.made += 1;
                            let last = lock.background.is_empty();
                            drop(lock);
                            // Now and then, and at the end: whoever shows progress redraws.
                            if last || state.state.lock().is_ok_and(|l| l.made % 8 == 0) {
                                waking = true;
                                wake();
                                waking = false;
                            }
                            continue;
                        }
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
    fn set_background(&self, work: std::collections::VecDeque<I>) -> Result<()> {
        let mut lock = self
            .shared
            .state
            .lock()
            .map_err(|_| error("preview state poisoned"))?;
        if lock.closed {
            return Err(error("preview worker has stopped"));
        }
        lock.background = work;
        self.shared.ready.notify_one();
        Ok(())
    }
    fn pause_background(&self, paused: bool) -> Result<()> {
        let mut lock = self
            .shared
            .state
            .lock()
            .map_err(|_| error("preview state poisoned"))?;
        lock.paused = paused;
        self.shared.ready.notify_one();
        Ok(())
    }
    fn background(&self) -> (usize, u64) {
        self.shared
            .state
            .lock()
            .map_or((0, 0), |lock| (lock.background.len(), lock.made))
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
    #[test]
    #[ignore = "requires Vulkan"]
    fn while_playing_a_frame_seen_before_comes_from_the_kept_pictures() {
        let renderer = Arc::new(Renderer::new().unwrap());
        let scheduler = GpuPreviewScheduler::with_renderer(renderer.clone(), || {}).unwrap();
        let control = scheduler.playback_cache();
        // No room for frames as the renderer makes them: only pictures can answer.
        control.configure(crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: 1 << 20,
            // Whatever else this machine is doing while the test runs.
            memory_reserve_bytes: 0,
            ..Default::default()
        });
        let plan = Arc::new(Plan::compile(&veylvik_document::motion_demo()).unwrap());
        let render = |frame: i64| {
            let request = RenderRequest::full(Time::new(frame, 24).unwrap());
            scheduler
                .submit(1, plan.clone(), request, ImageInputs::default())
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(result) = scheduler.try_take().unwrap() {
                    return (result.frame.unwrap(), result.screen_ready);
                }
                assert!(Instant::now() < deadline, "preview timed out");
                thread::sleep(Duration::from_millis(5));
            }
        };
        let look = crate::screen::Look {
            exposure: 0.0,
            filmic: false,
        };
        // Not playing: nothing is kept and nothing is answered from pictures.
        assert!(!render(3).1 && !render(3).1);
        assert!(control.status().ready_frames.is_empty());
        control.set_playing(Some(look));
        let (first, ready) = render(3);
        assert!(!ready, "the first time a frame is the renderer's own");
        render(9);
        // Pictures are packed beside the worker: give the two a moment to arrive.
        thread::sleep(Duration::from_millis(300));
        let (again, ready) = render(3);
        assert!(ready, "a frame seen before was rendered again");
        assert_eq!(control.status().hits, 1);
        assert_eq!(control.status().ready_frames.len(), 2);
        assert!(control.status().picture_memory_bytes > 0);
        // The picture is the frame as the screen shows it: clipped at white, eight bits.
        let (was, is) = (
            renderer.read_frame(&first).unwrap().pixels,
            renderer.read_frame(&again).unwrap().pixels,
        );
        let encoded = |v: f32| {
            let v = v.clamp(0.0, 1.0);
            ((if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }) * 255.0)
                .round() as i32
        };
        let (mut checked, mut worst) = (0, 0);
        for (a, b) in was.iter().zip(&is) {
            if a[3] == 1.0 {
                checked += 1;
                worst = worst.max(
                    (0..3)
                        .map(|c| (encoded(a[c]) - encoded(b[c])).abs())
                        .max()
                        .unwrap(),
                );
            }
        }
        assert!(
            checked > 1000 && worst <= 1,
            "{checked} opaque pixels, worst {worst}"
        );
        // Playing stops: the frame is the renderer's own again.
        control.set_playing(None);
        assert!(!render(3).1);
        // Another look is not answered with pictures of this one.
        control.set_playing(Some(crate::screen::Look {
            exposure: 1.0,
            filmic: false,
        }));
        assert!(!render(3).1);
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn pictures_give_way_when_the_machine_is_short_of_memory() {
        let renderer = Arc::new(Renderer::new().unwrap());
        let scheduler = GpuPreviewScheduler::with_renderer(renderer, || {}).unwrap();
        let control = scheduler.playback_cache();
        // A reserve no machine has free: nothing may be kept in memory, whatever the limit.
        control.configure(crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: 1 << 30,
            memory_reserve_bytes: u64::MAX,
            ..Default::default()
        });
        control.set_playing(Some(crate::screen::Look::NEUTRAL));
        let plan = Arc::new(Plan::compile(&veylvik_document::motion_demo()).unwrap());
        for frame in [3, 9, 3] {
            scheduler
                .submit(
                    1,
                    plan.clone(),
                    RenderRequest::full(Time::new(frame, 24).unwrap()),
                    ImageInputs::default(),
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(result) = scheduler.try_take().unwrap() {
                    assert!(result.frame.is_ok() && !result.screen_ready);
                    break;
                }
                assert!(Instant::now() < deadline, "preview timed out");
                thread::sleep(Duration::from_millis(5));
            }
            thread::sleep(Duration::from_millis(150));
        }
        assert_eq!(control.status().picture_memory_bytes, 0);
        assert!(control.status().ready_frames.is_empty());
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn frames_are_prepared_behind_what_the_viewer_asks_for_and_never_shown() {
        let renderer = Arc::new(Renderer::new().unwrap());
        let scheduler = GpuPreviewScheduler::with_renderer(renderer, || {}).unwrap();
        let control = scheduler.playback_cache();
        control.configure(crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: 8 << 20,
            memory_reserve_bytes: 0,
            ..Default::default()
        });
        let plan = Arc::new(Plan::compile(&veylvik_document::motion_demo()).unwrap());
        let look = crate::screen::Look {
            exposure: 0.0,
            filmic: false,
        };
        let at = |frame: i64| Time::new(frame, 24).unwrap();
        let wait = |until: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !until() {
                assert!(Instant::now() < deadline, "timed out");
                thread::sleep(Duration::from_millis(5));
            }
        };
        // Held back, nothing is made.
        scheduler.pause_preparing(true).unwrap();
        scheduler
            .prepare(
                1,
                plan.clone(),
                ImageInputs::default(),
                (0..6).map(at),
                look,
            )
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(scheduler.preparing(), (6, 0));
        // What the viewer asks for is answered though six frames wait, and is the
        // renderer's own frame: the viewer is not playing.
        scheduler
            .submit(
                1,
                plan.clone(),
                RenderRequest::full(at(20)),
                ImageInputs::default(),
            )
            .unwrap();
        wait(&|| scheduler.preparing().1 == 0 && control.status().revision == 1);
        let shown = loop {
            if let Some(result) = scheduler.try_take().unwrap() {
                break result;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(shown.frame.is_ok() && !shown.screen_ready);
        assert_eq!(scheduler.preparing(), (6, 0));
        // Let go, all six are made and kept, and none of them is handed to the viewer.
        scheduler.pause_preparing(false).unwrap();
        wait(&|| scheduler.preparing() == (0, 6));
        assert!(scheduler.try_take().unwrap().is_none());
        // Asking for them again makes nothing: they are kept (the list runs through).
        scheduler
            .prepare(
                1,
                plan.clone(),
                ImageInputs::default(),
                (0..6).map(at),
                look,
            )
            .unwrap();
        wait(&|| scheduler.preparing() == (0, 12));
        // Pictures are packed beside the worker: the last ones arrive a moment later.
        thread::sleep(Duration::from_millis(300));
        // While playing, such a frame comes from the kept pictures.
        control.set_playing(Some(look));
        scheduler
            .submit(1, plan, RenderRequest::full(at(4)), ImageInputs::default())
            .unwrap();
        let played = loop {
            if let Some(result) = scheduler.try_take().unwrap() {
                break result;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(played.screen_ready, "a prepared frame was rendered again");
        assert_eq!(control.status().ready_frames.len(), 6);
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_composition_of_a_hosts_media_is_prepared_and_kept() {
        use veylvik_render::{HostFrame, HostMedia, HostPixels, HostSource, SourceInfo};
        const W: u32 = 64;
        const H: u32 = 36;
        /// A host's clip: a fresh buffer for every frame, a little brighter each frame.
        struct Clip(wgpu::Device, wgpu::Queue);
        impl HostMedia for Clip {
            fn info(&self) -> SourceInfo {
                SourceInfo {
                    width: W,
                    height: H,
                    duration: None,
                    frame_rate: None,
                    frames: None,
                    description: "a host's clip".into(),
                }
            }
            // Its frame number (times are kept reduced: 2/24 is 1/12).
            fn identity(&self, time: Time) -> u64 {
                (time.ticks as f64 * 24.0 / f64::from(time.rate)).round() as u64
            }
            fn frame(&self, time: Time, _: Option<[u32; 2]>) -> veylvik_render::Result<HostFrame> {
                let value = self.identity(time) as f32 / 10.0;
                let bytes: Vec<u8> = (0..W * H)
                    .flat_map(|_| [value, 0.2, 0.1, 1.0])
                    .flat_map(f32::to_le_bytes)
                    .collect();
                let buffer = self.0.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("a host's frame"),
                    size: bytes.len() as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.1.write_buffer(&buffer, 0, &bytes);
                Ok(HostFrame {
                    buffer,
                    width: W,
                    height: H,
                    pixels: HostPixels::rec709(true),
                })
            }
        }
        let renderer = Arc::new(Renderer::new().unwrap());
        let clip = HostSource::new(
            Clip(renderer.device().clone(), renderer.queue().clone()),
            1 << 20,
        );
        let scheduler = GpuPreviewScheduler::with_renderer(renderer, || {}).unwrap();
        let control = scheduler.playback_cache();
        control.configure(crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: 8 << 20,
            memory_reserve_bytes: 0,
            ..Default::default()
        });
        let rate = Time::new(24, 1).unwrap();
        let composition = veylvik_document::Composition::from_host_footage(
            "clip",
            "Handheld",
            None,
            [W, H],
            rate,
            12,
        )
        .unwrap();
        let plan = Arc::new(Plan::compile(&composition).unwrap());
        let mut inputs = ImageInputs::default();
        inputs.insert_source("clip", Arc::new(clip));
        let look = crate::screen::Look {
            exposure: 0.0,
            filmic: false,
        };
        let at = |frame: i64| Time::at_frame(frame, rate).unwrap();
        scheduler
            .prepare(1, plan, inputs, (0..6).map(at), look)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while scheduler.preparing() != (0, 6) {
            assert!(
                Instant::now() < deadline,
                "not prepared: {:?}",
                scheduler.preparing()
            );
            thread::sleep(Duration::from_millis(5));
        }
        // Pictures are packed beside the worker: the last ones arrive a moment later.
        while control.status().ready_frames.len() < 6 {
            assert!(
                Instant::now() < deadline,
                "kept: {:?}",
                control.status().ready_frames
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn the_next_session_plays_what_the_last_one_kept() {
        let directory = std::env::temp_dir().join(format!(
            "veylvik-sessions-test-{}",
            veylvik_document::NodeId::new_v4()
        ));
        let settings = crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: 8 << 20,
            picture_disk_bytes: 64 << 20,
            memory_reserve_bytes: 0,
            directory: Some(directory.clone()),
            ..Default::default()
        };
        let look = crate::screen::Look {
            exposure: 0.0,
            filmic: false,
        };
        let at = |frame: i64| Time::new(frame, 24).unwrap();
        let plan = || Arc::new(Plan::compile(&veylvik_document::motion_demo()).unwrap());
        let session = || {
            let renderer = Arc::new(Renderer::new().unwrap());
            let scheduler = GpuPreviewScheduler::with_renderer(renderer, || {}).unwrap();
            scheduler.playback_cache().configure(settings.clone());
            scheduler
        };
        let played = |scheduler: &GpuPreviewScheduler, frame: i64| {
            scheduler.playback_cache().set_playing(Some(look));
            scheduler
                .submit(
                    1,
                    plan(),
                    RenderRequest::full(at(frame)),
                    ImageInputs::default(),
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(result) = scheduler.try_take().unwrap() {
                    assert!(result.frame.is_ok());
                    return result.screen_ready;
                }
                assert!(Instant::now() < deadline, "timed out");
                thread::sleep(Duration::from_millis(5));
            }
        };
        // One session prepares four frames, and ends.
        let first = session();
        first
            .prepare(1, plan(), ImageInputs::default(), (0..4).map(at), look)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while first.preparing() != (0, 4) {
            assert!(Instant::now() < deadline, "timed out");
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(400));
        drop(first);
        // Another, with a renderer of its own and the composition compiled anew, plays
        // one of them from disk without rendering it; a frame nobody kept is rendered.
        let second = session();
        assert!(
            played(&second, 2),
            "a frame kept by the last session was rendered again"
        );
        assert!(!played(&second, 30));
        // Emptying the store is the artist's word, and takes the files with it.
        second.playback_cache().clear();
        assert!(!played(&second, 2));
        drop(second);
        let left: usize = std::fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("kept-"))
            .map(|entry| std::fs::read_dir(entry.path()).unwrap().count())
            .sum();
        // Only what the second session kept after the clear: the two frames it played.
        assert!(left <= 2, "{left} files left after clearing");
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn preparing_never_pushes_out_what_is_kept_and_rests_when_the_store_is_full() {
        let renderer = Arc::new(Renderer::new().unwrap());
        let scheduler = GpuPreviewScheduler::with_renderer(renderer, || {}).unwrap();
        let control = scheduler.playback_cache();
        let plan = Arc::new(Plan::compile(&veylvik_document::motion_demo()).unwrap());
        let look = crate::screen::Look {
            exposure: 0.0,
            filmic: false,
        };
        // Moments far enough apart to be different pictures.
        let at = |step: i64| Time::new(step * 5, 24).unwrap();
        let settle = |made: u64| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while scheduler.preparing() != (0, made) {
                assert!(Instant::now() < deadline, "timed out");
                thread::sleep(Duration::from_millis(5));
            }
            thread::sleep(Duration::from_millis(300));
        };
        // Find what one picture of this composition takes, with room for many.
        control.configure(crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: 64 << 20,
            memory_reserve_bytes: 0,
            ..Default::default()
        });
        scheduler
            .prepare(1, plan.clone(), ImageInputs::default(), [at(0)], look)
            .unwrap();
        settle(1);
        // Asked for again it costs nothing, and the store's figures by then include it
        // (pictures are packed beside the worker and arrive a moment later).
        scheduler
            .prepare(1, plan.clone(), ImageInputs::default(), [at(0)], look)
            .unwrap();
        settle(2);
        let one = control.status().picture_memory_bytes;
        assert!(one > 0);
        // Room for about three, and twelve asked for.
        control.configure(crate::playback::Settings {
            gpu_bytes: 0,
            picture_memory_bytes: one * 3 + one / 2,
            memory_reserve_bytes: 0,
            ..Default::default()
        });
        scheduler
            .prepare(
                1,
                plan.clone(),
                ImageInputs::default(),
                (0..12).map(at),
                look,
            )
            .unwrap();
        settle(14);
        // The first ones are kept and stay kept: later ones did not push them out.
        scheduler
            .prepare(1, plan, ImageInputs::default(), [at(0)], look)
            .unwrap();
        settle(15);
        let status = control.status();
        assert!(status.full, "the store did not say it is full");
        let kept: Vec<i64> = status
            .ready_frames
            .iter()
            .map(|t| t.ticks * 24 / i64::from(t.rate) / 5)
            .collect();
        assert!(
            kept.contains(&0) && kept.contains(&1),
            "the first pictures were pushed out: {kept:?}"
        );
        assert!(kept.len() <= 4 && !kept.contains(&11), "{kept:?}");
        assert!(status.picture_memory_bytes <= one * 3 + one / 2);
    }
}
