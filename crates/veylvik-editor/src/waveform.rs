//! The composition's sound as a strip under the ruler: mixed like export on a worker at a low rate, reduced to 2048 peak levels, and remixed only when footage, its timing or the composition's length changes. At most the first ten minutes.
use crate::Studio;
use eframe::egui;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use veylvik_audio::Pcm;
use veylvik_document::{Composition, NodeId, Operation};

/// Levels across the strip.
const BINS: usize = 2048;
/// Enough for a picture of the sound, and quick to mix.
const RATE: u32 = 8000;

/// Peak level per slice of the first `seconds` of the composition, 0..=1.
pub struct Peaks {
    pub seconds: f64,
    pub levels: Vec<f32>,
}
type Mixed = Result<Option<Peaks>, String>;
#[derive(Default)]
pub struct Waveform {
    checked: Option<(NodeId, u64)>,
    key: Option<u64>,
    pending: Option<(mpsc::Receiver<Mixed>, Arc<AtomicBool>)>,
    pub peaks: Option<Arc<Peaks>>,
}
/// The loudest sample of either channel in each of `bins` equal slices.
pub(crate) fn levels(pcm: &Pcm, bins: usize) -> Vec<f32> {
    let frames = pcm.samples.len() / 2;
    let mut levels = vec![0.0_f32; bins];
    if frames == 0 || bins == 0 {
        return levels;
    }
    for (frame, pair) in pcm.samples.as_chunks::<2>().0.iter().enumerate() {
        let bin = (frame as u128 * bins as u128 / frames as u128) as usize;
        let level = pair[0].abs().max(pair[1].abs());
        if level.is_finite() && level > levels[bin] {
            levels[bin] = level.min(1.0);
        }
    }
    levels
}
/// What the sound depends on; None when the composition has no footage at all.
fn fingerprint(doc: &Composition, base: &std::path::Path) -> Option<u64> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut any = false;
    for node in doc.nodes.values() {
        if let Operation::Footage(footage) = &node.operation {
            any = true;
            footage.asset.hash(&mut hasher);
            let t = &footage.timing;
            for time in [Some(t.start), Some(t.source_in), t.source_out] {
                time.map(|t| (t.ticks, t.rate)).hash(&mut hasher);
            }
        }
        if let Operation::Sound(sound) = &node.operation {
            any = true;
            (&sound.file, node.bypass).hash(&mut hasher);
            serde_json::to_string(&sound.gain)
                .unwrap_or_default()
                .hash(&mut hasher);
            let t = &sound.timing;
            for time in [Some(t.start), Some(t.source_in), t.source_out] {
                time.map(|t| (t.ticks, t.rate)).hash(&mut hasher);
            }
        }
    }
    (doc.frame_count, doc.frame_rate.ticks, doc.frame_rate.rate).hash(&mut hasher);
    base.hash(&mut hasher);
    any.then(|| hasher.finish())
}
impl Studio {
    /// Keep the strip's levels current; cheap unless the sound's sources changed.
    pub(crate) fn waveform_tick(&mut self, ctx: &egui::Context) {
        if let Some((rx, _)) = &self.waveform.pending
            && let Ok(result) = rx.try_recv()
        {
            self.waveform.pending = None;
            match result {
                Ok(peaks) => self.waveform.peaks = peaks.map(Arc::new),
                Err(error) => {
                    self.waveform.peaks = None;
                    self.status = format!("Waveform: {error}");
                }
            }
        }
        let seen = (self.project.session, self.project.document.revision());
        if self.waveform.checked == Some(seen) {
            return;
        }
        self.waveform.checked = Some(seen);
        let doc = self.project.document.snapshot();
        let key = fingerprint(doc, &self.project.base);
        if key == self.waveform.key {
            return;
        }
        self.waveform.key = key;
        if let Some((_, cancel)) = self.waveform.pending.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if key.is_none() {
            self.waveform.peaks = None;
            return;
        }
        let fps = doc.frame_rate.ticks as f64 / f64::from(doc.frame_rate.rate.max(1));
        let seconds = (f64::from(doc.frame_count) / fps).min(veylvik_audio::MAX_SECONDS);
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (doc, base, ctx) = (doc.clone(), self.project.base.clone(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("waveform".into())
            .spawn(move || {
                let result = veylvik_audio::mix(&doc, &base, [0.0, seconds], RATE, || {
                    !stop.load(Ordering::Relaxed)
                })
                .map(|pcm| {
                    pcm.map(|pcm| Peaks {
                        seconds,
                        levels: levels(&pcm, BINS),
                    })
                    .filter(|peaks| peaks.levels.iter().any(|level| *level > 0.0))
                })
                .map_err(|e| e.to_string());
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.waveform.pending = Some((rx, cancel));
        }
    }
}
/// Paint `peaks` in `rect`, whose width spans frames 0..=`end` at `fps`.
pub(crate) fn paint(ui: &egui::Ui, rect: egui::Rect, end: f32, fps: f64, peaks: &Peaks) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 2.0, crate::theme::palette(ui.ctx()).canvas);
    let colour = egui::Color32::from_rgb(90, 170, 150);
    let middle = rect.center().y;
    let half = rect.height() * 0.5 - 1.0;
    let per_point = ui.ctx().pixels_per_point();
    let columns = (rect.width() * per_point).ceil().max(1.0) as usize;
    let length = f64::from(end) / fps;
    // Quiet material is drawn larger, up to four times, so its shape can be read.
    let loudest = peaks
        .levels
        .iter()
        .copied()
        .fold(0.0_f32, f32::max)
        .max(0.25);
    for column in 0..columns {
        // The slices this screen column covers; the loudest of them is drawn.
        let span = |c: usize| {
            let seconds = c as f64 / columns as f64 * length;
            ((seconds / peaks.seconds) * peaks.levels.len() as f64) as usize
        };
        let (first, last) = (span(column), span(column + 1).max(span(column) + 1));
        if first >= peaks.levels.len() {
            break;
        }
        let level = peaks.levels[first..last.min(peaks.levels.len())]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        if level <= 0.0 {
            continue;
        }
        let x = rect.left() + (column as f32 + 0.5) / per_point;
        // A square root lifts quiet passages so speech is visible beside music.
        let reach = ((level / loudest).sqrt() * half).max(0.5);
        painter.vline(
            x,
            middle - reach..=middle + reach,
            (1.0 / per_point, colour),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_take_the_loudest_sample_of_either_channel_per_slice() {
        // Four slices of two sample frames each; one channel quiet, clipping and NaN handled.
        let pcm = Pcm {
            rate: 8,
            start: 0.0,
            samples: vec![
                0.1,
                -0.2,
                0.0,
                0.05, // slice 0
                0.0,
                0.0,
                0.0,
                0.0, // slice 1
                -0.9,
                0.3,
                0.2,
                0.1, // slice 2
                3.0,
                f32::NAN,
                0.0,
                0.0, // slice 3
            ],
        };
        assert_eq!(levels(&pcm, 4), vec![0.2, 0.0, 0.9, 1.0]);
        assert_eq!(levels(&pcm, 1), vec![1.0]);
        assert_eq!(
            levels(
                &Pcm {
                    rate: 8,
                    start: 0.0,
                    samples: vec![]
                },
                3
            ),
            vec![0.0; 3]
        );
        // The sound depends on footage and length, not on anything else in the project.
        let rate = veylvik_document::Time::new(24, 1).unwrap();
        let mut doc = veylvik_document::blank(64, 36, rate).unwrap();
        let base = std::path::Path::new("/projects");
        assert_eq!(fingerprint(&doc, base), None);
        let clip = veylvik_document::Node::new(
            "Clip",
            Operation::Footage(veylvik_document::nodes::Footage {
                asset: "clip.mp4".into(),
                timing: Default::default(),
                placement: Default::default(),
                colour: Default::default(),
                layer: None,
                fallback: String::new(),
            }),
            &[],
        );
        let id = clip.id;
        doc.nodes.insert(id, clip);
        let first = fingerprint(&doc, base).unwrap();
        doc.width = 128;
        assert_eq!(fingerprint(&doc, base), Some(first));
        doc.frame_count += 1;
        let longer = fingerprint(&doc, base).unwrap();
        assert_ne!(longer, first);
        if let Operation::Footage(footage) = &mut doc.nodes.get_mut(&id).unwrap().operation {
            footage.timing.start = veylvik_document::Time::new(1, 1).unwrap();
        }
        assert_ne!(fingerprint(&doc, base), Some(longer));
    }
}
