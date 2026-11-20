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
            || self.playing
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
