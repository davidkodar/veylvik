//! Session playback range and GPU preview reduction; never changes export settings.
#[derive(Default)]
pub struct Playback {
    pub enabled: bool,
    pub first: i64,
    pub last: Option<i64>,
    pub resolution: u32,
    pub warming: Option<Warm>,
}
#[derive(Clone, Copy)]
pub struct Warm {
    pub end: i64,
    pub restore: i64,
}
impl Playback {
    pub fn range(&self, count: u32) -> (i64, i64) {
        let end = i64::from(count.saturating_sub(1));
        if self.enabled {
            (
                self.first.clamp(0, end),
                self.last
                    .unwrap_or(end)
                    .clamp(self.first.clamp(0, end), end),
            )
        } else {
            (0, end)
        }
    }
    pub fn set_in(&mut self, frame: i64, count: u32) {
        self.enabled = true;
        self.first = frame.clamp(0, i64::from(count.saturating_sub(1)));
        if self.last.is_some_and(|v| v < self.first) {
            self.last = Some(self.first);
        }
    }
    pub fn set_out(&mut self, frame: i64, count: u32) {
        self.enabled = true;
        let frame = frame.clamp(0, i64::from(count.saturating_sub(1)));
        self.last = Some(frame);
        self.first = self.first.min(frame);
    }
    pub fn divisor(&self) -> u32 {
        match self.resolution {
            2 => 2,
            4 => 4,
            _ => 1,
        }
    }
}
pub fn frame(start: i64, elapsed: i64, range: (i64, i64), looping: bool) -> (i64, bool) {
    let (first, last) = range;
    let length = (last - first + 1).max(1);
    let offset = start.clamp(first, last) - first;
    let position = offset.saturating_add(elapsed.max(0));
    if looping {
        (first + position % length, false)
    } else {
        (first + position.min(length - 1), position >= length - 1)
    }
}
/// Playing every frame: where the clock says to be (`clock`, as [`frame`] gives it), seen
/// from the frame shown now. When the clock has gone past the next frame, playing is
/// behind: it goes to the next frame (and its end flag) rather than skip; true then.
pub fn every_frame(
    clock: (i64, bool),
    shown: i64,
    range: (i64, i64),
    looping: bool,
) -> ((i64, bool), bool) {
    let next = frame(shown, 1, range, looping);
    if clock.0 != shown && clock.0 != next.0 {
        (next, true)
    } else {
        (clock, false)
    }
}
#[cfg(test)]
mod every_frame_tests {
    use super::*;
    #[test]
    fn playing_every_frame_steps_one_at_a_time_when_behind() {
        let range = (0, 9);
        // On time: where the clock says, nothing held back.
        assert_eq!(
            every_frame((4, false), 3, range, false),
            ((4, false), false)
        );
        assert_eq!(
            every_frame((3, false), 3, range, false),
            ((3, false), false)
        );
        // Behind: the next frame, not the one the clock reached.
        assert_eq!(every_frame((7, false), 3, range, false), ((4, false), true));
        // At the end of a range that does not loop, the last frame ends it.
        assert_eq!(every_frame((9, true), 7, range, false), ((8, false), true));
        assert_eq!(every_frame((9, true), 8, range, false), ((9, true), false));
        // Looping: from the last frame the next is the first.
        assert_eq!(every_frame((2, false), 9, range, true), ((0, false), true));
        assert_eq!(every_frame((0, false), 9, range, true), ((0, false), false));
    }
}
impl crate::Studio {
    pub(super) fn playback_toolbar(&mut self, ui: &mut eframe::egui::Ui) {
        use eframe::egui;
        let count = self.project.document.snapshot().frame_count;
        // Timecode when that is how positions are shown, else frame numbers.
        let shown = (self.preferences.values.time_display == crate::timecode::Display::Timecode)
            .then(|| self.project.document.snapshot().frame_rate);
        ui.horizontal_wrapped(|ui| {
            let mut changed = ui.checkbox(&mut self.playback_controls.enabled, "Range")
                .on_hover_text("Limit playback and Cache range to In/Out. Export range is unchanged.").changed();
            let mut first = self.playback_controls.first.clamp(0, i64::from(count.saturating_sub(1)));
            let mut last = self.playback_controls.last.unwrap_or(i64::from(count.saturating_sub(1)))
                .clamp(first, i64::from(count.saturating_sub(1)));
            if frame_control(ui, &mut first, 0..=last, "In ", shown).on_hover_text("Inclusive first playback frame").changed() {
                self.playback_controls.first = first;
                self.playback_controls.enabled = true;
                changed = true;
            }
            if frame_control(ui, &mut last, first..=i64::from(count.saturating_sub(1)), "Out ", shown)
                .on_hover_text("Inclusive last playback frame").changed() {
                self.playback_controls.last = Some(last);
                self.playback_controls.enabled = true;
                changed = true;
            }
            ui.menu_button("Set", |ui| {
                if ui.button("Set In at playhead (I)").clicked() {
                    self.playback_controls.set_in(self.project.frame, count); changed = true; ui.close();
                }
                if ui.button("Set Out at playhead (O)").clicked() {
                    self.playback_controls.set_out(self.project.frame, count); changed = true; ui.close();
                }
                if ui.button("Reset to full composition").clicked() {
                    self.playback_controls.enabled = false; self.playback_controls.first = 0;
                    self.playback_controls.last = None; changed = true; ui.close();
                }
            }).response.on_hover_text("Set playback range from the current frame, or reset it");
            if changed { self.transport.pause(); self.playback_controls.warming = None; }
            let old = self.playback_controls.divisor();
            egui::ComboBox::from_id_salt("preview-resolution").width(110.)
                .selected_text(match old {2 => "Preview: Half", 4 => "Preview: Quarter", _ => "Preview: Full"})
                .show_ui(ui, |ui| {
                    for (value, label) in [(1,"Full resolution"), (2,"Half resolution"), (4,"Quarter resolution")] {
                        ui.selectable_value(&mut self.playback_controls.resolution, value, label);
                    }
                }).response.on_hover_text("Cache and display completed previews at reduced resolution. Effects evaluate at full project resolution; exports stay unchanged.");
            if old != self.playback_controls.divisor() {
                self.transport.pause();
                if let Some(s) = &self.scheduler { s.playback_cache().set_resolution(self.playback_controls.divisor()); }
                self.request_preview();
            }
            let range = self.playback_controls.range(count);
            if self.playback_controls.warming.is_some() {
                if ui.button("Stop caching").clicked() { self.request_preview(); }
            } else if ui.add_enabled(!self.files.busy() && range.1-range.0 < 10000,
                egui::Button::new("Cache range"))
                .on_hover_text("Render every frame in the playback range (up to 10,000). Budgets may retain only part of it. Choose Half/Quarter or enable disk cache for longer ranges.")
                .clicked() {
                self.cache_range();
            }
            if ui.add(crate::icons::Button::new(crate::icons::Icon::Reset, "Clear playback cache"))
                .on_hover_text("Release cached playback frames and session disk files").clicked() {
                if let Some(s) = &self.scheduler { s.playback_cache().clear(); }
                self.fill_again();
                self.request_preview();
            }
            if let Some(scheduler) = &self.scheduler {
                let cache = scheduler.playback_cache().status();
                let doc = self.project.document.snapshot();
                let valid = !self.draft_dirty && cache.revision == self.project.document.revision()
                    && cache.output == self.project.preview.unwrap_or(doc.output)
                    && cache.size == [doc.width.div_ceil(self.playback_controls.divisor()), doc.height.div_ceil(self.playback_controls.divisor())];
                let frames: std::collections::BTreeSet<_> = cache.gpu_frames.iter().chain(&cache.disk_frames).chain(&cache.ready_frames)
                    .filter_map(|t|crate::key_timeline::frame(*t, doc.frame_rate, count))
                    .filter(|f| valid && *f>=range.0 && *f<=range.1).collect();
                ui.add_sized([166.,22.], egui::Label::new(egui::RichText::new(format!("{:>4}/{:<4} {}",frames.len(),range.1-range.0+1,if cache.full {"store full"} else {"cached"})).monospace().weak()).truncate())
                    .on_hover_text(format!("{}Played pictures kept: {:.0} MiB in memory, {:.0} MiB on disk; found again {} times. The green line under the ruler marks the frames that are kept. Limits are in Preferences, under Played pictures.",if cache.full {"The store is full: frames are no longer prepared in the background. Raise its limits, or clear it. "} else {""},cache.picture_memory_bytes as f64/1048576.,cache.picture_disk_bytes as f64/1048576.,cache.hits));
                if let Some(warning) = cache.warning {
                    ui.colored_label(ui.visuals().warn_fg_color,"Cache notice").on_hover_text(warning);
                }
            }
        });
    }
}
fn frame_control(
    ui: &mut eframe::egui::Ui,
    value: &mut i64,
    range: std::ops::RangeInclusive<i64>,
    prefix: &str,
    timecode: Option<veylvik_document::Time>,
) -> eframe::egui::Response {
    use eframe::egui;
    let width = if timecode.is_some() { 132. } else { 86. };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.), egui::Sense::hover());
    let mut field = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    field.set_clip_rect(rect.intersect(ui.clip_rect()));
    field.add_sized(
        rect.size(),
        match timecode {
            Some(rate) => egui::DragValue::new(value)
                .range(range)
                .prefix(prefix)
                .custom_formatter(move |v, _| crate::timecode::format(v as i64, rate))
                .custom_parser(move |text| crate::timecode::parse(text, rate).map(|f| f as f64)),
            None => egui::DragValue::new(value).range(range).prefix(prefix),
        },
    )
}
impl crate::Studio {
    /// Render every frame of the playback range once, in order, so it plays from the
    /// store afterwards.
    pub(crate) fn cache_range(&mut self) {
        let count = self.project.document.snapshot().frame_count;
        let range = self.playback_controls.range(count);
        if self.playback_controls.warming.is_some()
            || self.files.busy()
            || range.1 - range.0 >= 10000
            || !self.apply_draft()
        {
            return;
        }
        self.transport.pause();
        self.playback_controls.warming = Some(Warm {
            end: range.1,
            restore: self.project.frame,
        });
        self.project.frame = range.0;
        self.request_preview_inner();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_respects_inclusive_ranges_and_single_frames() {
        assert_eq!(frame(28, 4, (20, 29), true), (22, false));
        assert_eq!(frame(28, 4, (20, 29), false), (29, true));
        assert_eq!(frame(0, 0, (20, 29), true), (20, false));
        assert_eq!(frame(100, 0, (20, 29), false), (29, true));
        assert_eq!(frame(3, 100, (7, 7), true), (7, false));
        let mut p = Playback::default();
        p.set_out(15, 100);
        p.set_in(25, 100);
        assert_eq!(p.range(100), (25, 25));
        assert_eq!(p.range(10), (9, 9));
    }
}
