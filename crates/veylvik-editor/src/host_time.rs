//! The playhead and the working range as a host sees them, in composition time: a host that shows the editor as a page of its own carries its timeline's place in and out, and the clip's used part becomes the working range.
use veylvik_document::Time;

impl crate::Studio {
    /// The frame nearest to `time`, within the composition.
    fn frame_at(&self, time: Time) -> i64 {
        let doc = self.project.document.snapshot();
        let (fps, last) = (doc.frame_rate, i64::from(doc.frame_count.saturating_sub(1)));
        if time.rate == 0 || fps.rate == 0 || fps.ticks <= 0 {
            return 0;
        }
        let numerator = i128::from(time.ticks) * i128::from(fps.ticks);
        let denominator = i128::from(time.rate) * i128::from(fps.rate);
        let nearest = (2 * numerator + denominator).div_euclid(2 * denominator);
        nearest.clamp(0, i128::from(last)) as i64
    }
    fn time_of(&self, frame: i64) -> Time {
        let rate = self.project.document.snapshot().frame_rate;
        Time::at_frame(frame, rate).unwrap_or(Time { ticks: 0, rate: 1 })
    }
    /// Move the playhead to `time` and stop playing; false while an edit is mid-gesture.
    pub(crate) fn host_set_time(&mut self, time: Time) -> bool {
        if !self.apply_draft() {
            return false;
        }
        self.playing = false;
        let frame = self.frame_at(time);
        self.project.frame = frame;
        // The host's own move is not reported back to it.
        self.reported_frame = frame;
        self.request_preview();
        true
    }
    pub(crate) fn host_time(&self) -> Time {
        self.time_of(self.project.frame)
    }
    /// Where the playhead went since the last call, when it moved: by the artist, by
    /// playing, never by [`Self::host_set_time`].
    pub(crate) fn host_take_time_change(&mut self) -> Option<Time> {
        (self.project.frame != self.reported_frame).then(|| {
            self.reported_frame = self.project.frame;
            self.time_of(self.project.frame)
        })
    }
    /// The working range from `start` up to `end` (not included): playing and looping
    /// stay inside it and the frames outside are shown dimmed. `None`: the whole
    /// composition.
    pub(crate) fn host_set_range(&mut self, range: Option<(Time, Time)>) {
        match range {
            Some((start, end)) => {
                let first = self.frame_at(start);
                // The end is the first frame not used.
                let last = (self.frame_at(end) - 1).max(first);
                self.playback_controls.enabled = true;
                self.playback_controls.first = first;
                self.playback_controls.last = Some(last);
                self.host_range = true;
            }
            None => {
                self.playback_controls.enabled = false;
                self.host_range = false;
            }
        }
    }
    /// The working range, from its first frame up to the frame after its last.
    pub(crate) fn host_range(&self) -> Option<(Time, Time)> {
        let count = self.project.document.snapshot().frame_count;
        self.playback_controls.enabled.then(|| {
            let (first, last) = self.playback_controls.range(count);
            (self.time_of(first), self.time_of(last + 1))
        })
    }
}
