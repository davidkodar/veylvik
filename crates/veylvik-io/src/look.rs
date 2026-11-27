//! A render's look compared with a reference's, region by region (veylvik-look): the reference is any still or a frame of any media Veylvik imports, rendered through the engine as it would be as footage, and both are measured at one common size.
use crate::Result;
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{Node, Operation, Time};
use veylvik_look::{Area, Picture, look, resampled, shimmer};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

/// Pictures are compared at most this wide (and at the narrower one's width).
const MOST: u32 = 1920;
/// A picture as it is seen: display-referred linear light.
pub struct Seen {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}
impl Seen {
    fn picture(&self) -> Picture<'_> {
        Picture {
            width: self.width,
            height: self.height,
            pixels: &self.pixels,
        }
    }
}
/// A frame of the reference at `seconds` (a still has only one), and for moving media the
/// frame after it, rendered as the media would be as footage in a composition of its own size.
/// `as_shown` reads a video's code values as a player shows them (an sRGB-like curve), which
/// is what a viewer of the reference sees; otherwise it is decoded to scene light by its own
/// tags, as Veylvik composites footage (a Rec.709 video's mid-tones then read lighter).
pub fn reference(
    renderer: &Renderer,
    path: &Path,
    seconds: f64,
    as_shown: bool,
) -> Result<(Seen, Option<Seen>)> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err("reference_seconds must be 0 or more".into());
    }
    let media = crate::footage::load(path, 512 * 1024 * 1024)
        .map_err(|e| format!("the reference {}: {e}", path.display()))?;
    let info = media.info().ok_or("the reference has no size")?;
    let rate = info.frame_rate.unwrap_or(Time { ticks: 24, rate: 1 });
    let mut doc = veylvik_document::blank(info.width, info.height, rate)?;
    let node = Node::new(
        "Reference",
        Operation::Footage(veylvik_document::nodes::Footage {
            asset: path
                .to_str()
                .ok_or("the reference's path is not UTF-8")?
                .to_owned(),
            timing: Default::default(),
            placement: veylvik_document::media::Placement::Fit,
            colour: if as_shown && matches!(media, crate::footage::Media::Video(_)) {
                veylvik_document::media::InputColour {
                    curve: veylvik_document::media::Curve::Srgb,
                    ..Default::default()
                }
            } else {
                Default::default()
            },
            layer: None,
            fallback: String::new(),
        }),
        &[],
    );
    doc.nodes.clear();
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let frame = (seconds * f64::from(rate.ticks as u32) / f64::from(rate.rate)).floor() as i64;
    if let Some(duration) = info.duration {
        let last = (duration.seconds()? * f64::from(rate.ticks as u32) / f64::from(rate.rate))
            .floor() as i64;
        doc.frame_count = u32::try_from((last + 2).max(1)).unwrap_or(u32::MAX);
        if frame >= last.max(1) {
            return Err(format!(
                "the reference is {:.2} seconds long: reference_seconds must be within it",
                duration.seconds()?
            )
            .into());
        }
    }
    let plan = Plan::compile(&doc)?;
    let base = path.parent().unwrap_or(Path::new("."));
    let inputs: ImageInputs = crate::load_inputs(&plan, base, renderer.budget_bytes)?;
    let at = |n: i64| -> Result<Seen> {
        let rendered = renderer.render_frame(
            &plan,
            RenderRequest::full(Time::at_frame(n, rate)?),
            &inputs,
        )?;
        Ok(Seen {
            width: doc.width,
            height: doc.height,
            pixels: rendered.pixels,
        })
    };
    let first = at(frame)?;
    let next = if info.duration.is_some() {
        Some(at(frame + 1)?)
    } else {
        None
    };
    Ok((first, next))
}
/// One region compared: a name, where it is in the reference and in the render.
pub struct Pair {
    pub name: String,
    pub reference: Area,
    pub ours: Area,
}
/// The regions of an `n` x `n` grid and the whole picture, the same in both.
pub fn grid_pairs(n: u32) -> Vec<Pair> {
    let mut pairs = vec![Pair {
        name: "whole".into(),
        reference: Area::WHOLE,
        ours: Area::WHOLE,
    }];
    for (k, area) in Area::grid(n).into_iter().enumerate() {
        pairs.push(Pair {
            name: format!("row {} column {}", k as u32 / n + 1, k as u32 % n + 1),
            reference: area,
            ours: area,
        });
    }
    pairs
}
/// The regions a tool call names: its `pairs`, or the whole and a `grid` (3 by default).
pub fn pairs(args: &Value) -> Result<Vec<Pair>> {
    let Some(list) = args.get("pairs") else {
        let n = args.get("grid").and_then(Value::as_u64).unwrap_or(3);
        if !(1..=6).contains(&n) {
            return Err("grid must be 1 to 6".into());
        }
        return Ok(grid_pairs(n as u32));
    };
    list.as_array()
        .ok_or("pairs must be a list")?
        .iter()
        .enumerate()
        .map(|(k, p)| {
            Ok(Pair {
                name: p
                    .get("name")
                    .and_then(Value::as_str)
                    .map_or_else(|| format!("pair {}", k + 1), str::to_owned),
                reference: serde_json::from_value(p["reference"].clone())?,
                ours: serde_json::from_value(p["ours"].clone())?,
            })
        })
        .collect()
}
/// A compare_look call: the reference it names (an absolute path, at reference_seconds,
/// as shown or as scene light) against `ours`, in the regions it names.
pub fn against(renderer: &Renderer, args: &Value, ours: (&Seen, Option<&Seen>)) -> Result<Value> {
    let path = Path::new(
        args["reference"]
            .as_str()
            .ok_or("reference must be a path")?,
    );
    if !path.is_absolute() {
        return Err("reference must be an absolute path".into());
    }
    let seconds = args
        .get("reference_seconds")
        .map_or(Some(0.0), Value::as_f64)
        .ok_or("reference_seconds must be a number")?;
    let pairs = pairs(args)?;
    let as_shown = args.get("reference_colour").and_then(Value::as_str) != Some("scene");
    let (reference, reference_next) = reference(renderer, path, seconds, as_shown)?;
    compare((&reference, reference_next.as_ref()), ours, &pairs)
}
/// The render's look against the reference's in each pair of regions, with advice. The
/// next frames, where both are given, add how each region shimmers.
pub fn compare(
    reference: (&Seen, Option<&Seen>),
    ours: (&Seen, Option<&Seen>),
    pairs: &[Pair],
) -> Result<Value> {
    for p in pairs {
        p.reference.check()?;
        p.ours.check()?;
    }
    let width = reference.0.width.min(ours.0.width).min(MOST);
    let shrink = |s: &Seen| resampled(s.picture(), width);
    let (r, o) = (shrink(reference.0), shrink(ours.0));
    let next = match (reference.1, ours.1) {
        (Some(a), Some(b)) => Some((shrink(a), shrink(b))),
        _ => None,
    };
    let mut regions = Vec::new();
    for p in pairs {
        let (a, b) = (look(r.picture(), &p.reference), look(o.picture(), &p.ours));
        let mut advice = veylvik_look::advise(&a, &b);
        let mut entry = json!({"name":p.name,"reference_area":p.reference,"area":p.ours,
            "reference":serde_json::to_value(&a)?,"ours":serde_json::to_value(&b)?});
        if let Some((rn, on)) = &next {
            let sa = shimmer(r.picture(), rn.picture(), &p.reference)?;
            let sb = shimmer(o.picture(), on.picture(), &p.ours)?;
            advice.extend(veylvik_look::advise_shimmer(&sa, &sb));
            entry["shimmer"] =
                json!({"reference":serde_json::to_value(&sa)?,"ours":serde_json::to_value(&sb)?});
        }
        entry["advice"] = json!(advice);
        regions.push(entry);
    }
    Ok(
        json!({"measured_width":width,"shimmer_measured":next.is_some(),"regions":regions,
        "units":"grains per 100 x 100 px and grain sizes in px as in a frame 1920 wide; brightness on the screen, 0 to 1"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dots(width: u32, height: u32, spacing: u32, light: f32) -> Seen {
        let mut pixels = vec![[0.0, 0.0, 0.0, 1.0]; (width * height) as usize];
        for y in (spacing / 2..height).step_by(spacing as usize) {
            for x in (spacing / 2..width).step_by(spacing as usize) {
                pixels[(y * width + x) as usize] = [light, light * 0.1, light * 0.1, 1.0];
            }
        }
        Seen {
            width,
            height,
            pixels,
        }
    }
    #[test]
    fn a_picture_compared_with_itself_needs_nothing_changed() {
        let a = dots(320, 180, 8, 0.8);
        let out = compare((&a, Some(&a)), (&a, Some(&a)), &grid_pairs(3)).unwrap();
        let regions = out["regions"].as_array().unwrap();
        assert_eq!(regions.len(), 10);
        assert!(
            regions
                .iter()
                .all(|r| r["advice"].as_array().unwrap().is_empty()),
            "{out}"
        );
        assert_eq!(out["shimmer_measured"], true);
    }
    #[test]
    fn pictures_of_different_sizes_are_measured_at_one_size() {
        // The same pattern at twice the size: grain counts and sizes as at 1920 wide agree.
        let small = dots(320, 180, 8, 0.8);
        let large = dots(640, 360, 16, 0.8);
        let out = compare((&large, None), (&small, None), &grid_pairs(1)).unwrap();
        assert_eq!(out["measured_width"], 320);
        let whole = &out["regions"][0];
        let (a, b) = (
            whole["reference"]["grains"].as_f64().unwrap(),
            whole["ours"]["grains"].as_f64().unwrap(),
        );
        assert!((a / b - 1.0).abs() < 0.3, "{a} {b}");
        assert_eq!(out["shimmer_measured"], false);
    }
    #[test]
    fn crowded_grains_are_named_in_the_advice() {
        let reference = dots(320, 180, 16, 0.8);
        let ours = dots(320, 180, 6, 0.8);
        let out = compare((&reference, None), (&ours, None), &grid_pairs(1)).unwrap();
        let advice = out["regions"][0]["advice"].to_string();
        assert!(advice.contains("fewer particles"), "{advice}");
    }
}
