//! The transport: whether the playhead runs, the clock it runs by, and what playing waits on (frames still being prepared). Stopping is one call, so nothing of a play is left behind whoever stops it.
use std::time::{Duration, Instant};

pub(crate) struct Transport {
    pub playing: bool,
    /// When the clock last started, and the frame it started from.
    clock: Instant,
    from: i64,
    /// Play was pressed and the frames ahead are still being prepared: the playhead
    /// holds, and since when nothing more could be prepared (to give up waiting).
    pub buffering: Option<Option<Instant>>,
    /// What the viewer says while playing waits, when it waits.
    pub waiting: Option<crate::fill::Waiting>,
    /// For this play only: play at once, skipping frames, as the artist asked.
    pub play_now: bool,
    /// Playing shows every frame and has fallen behind real time: the sound waits.
    pub behind: bool,
}
impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            clock: Instant::now(),
            from: 0,
            buffering: None,
            waiting: None,
            play_now: false,
            behind: false,
        }
    }
}
impl Transport {
    /// Stop playing, and forget everything this play was waiting on.
    pub(crate) fn pause(&mut self) {
        self.playing = false;
        self.stop_waiting();
        self.play_now = false;
        self.behind = false;
    }
    /// Nothing to wait for (any more): the playhead may run.
    pub(crate) fn stop_waiting(&mut self) {
        self.buffering = None;
        self.waiting = None;
    }
    /// Start the clock again from `frame`.
    pub(crate) fn start_clock(&mut self, frame: i64) {
        self.clock = Instant::now();
        self.from = frame;
    }
    /// The frame the clock started from.
    pub(crate) fn from(&self) -> i64 {
        self.from
    }
    /// Whole frames since the clock started, at `fps` (ticks frames each rate seconds).
    pub(crate) fn elapsed_frames(&self, fps: veylvik_document::Time) -> i64 {
        Self::frames_in(self.clock.elapsed(), fps)
    }
    fn frames_in(elapsed: Duration, fps: veylvik_document::Time) -> i64 {
        (elapsed.as_secs_f64() * fps.ticks as f64 / fps.rate as f64) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pausing_forgets_what_the_play_waited_on_and_the_clock_counts_whole_frames() {
        let mut transport = Transport {
            playing: true,
            buffering: Some(None),
            play_now: true,
            behind: true,
            ..Default::default()
        };
        transport.pause();
        assert!(!transport.playing && transport.buffering.is_none() && transport.waiting.is_none());
        assert!(!transport.play_now && !transport.behind);
        // 24 frames a second: 1.04 s is 24 whole frames, 30000/1001 is 29.97.
        assert_eq!(
            Transport::frames_in(
                Duration::from_millis(1040),
                veylvik_document::Time { ticks: 24, rate: 1 }
            ),
            24
        );
        assert_eq!(
            Transport::frames_in(
                Duration::from_secs(10),
                veylvik_document::Time {
                    ticks: 30000,
                    rate: 1001
                }
            ),
            299
        );
        transport.start_clock(42);
        assert_eq!(transport.from(), 42);
    }
}
