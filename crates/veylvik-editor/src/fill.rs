//! Tells the preview engine which frames of the playback range to prepare: those not kept yet, from the playhead on. The engine makes them behind whatever the viewer asks for, so this only says what and when to hold back; it never renders or waits itself.
use crate::Studio;
use eframe::egui;
use std::time::Duration;

#[derive(Default)]
pub(crate) struct Fill {
    /// What the engine was last told to prepare: the revision, the range, the look and
    /// where the playhead was. Told again only when one of these changes.
    told: Option<(u64, (i64, i64), u64, i64)>,
    /// Whether the engine is preparing just now, for whoever shows it.
    pub active: bool,
    /// When missing frames were last asked for again.
    again: Option<std::time::Instant>,
    /// The last measuring point and frames prepared a second, for the time left.
    since: Option<Since>,
    rate: Option<f64>,
    /// Windows measured so far: the time left is said once two are.
    windows: u32,
}
impl Fill {
    /// Waiting to play is over: the next wait measures afresh.
    pub(crate) fn clear_since(&mut self) {
        self.since = None;
        self.rate = None;
        self.windows = 0;
    }
}
impl Studio {
    /// The store was emptied: prepare again.
    pub(crate) fn fill_again(&mut self) {
        self.fill.told = None;
    }
    pub(crate) fn fill_tick(&mut self, ctx: &egui::Context) {
        self.fill.active = false;
        let values = &self.preferences.values;
        let wanted =
            values.fill_while_idle && values.played_memory_mib + values.played_disk_mib > 0;
        // Held back while the picture on screen comes from elsewhere than the saved
        // composition, or while frames are already being made in order.
        let hold = !wanted
            // Playing from the store holds preparing back; waiting to play needs it.
            || (self.playing && self.buffering.is_none())
            || self.playback_controls.warming.is_some()
            || self.draft_dirty
            || self.error
            || self.files.busy()
            || self.scene_view.is_some()
            || self.scene_reflections
            || self.key_timeline.preview_commands().is_some();
        let Some(scheduler) = &self.scheduler else {
            return;
        };
        let _ = scheduler.pause_preparing(hold);
        if hold {
            return;
        }
        let view = self.effective_view();
        let look = veylvik_preview::screen::Look {
            exposure: if view.exposure.is_finite() {
                view.exposure.clamp(-16.0, 16.0)
            } else {
                0.0
            },
            filmic: view.filmic,
        };
        let doc = self.project.document.snapshot();
        let (rate, count) = (doc.frame_rate, doc.frame_count);
        let range = self.playback_controls.range(count);
        let revision = self.project.document.revision();
        let from = self.project.frame.clamp(range.0, range.1);
        let key = (
            revision,
            range,
            u64::from(look.exposure.to_bits()) | (u64::from(look.filmic) << 32),
            from,
        );
        let (left, _) = scheduler.preparing();
        self.fill.active = left > 0;
        // Everything asked for was made and some frames are still not kept (one could
        // not be kept while playing went on, say): ask for those again, now and then.
        let status = scheduler.playback_cache().status();
        let wanted = (range.1 - range.0 + 1).max(0) as usize;
        if self.fill.told == Some(key)
            && left == 0
            && !status.full
            && status.ready_frames.len() < wanted
            && self
                .fill
                .again
                .is_none_or(|at| at.elapsed() > Duration::from_secs(2))
        {
            self.fill.again = Some(std::time::Instant::now());
            self.fill.told = None;
        }
        if self.fill.told == Some(key) {
            if left > 0 {
                // Keep the cached marks moving while frames arrive.
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            return;
        }
        let ready: std::collections::BTreeSet<i64> = scheduler
            .playback_cache()
            .status()
            .ready_frames
            .iter()
            .filter_map(|time| crate::key_timeline::frame(*time, rate, count))
            .collect();
        // From the playhead to the end of the range, then from its start.
        let times: Vec<_> = (from..=range.1)
            .chain(range.0..from)
            .filter(|frame| !ready.contains(frame))
            .filter_map(|frame| veylvik_document::Time::at_frame(frame, rate).ok())
            .collect();
        let inputs = self.project.inputs.clone();
        let Ok(plan) = self.project.plan() else {
            return;
        };
        let Some(scheduler) = &self.scheduler else {
            return;
        };
        if scheduler
            .prepare(revision, plan, inputs, times, look)
            .is_ok()
        {
            self.fill.told = Some(key);
            self.fill.active = true;
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

/// Why playing waits, for the panel over the viewer.
pub(crate) enum Waiting {
    /// Frames ahead are being prepared; `rate` is frames prepared a second when known.
    Preparing {
        have: usize,
        wanted: usize,
        cost: Option<Duration>,
        frame: Duration,
        size: [u32; 2],
        left: Option<Duration>,
    },
    /// What would be played needs more room than the store has.
    TooBig { needed: u64, allowed: u64 },
}
/// Frames prepared since playing began to wait, to tell how long is left.
#[derive(Clone, Copy)]
struct Since {
    at: std::time::Instant,
    have: usize,
}
impl Studio {
    /// While playing: whether the playhead has to hold. It holds only when it must: the
    /// frames ahead are not all prepared, and this composition renders slower than real
    /// time (with a margin), so playing live would skip frames. Says why on the viewer.
    pub(crate) fn hold_for_frames(&mut self, ctx: &egui::Context) -> bool {
        let values = &self.preferences.values;
        let off = !values.wait_for_frames
            || !values.fill_while_idle
            || values.played_memory_mib + values.played_disk_mib == 0
            || self.play_now
            || self.draft_dirty
            || self.error
            || self.scene_view.is_some()
            || self.scene_reflections;
        let allowed = (values.played_memory_mib + values.played_disk_mib) * 1024 * 1024;
        let looping = values.loop_playback;
        let Some(scheduler) = self.scheduler.as_ref().filter(|_| !off) else {
            self.buffering = None;
            self.waiting = None;
            return false;
        };
        let doc = self.project.document.snapshot();
        let (rate, count, size) = (doc.frame_rate, doc.frame_count, [doc.width, doc.height]);
        let frame = Duration::from_secs_f64(f64::from(rate.rate.max(1)) / rate.ticks.max(1) as f64);
        let range = self.playback_controls.range(count);
        let status = scheduler.playback_cache().status();
        let ready: std::collections::BTreeSet<i64> = status
            .ready_frames
            .iter()
            .filter_map(|time| crate::key_timeline::frame(*time, rate, count))
            .collect();
        // What will be played: from here to the end, or the whole range when looping.
        let from = if looping {
            range.0
        } else {
            self.project.frame.clamp(range.0, range.1)
        };
        let wanted = (range.1 - from + 1).max(0) as usize;
        let have = ready.range(from..=range.1).count();
        let cost = scheduler.frame_cost();
        // All there, or live is fast enough: play at once, nothing to explain.
        let fast = cost.is_some_and(|cost| cost.as_secs_f64() < frame.as_secs_f64() * 0.8);
        if have >= wanted || fast {
            self.buffering = None;
            self.waiting = None;
            return false;
        }
        // Too long for the store: say so and let the artist choose; waiting would not end.
        let kept = status.picture_memory_bytes + status.picture_disk_bytes;
        let each = if status.ready_frames.is_empty() || kept == 0 {
            u64::from(size[0]) * u64::from(size[1]) * 2
        } else {
            kept / status.ready_frames.len() as u64
        };
        let needed = each.saturating_mul(wanted as u64);
        if needed > allowed || status.full {
            self.buffering = Some(None);
            self.waiting = Some(Waiting::TooBig { needed, allowed });
            if !self.error {
                self.status = "Playing waits: this range needs more room than the store has".into();
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return true;
        }
        // Nothing left being prepared and frames still missing for a while: waiting
        // longer would be for nothing, so play as frames render and say why.
        let (left, _) = scheduler.preparing();
        let stuck = match self.buffering {
            Some(Some(since)) if left == 0 => since.elapsed() > Duration::from_millis(1500),
            _ => false,
        };
        if stuck {
            self.buffering = None;
            self.waiting = None;
            if !self.error {
                self.status = "Playing as frames render: this range could not be prepared".into();
            }
            return false;
        }
        self.buffering = Some(match self.buffering {
            Some(since) if left == 0 => since.or(Some(std::time::Instant::now())),
            _ if left == 0 => Some(std::time::Instant::now()),
            _ => None,
        });
        // Measured over the last few seconds, so the start (and what other work took
        // then) does not decide; until then from what a frame costs to make.
        let since = *self.fill.since.get_or_insert(Since {
            at: std::time::Instant::now(),
            have,
        });
        let elapsed = since.at.elapsed().as_secs_f64();
        if elapsed >= 3.0 {
            let now = have.saturating_sub(since.have) as f64 / elapsed;
            self.fill.rate = Some(self.fill.rate.map_or(now, |rate| rate * 0.5 + now * 0.5));
            self.fill.windows += 1;
            self.fill.since = Some(Since {
                at: std::time::Instant::now(),
                have,
            });
        }
        let missing = (wanted - have) as f64;
        // Said only once measured over two windows: a first guess jumps about.
        let left = match self.fill.rate {
            Some(rate) if rate > 0.0 && self.fill.windows >= 2 => {
                Some(Duration::from_secs_f64(missing / rate))
            }
            _ => None,
        };
        self.waiting = Some(Waiting::Preparing {
            have,
            wanted,
            cost,
            frame,
            size,
            left,
        });
        if !self.error {
            self.status = format!("Preparing playback: {have} of {wanted} frames");
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        true
    }
    /// The panel over the viewer while playing waits: why, how far, and the ways out.
    pub(crate) fn wait_panel(&mut self, ctx: &egui::Context) {
        let (Some(waiting), Some(canvas)) = (&self.waiting, self.viewer_canvas) else {
            return;
        };
        if !self.playing {
            return;
        }
        let mut choice = None;
        egui::Area::new(egui::Id::new("playback-waits"))
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .show(ctx, |ui| {
                // Lifted off the picture: a picture is often black, and so is the panel.
                let palette = crate::theme::palette(ctx);
                let frame = crate::design::panel(ctx)
                    .inner_margin(18.0)
                    .fill(crate::theme::mix(palette.panel, palette.text, 0.06))
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::mix(palette.border, palette.text, 0.25)))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(160),
                    });
                frame.show(ui, |ui| {
                    // One width whatever the numbers, so nothing moves while they change.
                    ui.set_width(440.0);
                    match waiting {
                        Waiting::Preparing { have, wanted, cost, frame, size, left } => {
                            ui.label(egui::RichText::new("Preparing smooth playback").size(17.0).strong());
                            ui.add_space(4.0);
                            ui.label(match cost {
                                Some(cost) => format!(
                                    "At {} x {} a frame of this composition takes about {:.0} ms to make, and playing in real time allows {:.0} ms. The frames are made first, then every one is played on time.",
                                    size[0], size[1], cost.as_secs_f64() * 1000.0, frame.as_secs_f64() * 1000.0
                                ),
                                None => "The frames are made first, then every one is played on time.".into(),
                            });
                            ui.add_space(8.0);
                            progress(ui, *have as f32 / (*wanted).max(1) as f32, &palette);
                            ui.add_space(6.0);
                            // Frames on the left, time on the right, each in its own place.
                            ui.horizontal(|ui| {
                                ui.set_width(440.0);
                                ui.label(format!("{have} of {wanted} frames"));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(match left {
                                        Some(left) => format!("about {} left", spoken(*left)),
                                        None => "working out the time left".into(),
                                    });
                                });
                            });
                        }
                        Waiting::TooBig { needed, allowed } => {
                            ui.label(egui::RichText::new("This range is too long to play smoothly").size(17.0).strong());
                            ui.add_space(4.0);
                            ui.label(format!(
                                "Playing it smoothly needs about {} of prepared frames, and the store of played pictures allows {}. Set a shorter In and Out range, or raise the limits.",
                                gigabytes(*needed), gigabytes(*allowed)
                            ));
                        }
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if matches!(waiting, Waiting::TooBig { .. }) && crate::design::primary(ui, "Open Preferences").clicked() {
                            choice = Some(2);
                        }
                        if ui.button("Play now, skipping frames").clicked() {
                            choice = Some(1);
                        }
                        if ui.button("Stop").clicked() {
                            choice = Some(0);
                        }
                    });
                });
            });
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            choice = Some(0);
        }
        match choice {
            Some(0) => self.toggle_play(),
            Some(1) => {
                self.play_now = true;
                self.waiting = None;
                self.buffering = None;
            }
            Some(2) => {
                self.toggle_play();
                self.preferences.begin_at_playback();
            }
            _ => {}
        }
    }
}
/// A duration as people say it: "40 s", "1 min 20 s".
/// Rounded so it changes in calm steps: 5 s under a minute, 10 s above.
fn spoken(time: Duration) -> String {
    let seconds = time.as_secs_f64();
    if seconds < 8.0 {
        return "a few seconds".into();
    }
    if seconds < 60.0 {
        return format!("{} s", ((seconds / 5.0).ceil() * 5.0) as u64);
    }
    let rounded = ((seconds / 10.0).ceil() * 10.0) as u64;
    match rounded % 60 {
        0 => format!("{} min", rounded / 60),
        rest => format!("{} min {rest} s", rounded / 60),
    }
}
/// A progress bar in the colours of Veylvik's mark, violet to teal along its length,
/// on a track that shows on any panel.
fn progress(ui: &mut egui::Ui, share: f32, palette: &crate::theme::Palette) {
    const VIOLET: egui::Color32 = egui::Color32::from_rgb(0x9c, 0x8c, 0xff);
    const TEAL: egui::Color32 = egui::Color32::from_rgb(0x5e, 0xea, 0xd4);
    let (bar, _) = ui.allocate_exact_size(egui::vec2(440.0, 10.0), egui::Sense::hover());
    let painter = ui.painter();
    let radius = bar.height() / 2.0;
    painter.rect_filled(
        bar,
        radius,
        crate::theme::mix(palette.panel, palette.text, 0.16),
    );
    let end = bar.left() + radius * 2.0 + (bar.width() - radius * 2.0) * share.clamp(0.0, 1.0);
    let colour = |x: f32| {
        crate::theme::mix(
            VIOLET,
            TEAL,
            ((x - bar.left()) / bar.width()).clamp(0.0, 1.0),
        )
    };
    // Round ends as circles, the run between them as a gradient.
    let (left, right) = (bar.left() + radius, end - radius);
    let mut mesh = egui::Mesh::default();
    for (x, y) in [
        (left, bar.top()),
        (right, bar.top()),
        (right, bar.bottom()),
        (left, bar.bottom()),
    ] {
        mesh.colored_vertex(egui::pos2(x, y), colour(x));
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.circle_filled(egui::pos2(left, bar.center().y), radius, colour(left));
    if right > left {
        painter.add(mesh);
    }
    painter.circle_filled(
        egui::pos2(right.max(left), bar.center().y),
        radius,
        colour(right.max(left)),
    );
}
fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}
