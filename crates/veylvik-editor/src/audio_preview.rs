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
        // Holding for frames is silence: sound and picture start together.
        if !prefs.audio_enabled || !self.playing || self.buffering.is_some() || self.playing_behind
        {
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
            let (doc, base) = (self.project.sound_composition(), self.project.base.clone());
            let reach = self.project.reach.clone();
            let hosted = self.project.host_footage();
            let window = [key.window[0] as f64 / fps, key.window[1] as f64 / fps];
            let spawned = std::thread::Builder::new()
                .name("audio-mix".into())
                .spawn(move || {
                    let result =
                        veylvik_audio::mix_within(&doc, &base, &reach, window, rate, || {
                            !stop.load(Ordering::Relaxed)
                        })
                        .map(|mixed| with_host_sound(mixed, &hosted, window, rate))
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

/// `mixed` with the host's own sound of its footage added where the composition places it
/// (as file footage is placed): the preview plays what the host's pages play.
pub(crate) fn with_host_sound(
    mixed: Option<veylvik_audio::Pcm>,
    hosted: &[crate::project::HostFootage],
    window: [f64; 2],
    rate: u32,
) -> Option<veylvik_audio::Pcm> {
    let frames = ((window[1] - window[0]).max(0.0) * f64::from(rate)).round() as usize;
    let mut out = mixed;
    for footage in hosted {
        let Some(clip) = veylvik_io::export_audio::place(
            std::path::Path::new(&footage.key),
            footage.timing,
            footage.length,
            window,
        ) else {
            continue;
        };
        let Some(sound) = footage
            .host
            .media_sound(&footage.key, [clip.from, clip.to], rate)
            .filter(|sound| sound.rate == rate)
        else {
            continue;
        };
        let out = out.get_or_insert_with(|| veylvik_audio::Pcm {
            rate,
            start: window[0],
            samples: vec![0.0; frames * 2],
        });
        // Where the clip begins in the window, in frames of sound.
        let at = ((window[0] + clip.at - out.start) * f64::from(rate))
            .round()
            .max(0.0) as usize;
        // The composition's own sound may end before the clip does.
        let needed = at * 2 + sound.samples.len();
        if out.samples.len() < needed {
            out.samples.resize(needed, 0.0);
        }
        for (i, sample) in sound.samples.iter().enumerate() {
            if let Some(slot) = out.samples.get_mut(at * 2 + i) {
                *slot += *sample;
            }
        }
    }
    out
}

#[cfg(test)]
mod host_sound_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    /// A host whose media sound is a constant, recording what was asked.
    struct Steady(Mutex<Vec<[f64; 2]>>);
    impl crate::host::Host for Steady {
        fn pick(
            &self,
            _: &crate::host::Pick,
        ) -> std::result::Result<Option<crate::host::Picked>, String> {
            Ok(None)
        }
        fn media(&self, _: &str) -> Option<Arc<dyn veylvik_render::FrameSource>> {
            None
        }
        fn media_sound(&self, _: &str, window: [f64; 2], rate: u32) -> Option<veylvik_audio::Pcm> {
            self.0.lock().unwrap().push(window);
            let frames = ((window[1] - window[0]) * f64::from(rate)).round() as usize;
            Some(veylvik_audio::Pcm {
                rate,
                start: window[0],
                samples: vec![0.5; frames * 2],
            })
        }
    }
    #[test]
    fn a_hosts_clip_sounds_where_the_composition_places_it() {
        let host = Arc::new(Steady(Mutex::new(Vec::new())));
        // Placed at 1 s, from 2 s into the media, 10 s of media: in a 0..4 s window it
        // plays media 2..5 s from 1 s on.
        let hosted = [crate::project::HostFootage {
            key: "clip".into(),
            timing: (1.0, 2.0, None),
            length: Some(10.0),
            host: host.clone(),
        }];
        let rate = 100;
        let alone = with_host_sound(None, &hosted, [0.0, 4.0], rate).unwrap();
        assert_eq!(host.0.lock().unwrap().as_slice(), &[[2.0, 5.0]]);
        assert_eq!(alone.samples.len(), 400 * 2);
        assert!(
            alone.samples[..200].iter().all(|s| *s == 0.0),
            "sound before the clip"
        );
        assert!(alone.samples[200..].iter().all(|s| *s == 0.5));
        // With the composition's own sound (shorter than the window): added, and longer.
        let own = veylvik_audio::Pcm {
            rate,
            start: 0.0,
            samples: vec![0.25; 300 * 2],
        };
        let both = with_host_sound(Some(own), &hosted, [0.0, 4.0], rate).unwrap();
        assert_eq!(both.samples.len(), 400 * 2);
        assert!(both.samples[..200].iter().all(|s| *s == 0.25));
        assert!(both.samples[200..600].iter().all(|s| *s == 0.75));
        assert!(both.samples[600..].iter().all(|s| *s == 0.5));
        // Outside the window nothing is asked.
        host.0.lock().unwrap().clear();
        assert!(with_host_sound(None, &hosted, [20.0, 24.0], rate).is_none());
        assert!(host.0.lock().unwrap().is_empty());
    }
}
