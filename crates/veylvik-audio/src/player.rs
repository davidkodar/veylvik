//! Plays a mixed `Pcm` through the default output device from any composition time. The
//! device callback never blocks or allocates: it try-locks the buffer and plays silence
//! when it is busy, stopped or past the end.
use crate::{Pcm, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Shared {
    pcm: Mutex<Option<Arc<Pcm>>>,
    /// Next sample frame of `pcm` to play.
    frame: AtomicU64,
    playing: AtomicBool,
    /// Linear gain as f32 bits.
    gain: AtomicU32,
}
pub struct Player {
    _stream: cpal::Stream,
    shared: Arc<Shared>,
    rate: u32,
    description: String,
}
/// An output device as the system reports it, for choosing in preferences.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The ALSA name, as saved in preferences.
    pub name: String,
    /// What people see: "PipeWire", "Scarlett Solo USB (direct)".
    pub label: String,
    /// Opens the hardware directly, bypassing the sound server.
    pub direct: bool,
    pub default: bool,
    /// "48000 Hz · 2 ch · f32", or why the device could not be queried.
    pub detail: String,
}
/// The audio system in use: ALSA, noting when PipeWire or PulseAudio serve it.
pub fn backend() -> String {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    let has = |name: &str| runtime.as_ref().is_some_and(|dir| dir.join(name).exists());
    if has("pipewire-0") {
        "ALSA via PipeWire".into()
    } else if has("pulse/native") {
        "ALSA via PulseAudio".into()
    } else {
        "ALSA".into()
    }
}
fn detail(config: &cpal::SupportedStreamConfig) -> String {
    format!(
        "{} Hz · {} ch · {:?}",
        config.sample_rate().0,
        config.channels(),
        config.sample_format()
    )
    .to_lowercase()
    .replace(" hz", " Hz")
}
/// ALSA card ids mapped to their long names from /proc/asound/cards ("USB" → "Scarlett Solo USB").
fn card_names() -> Vec<(String, String)> {
    let text = std::fs::read_to_string("/proc/asound/cards").unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (id, rest) = line.split_once('[')?.1.split_once(']')?;
            let long = rest.split_once(" - ").map_or(rest, |(_, name)| name).trim();
            Some((
                id.trim().to_owned(),
                long.trim_start_matches(':').trim().to_owned(),
            ))
        })
        .collect()
}
/// A person-facing label for an ALSA output name, or `None` for plumbing (rate converters,
/// up/downmixers, duplicates of the default) that nobody should pick.
fn label(name: &str, cards: &[(String, String)]) -> Option<(String, bool)> {
    match name {
        "pipewire" => return Some(("PipeWire".into(), false)),
        "pulse" => return Some(("PulseAudio".into(), false)),
        "jack" => return Some(("JACK".into(), false)),
        _ => {}
    }
    let (kind, args) = name.split_once(':')?;
    let field = |key: &str| {
        args.split(',')
            .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
            .map(str::to_owned)
    };
    let card = field("CARD")?;
    let card = cards
        .iter()
        .find(|(id, _)| *id == card)
        .map_or(card, |(_, long)| long.clone());
    let device = field("DEV")
        .and_then(|d| d.parse::<u32>().ok())
        .unwrap_or(0);
    match kind {
        "front" => Some((format!("{card} (direct)"), true)),
        "hdmi" => Some((
            format!("{card} HDMI/DisplayPort {} (direct)", device + 1),
            true,
        )),
        "iec958" => Some((format!("{card} S/PDIF (direct)"), true)),
        _ => None,
    }
}
/// Output devices worth choosing, with their default formats: sound servers first, then
/// hardware outputs opened directly. "System default" is not listed; it is the absence of a choice.
pub fn devices() -> Vec<Device> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.name().ok());
    let cards = card_names();
    let mut list: Vec<Device> = host
        .output_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d.name().ok()?;
                    let (label, direct) = label(&name, &cards)?;
                    let detail = d
                        .default_output_config()
                        .map(|c| detail(&c))
                        .unwrap_or_else(|e| e.to_string());
                    Some(Device {
                        default: Some(&name) == default.as_ref(),
                        name,
                        label,
                        direct,
                        detail,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|d| d.direct);
    list
}
impl Player {
    /// Open the named output device, or the system default for `None`. Fails explicitly
    /// when it is missing; never falls back to another device.
    pub fn open(name: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let device = match name {
            None => host
                .default_output_device()
                .ok_or("No audio output device is available")?,
            Some(name) => host
                .output_devices()?
                .find(|d| d.name().is_ok_and(|n| n == name))
                .ok_or_else(|| format!("Audio device \"{name}\" is not available"))?,
        };
        let config = device.default_output_config()?;
        let shown = match name {
            None => "System default".to_owned(),
            Some(name) => label(name, &card_names()).map_or(name.to_owned(), |(l, _)| l),
        };
        let description = format!("{} · {} · {}", shown, detail(&config), backend());
        let shared = Arc::new(Shared::default());
        shared.gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let rate = config.sample_rate.0;
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, shared.clone())?,
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared.clone())?,
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared.clone())?,
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, shared.clone())?,
            other => return Err(format!("unsupported audio sample format {other:?}").into()),
        };
        stream.play()?;
        Ok(Self {
            _stream: stream,
            shared,
            rate,
            description,
        })
    }
    /// "device · 48000 Hz · 2 ch · f32 · ALSA via PipeWire"
    pub fn description(&self) -> &str {
        &self.description
    }
    /// The device's sample rate; mix at this rate so playback needs no resampling.
    pub fn rate(&self) -> u32 {
        self.rate
    }
    /// Play `pcm` from composition time `at` (seconds).
    pub fn play(&self, pcm: &Arc<Pcm>, at: f64) {
        let frame = ((at - pcm.start) * f64::from(pcm.rate)).round().max(0.0) as u64;
        if let Ok(mut current) = self.shared.pcm.lock() {
            *current = Some(pcm.clone());
        }
        self.shared.frame.store(frame, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Release);
    }
    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Release);
    }
    pub fn set_gain(&self, gain: f32) {
        self.shared
            .gain
            .store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }
    /// Composition time now playing, or `None` when stopped.
    pub fn position(&self) -> Option<f64> {
        if !self.shared.playing.load(Ordering::Acquire) {
            return None;
        }
        let pcm = self.shared.pcm.lock().ok()?.clone()?;
        let frame = self.shared.frame.load(Ordering::Relaxed);
        Some(pcm.start + frame as f64 / f64::from(pcm.rate))
    }
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let stream = device.build_output_stream(
        config,
        move |out: &mut [T], _| fill(out, channels, &shared),
        |error| eprintln!("veylvik: audio output error: {error}"),
        None,
    )?;
    Ok(stream)
}
fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(
    out: &mut [T],
    channels: usize,
    shared: &Shared,
) {
    let silence = T::from_sample(0.0f32);
    let pcm = if shared.playing.load(Ordering::Acquire) {
        shared.pcm.try_lock().ok().and_then(|p| p.clone())
    } else {
        None
    };
    let Some(pcm) = pcm else {
        out.fill(silence);
        return;
    };
    let gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
    let mut frame = shared.frame.load(Ordering::Relaxed);
    let total = pcm.frames();
    for slot in out.chunks_mut(channels) {
        let (l, r) = if frame < total {
            let i = frame as usize * 2;
            (pcm.samples[i] * gain, pcm.samples[i + 1] * gain)
        } else {
            (0.0, 0.0)
        };
        frame += 1;
        match slot.len() {
            1 => slot[0] = T::from_sample((l + r) * 0.5),
            _ => {
                slot[0] = T::from_sample(l);
                slot[1] = T::from_sample(r);
                for extra in &mut slot[2..] {
                    *extra = silence;
                }
            }
        }
    }
    shared.frame.store(frame, Ordering::Relaxed);
}
#[cfg(test)]
mod tests {
    #[test]
    fn device_labels_name_cards_and_hide_alsa_plumbing() {
        let cards = vec![
            ("USB".to_owned(), "Scarlett Solo USB".to_owned()),
            ("NVidia".to_owned(), "HDA NVidia".to_owned()),
        ];
        let l = |n: &str| super::label(n, &cards);
        assert_eq!(l("pipewire"), Some(("PipeWire".into(), false)));
        assert_eq!(
            l("front:CARD=USB,DEV=0"),
            Some(("Scarlett Solo USB (direct)".into(), true))
        );
        assert_eq!(
            l("hdmi:CARD=NVidia,DEV=2"),
            Some(("HDA NVidia HDMI/DisplayPort 3 (direct)".into(), true))
        );
        for plumbing in [
            "default",
            "sysdefault",
            "lavrate",
            "upmix",
            "surround40:CARD=USB,DEV=0",
            "sysdefault:CARD=USB",
        ] {
            assert_eq!(l(plumbing), None, "{plumbing}");
        }
    }
}
