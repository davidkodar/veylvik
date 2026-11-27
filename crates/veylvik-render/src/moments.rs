//! Which moments a temporal branch is made of at a moment of the whole, and the settings that decide how they combine: worked out once here, so naming a picture (`Renderer::key`) and making it (`temporal_frame`) cannot disagree.
use crate::plan::{Kind, Plan, TemporalBranch};
use crate::{Renderer, Result, Shutter};
use veylvik_document::Time;

/// What a branch needs at one moment of the whole.
pub(crate) enum Need {
    /// Motion blur: the moments of a shutter, averaged.
    Shutter { shutter: Shutter, times: Vec<Time> },
    /// Time remap: one other moment.
    At(Time),
    /// Optical flow: two moments (the second only when it counts) and how far between.
    Flow {
        first: Time,
        second: Option<Time>,
        settings: crate::flow::FlowSettings,
        quality: veylvik_document::nodes::FlowQuality,
        preview: bool,
    },
    /// OCIO: the moment itself through a colour transform.
    Transformed {
        transform: veylvik_document::color_management::OcioTransform,
        time: Time,
    },
    /// Particle history: earlier moments packed into a grid.
    History { grid: u32, times: Vec<Time> },
    /// Own size: the moment itself at another size.
    Resized(Time),
    /// OpenFX: its own moment, then the other frames the plugin names (number, moment).
    Frames {
        node: veylvik_document::NodeId,
        port: &'static str,
        now: Time,
        others: Vec<(f64, Time)>,
    },
}
/// One piece of what names a branch: a settings word, or a moment whose picture it is
/// made of.
pub(crate) enum Part {
    Word(u32),
    Moment(Time),
}
impl Need {
    /// What names the branch, in order.
    pub(crate) fn name(&self) -> Vec<Part> {
        use Part::{Moment, Word};
        let bits = |v: u64| [Word(v as u32), Word((v >> 32) as u32)];
        match self {
            Need::Shutter { times, .. } => std::iter::once(Word(1))
                .chain(times.iter().map(|t| Moment(*t)))
                .collect(),
            Need::At(time) => vec![Word(2), Moment(*time)],
            Need::Flow {
                first,
                second,
                settings,
                quality,
                preview,
            } => {
                let mut parts = vec![
                    Word(3),
                    Word(settings.fraction.to_bits()),
                    Word(settings.confidence.to_bits()),
                    Word(u32::from(settings.diagnostic)),
                    Word(*quality as u32),
                    Word(u32::from(*preview)),
                    Moment(*first),
                ];
                parts.extend(second.map(Moment));
                parts
            }
            Need::Transformed { transform, time } => {
                let name = crate::fnv(format!("{transform:?}").as_bytes(), 0);
                let mut parts = vec![Word(4)];
                parts.extend(bits(name));
                parts.push(Moment(*time));
                parts
            }
            Need::History { grid, times } => [Word(5), Word(*grid)]
                .into_iter()
                .chain(times.iter().map(|t| Moment(*t)))
                .collect(),
            Need::Resized(time) => vec![Word(6), Moment(*time)],
            Need::Frames { now, others, .. } => {
                let mut parts = vec![Word(7), Moment(*now)];
                for (frame, time) in others {
                    parts.extend(bits(frame.to_bits()));
                    parts.push(Moment(*time));
                }
                parts
            }
        }
    }
    /// Whether reading it is an extra moment in time (a picture of its own size is not).
    pub(crate) fn counts(&self) -> bool {
        !matches!(self, Need::Resized(_))
    }
    /// Whether its name can hold between sessions (an OCIO transform's own files are not
    /// part of it).
    pub(crate) fn lasting(&self) -> bool {
        !matches!(self, Need::Transformed { .. })
    }
}
impl Renderer {
    /// What `branch` needs at `time` of the whole.
    pub(crate) fn need(
        &self,
        plan: &Plan,
        branch: &TemporalBranch,
        time: Time,
        preview: bool,
        inputs: &crate::ImageInputs,
    ) -> Result<Need> {
        Ok(match &branch.kind {
            Kind::MotionBlur(blur) => {
                let shutter = Shutter {
                    angle: blur.angle.at(time)?,
                    phase: blur.phase.at(time)?,
                    samples: if preview {
                        &blur.preview_samples
                    } else {
                        &blur.final_samples
                    }
                    .at(time)?
                    .round() as u32,
                    frame_rate: plan.frame_rate,
                };
                Need::Shutter {
                    shutter,
                    times: shutter.sample_times(time)?,
                }
            }
            Kind::TimeRemap(remap) => Need::At(remap.source_at(time)?),
            Kind::OpticalFlow(flow) => {
                use veylvik_document::nodes::FlowQuality;
                let (first, second, fraction) = flow.source_pair(time)?;
                let edge = match (flow.quality, preview) {
                    (FlowQuality::Fast, _) => 512,
                    (FlowQuality::Balanced, true) => 768,
                    (FlowQuality::Fine, true) => 1024,
                    (FlowQuality::Balanced, false) => 1024,
                    (FlowQuality::Fine, false) => 1536,
                };
                Need::Flow {
                    first,
                    second: (fraction != 0. || flow.diagnostic).then_some(second),
                    settings: crate::flow::FlowSettings {
                        fraction,
                        analysis_edge: edge,
                        confidence: flow.confidence.at(time)?,
                        diagnostic: flow.diagnostic,
                    },
                    quality: flow.quality,
                    preview,
                }
            }
            Kind::Ocio(node) => Need::Transformed {
                transform: node.transform.clone(),
                time,
            },
            // A flock is born on its emitter as it is at the flock's start.
            Kind::History(particles) if crate::nodes::particles::simulating(particles) => {
                let start = f64::from(particles.start.at(time)?).max(0.0);
                Need::At(Time::new((start * 24000.0).round() as i64, 24000)?)
            }
            Kind::History(particles) => {
                let grid = crate::history::grid(particles.history);
                Need::History {
                    grid,
                    times: crate::history::moments(
                        time,
                        particles.life.at(time)?,
                        grid,
                        plan.frame_rate,
                    )?,
                }
            }
            Kind::OwnSize => Need::Resized(time),
            Kind::Shade => Need::At(time),
            Kind::Frames { node, port, effect } => {
                let others = self
                    .ofx_other_frames(plan, *node, port, effect, time, inputs)?
                    .into_iter()
                    .map(|frame| Ok((frame, crate::temporal::time_of(frame, plan.frame_rate)?)))
                    .collect::<Result<_>>()?;
                Need::Frames {
                    node: *node,
                    port,
                    now: time,
                    others,
                }
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_branch_is_named_by_its_settings_and_its_moments_in_a_fixed_order() {
        let t = |f| Time::at_frame(f, Time { ticks: 24, rate: 1 }).unwrap();
        let words = |need: &Need| -> Vec<String> {
            need.name()
                .iter()
                .map(|p| match p {
                    Part::Word(w) => format!("w{w}"),
                    Part::Moment(m) => format!("m{}/{}", m.ticks, m.rate),
                })
                .collect()
        };
        let m = |time: Time| format!("m{}/{}", time.ticks, time.rate);
        assert_eq!(words(&Need::At(t(5))), ["w2".to_string(), m(t(5))]);
        let frames = Need::Frames {
            node: veylvik_document::NodeId::nil(),
            port: "source",
            now: t(10),
            others: vec![(9.0, t(9))],
        };
        let bits = 9.0f64.to_bits();
        assert_eq!(
            words(&frames),
            [
                "w7".to_string(),
                m(t(10)),
                format!("w{}", bits as u32),
                format!("w{}", (bits >> 32) as u32),
                m(t(9))
            ]
        );
        assert!(!Need::Resized(t(1)).counts() && Need::At(t(1)).counts());
    }
}
