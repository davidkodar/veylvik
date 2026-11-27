//! Plain advice from the differences between a reference's look and a render's: what to change, in words, with the numbers behind it.
use crate::{Look, Shimmer};

/// Two values differ when one is more than `by` times the other (and not both tiny).
fn apart(ours: f32, reference: f32, by: f32, floor: f32) -> Option<f32> {
    if ours.max(reference) < floor {
        return None;
    }
    let (o, r) = (ours.max(1e-6), reference.max(1e-6));
    (o / r > by || r / o > by).then_some(o / r)
}
/// What to change in a render for its region to look like the reference's.
pub fn advise(reference: &Look, ours: &Look) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(k) = apart(ours.grains, reference.grains, 1.3, 0.1) {
        out.push(if k > 1.0 {
            format!("{:.1}x as many distinct grains as the reference ({:.0} against {:.0} per 100 x 100 px): fewer particles here, or spread them further apart (in depth too)", k, ours.grains, reference.grains)
        } else {
            format!("only {:.0}% of the reference's distinct grains ({:.0} against {:.0} per 100 x 100 px): more particles here, or fewer overlapping into a haze", 100.0 * k, ours.grains, reference.grains)
        });
    }
    if let Some(k) = apart(ours.grain_size, reference.grain_size, 1.25, 1.0) {
        out.push(if k > 1.0 {
            format!("grains look {:.1}x as large ({:.1} against {:.1} px across): smaller particles, sharper focus here, or no clumps", k, ours.grain_size, reference.grain_size)
        } else {
            format!("grains look smaller ({:.1} against {:.1} px across): larger particles, or throw them a little out of focus", ours.grain_size, reference.grain_size)
        });
    }
    if let Some(k) = apart(ours.stand_out, reference.stand_out, 1.15, 1.0) {
        out.push(if k < 1.0 {
            format!("grains stand out less from what is round them ({:.1}x against {:.1}x): darker gaps between them (fewer overlapping, less haze or glow), sharper focus or stronger sparkle", ours.stand_out, reference.stand_out)
        } else {
            format!("grains stand out more than the reference's ({:.1}x against {:.1}x): softer light between them, a little haze or less sparkle", ours.stand_out, reference.stand_out)
        });
    }
    if let Some(k) = apart(ours.fine_contrast, reference.fine_contrast, 1.25, 0.002) {
        out.push(if k < 1.0 {
            format!("{:.0}% of the reference's fine detail ({:.4} against {:.4}): crisper grains (focus, smaller grains, sparkle with some spread) rather than a soft mass", 100.0 * k, ours.fine_contrast, reference.fine_contrast)
        } else {
            format!("more fine detail than the reference ({:.4} against {:.4}): softer, fewer or out-of-focus grains", ours.fine_contrast, reference.fine_contrast)
        });
    }
    let names = ["darks", "middle", "brights", "highlights"];
    for k in [1usize, 2, 3] {
        if let Some(r) = apart(ours.brightness[k], reference.brightness[k], 1.2, 0.03) {
            out.push(format!(
                "{} {} ({:.2} against {:.2} on the screen): {}",
                names[k],
                if r > 1.0 { "brighter" } else { "darker" },
                ours.brightness[k],
                reference.brightness[k],
                match (k, r > 1.0) {
                    (1, true) => "lower the exposure or the lights",
                    (1, false) => "raise the exposure or the lights",
                    (_, true) => "tame the hottest light (sparkle, glow, key light)",
                    (_, false) => "more light on the brightest parts (sparkle, key light, glints)",
                }
            ));
        }
    }
    if (ours.coverage - reference.coverage).abs() > 0.1 {
        out.push(format!(
            "{:.0}% of this region is lit against the reference's {:.0}%: {}",
            100.0 * ours.coverage,
            100.0 * reference.coverage,
            if ours.coverage < reference.coverage {
                "fill more of it (more, wider or nearer material, or a faint body of it)"
            } else {
                "leave more of it dark"
            }
        ));
    }
    if apart(ours.background, reference.background, 1.6, 0.01).is_some() {
        out.push(format!(
            "background {} ({:.3} against {:.3}): {}",
            if ours.background < reference.background {
                "darker"
            } else {
                "lighter"
            },
            ours.background,
            reference.background,
            if ours.background < reference.background {
                "a little haze or a soft glow behind"
            } else {
                "less haze or glow behind"
            }
        ));
    }
    for r in &reference.hues {
        let o = ours.hues.iter().find(|h| h.band == r.band);
        let share = o.map_or(0.0, |h| h.share);
        if (share - r.share).abs() > 0.05 {
            out.push(format!(
                "{:.0}% {} against the reference's {:.0}%: {} {}",
                100.0 * share,
                r.band,
                100.0 * r.share,
                if share < r.share { "more" } else { "less" },
                r.band
            ));
        }
        if let Some(o) = o.filter(|o| {
            o.share > 0.05 && r.share > 0.05 && (o.saturation - r.saturation).abs() > 0.15
        }) {
            out.push(format!(
                "{} {} saturated ({:.2} against {:.2})",
                r.band,
                if o.saturation > r.saturation {
                    "more"
                } else {
                    "less"
                },
                o.saturation,
                r.saturation
            ));
        }
    }
    for o in &ours.hues {
        if o.share > 0.05 && !reference.hues.iter().any(|r| r.band == o.band) {
            out.push(format!(
                "{:.0}% {}, which the reference does not have",
                100.0 * o.share,
                o.band
            ));
        }
    }
    out
}
/// What to change for a region to shimmer like the reference's.
pub fn advise_shimmer(reference: &Shimmer, ours: &Shimmer) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(k) = apart(ours.bright, reference.bright, 1.4, 0.005) {
        out.push(if k < 1.0 {
            format!("{:.1}% of it glints against the reference's {:.1}%: more grains catching the light (sparkle with a wider spread, more lights, or lights behind)", 100.0 * ours.bright, 100.0 * reference.bright)
        } else {
            format!("{:.1}% of it glints against the reference's {:.1}%: fewer or narrower glints", 100.0 * ours.bright, 100.0 * reference.bright)
        });
    }
    if reference.bright > 0.005 && (ours.changing - reference.changing).abs() > 0.12 {
        out.push(format!(
            "{:.0}% of its glints change from one frame to the next against the reference's {:.0}%: {}",
            100.0 * ours.changing,
            100.0 * reference.changing,
            if ours.changing < reference.changing { "facets turning faster, or grains moving more" } else { "facets turning slower, or calmer grains" }
        ));
    }
    out
}
