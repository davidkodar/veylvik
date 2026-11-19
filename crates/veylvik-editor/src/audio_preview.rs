//! Playback sound: the composition's audio is mixed in the background (like export) for the
//! playback range and played from the playhead; edits remix, and drift over 0.2 s resyncs.
use crate::Studio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use veylvik_audio::{Pcm, Player};
use veylvik_document::NodeId;

/// What a mix was made from; any change remixes.
#[derive(Clone, PartialEq)]
struct Key {
    session: NodeId,
    revision: u64,
    window: [u64; 2],
    rate: u32,
}
type Mixed = Result<Option<Pcm>, String>;
#[derive(Default)]
pub struct AudioPreview {
    player: Option<Player>,
    /// Device preference the player (or its error) belongs to.
    opened_for: Option<Option<String>>,
    error: Option<String>,
    pcm: Option<(Key, Option<Arc<Pcm>>)>,
    pending: Option<(Key, mpsc::Receiver<Mixed>, Arc<AtomicBool>)>,
    /// The mix now playing, so a fresh mix restarts at the playhead.
    playing: Option<Arc<Pcm>>,
}
impl AudioPreview {
    /// What preferences show: the device in use, or why there is no sound.
    pub fn status(&self) -> String {
        match (&self.player, &self.error) {
            (Some(player), None) => player.description().to_owned(),
            (Some(player), Some(error)) => {
                format!("{} · last mix failed: {error}", player.description())
            }
            (None, Some(error)) => format!("No audio: {error}"),
            (None, None) => "Not opened yet; it opens on first playback.".into(),
        }
    }
    fn stop(&mut self) {
        if let Some(player) = &self.player {
            player.stop();
        }
        self.playing = None;
    }
}
impl Studio {
    pub(crate) fn audio_tick(&mut self) {
        let prefs = &self.preferences.values;
        if !prefs.audio_enabled || !self.playing {
            self.audio.stop();
            return;
        }
        let device = prefs.audio_device.clone();
        if self.audio.opened_for.as_ref() != Some(&device) {
            self.audio.stop();
            self.audio.player = None;
            self.audio.pcm = None;
            match Player::open(device.as_deref()) {
                Ok(player) => {
                    self.audio.player = Some(player);
                    self.audio.error = None;
                }
                Err(e) => self.audio.error = Some(e.to_string()),
            }
            self.audio.opened_for = Some(device);
        }
        let Some(rate) = self.audio.player.as_ref().map(Player::rate) else {
            return;
        };
        let gain = self.preferences.values.audio_volume;
        let doc = self.project.document.snapshot();
        let fps = doc.frame_rate.ticks as f64 / doc.frame_rate.rate as f64;
        let range = self.playback_controls.range(doc.frame_count);
        let key = Key {
            session: self.project.session,
            revision: self.project.document.revision(),
            window: [range.0 as u64, range.1 as u64 + 1],
            rate,
        };
        // Collect a finished mix; ignore one for an outdated key.
        if let Some((pending, rx, _)) = &self.audio.pending
            && let Ok(result) = rx.try_recv()
        {
            let pending = pending.clone();
            self.audio.pending = None;
            match result {
                Ok(pcm) => self.audio.pcm = Some((pending, pcm.map(Arc::new))),
                Err(e) => {
                    self.audio.error = Some(e);
                    self.audio.pcm = Some((pending, None));
                }
            }
        }
        let current = self.audio.pcm.as_ref().is_some_and(|(k, _)| *k == key);
        let mixing = self.audio.pending.as_ref().is_some_and(|(k, ..)| *k == key);
        if !current && !mixing {
            if let Some((_, _, cancel)) = self.audio.pending.take() {
                cancel.store(true, Ordering::Relaxed);
            }
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            let (doc, base) = (doc.clone(), self.project.base.clone());
            let window = [key.window[0] as f64 / fps, key.window[1] as f64 / fps];
            let spawned = std::thread::Builder::new()
                .name("audio-mix".into())
                .spawn(move || {
                    let result = veylvik_audio::mix(&doc, &base, window, rate, || {
                        !stop.load(Ordering::Relaxed)
                    })
                    .map_err(|e| e.to_string());
                    let _ = tx.send(result);
                });
            if spawned.is_ok() {
                self.audio.pending = Some((key, rx, cancel));
            }
        }
        // Until the current mix is ready, the previous one keeps playing.
        let Some(pcm) = self.audio.pcm.as_ref().and_then(|(_, p)| p.clone()) else {
            self.audio.stop();
            return;
        };
        let player = self.audio.player.as_ref().unwrap();
        player.set_gain(gain);
        let now = self.project.frame as f64 / fps;
        let fresh = !self
            .audio
            .playing
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, &pcm));
        let drift = player.position().map_or(f64::INFINITY, |t| (t - now).abs());
        if fresh || drift > 0.2 {
            player.play(&pcm, now);
            self.audio.playing = Some(pcm);
        }
    }
}
