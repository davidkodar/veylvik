//! Layer bars under the ruler: one bar for each footage clip and each fade, showing when it is on screen. Dragging a bar slides it in time and dragging an end trims it, in whole frames; the change is previewed while dragging and committed on release as one undo step.
use crate::key_timeline::Action;
use eframe::egui::{self, Color32, FontId, Rangef, Rect, Stroke};
use veylvik_document::{Command, Composition, Node, NodeId, Operation, Time};

/// Rows shown; a composition with more says how many are left out.
const ROWS: usize = 8;
const ROW: f32 = 17.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Start,
    Body,
    End,
}
/// When a layer is on screen, in frames: `first` up to but not including `last`
/// (None runs to the end of the composition or of the media).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Bar {
    pub id: NodeId,
    pub label: String,
    pub first: i64,
    pub last: Option<i64>,
    pub footage: bool,
}
fn frames(time: Time, fps: Time) -> i64 {
    let seconds = time.ticks as f64 / f64::from(time.rate.max(1));
    (seconds * fps.ticks as f64 / f64::from(fps.rate.max(1))).round() as i64
}
fn shifted(time: Time, by: i64, fps: Time) -> Option<Time> {
    let step = Time::at_frame(by.abs(), fps).ok()?;
    let next = if by >= 0 {
        time.checked_add(step)
    } else {
        time.checked_sub(step)
    }
    .ok()?;
    (next.ticks >= 0).then_some(next)
}
pub(crate) fn bar(node: &Node, fps: Time) -> Option<Bar> {
    let (first, last, footage) = match &node.operation {
        Operation::Footage(veylvik_document::nodes::Footage { timing: t, .. })
        | Operation::Sound(veylvik_document::nodes::Sound { timing: t, .. }) => {
            let first = frames(t.start, fps);
            (
                first,
                t.source_out
                    .map(|out| first + frames(out, fps) - frames(t.source_in, fps)),
                true,
            )
        }
        Operation::Fade(fade) => (
            frames(fade.timing.start, fps),
            fade.timing.end.map(|end| frames(end, fps)),
            false,
        ),
        _ => return None,
    };
    Some(Bar {
        id: node.id,
        label: node.label.clone(),
        first,
        last,
        footage,
    })
}
pub(crate) fn bars(doc: &Composition) -> Vec<Bar> {
    doc.nodes
        .values()
        .filter_map(|node| bar(node, doc.frame_rate))
        .collect()
}
/// `node` with `part` of its bar moved by `by` frames; None when nothing would change or
/// the result is not a layer that can exist (before time zero, or shorter than a frame).
pub(crate) fn moved(node: &Node, part: Part, by: i64, doc: &Composition) -> Option<Node> {
    if by == 0 {
        return None;
    }
    let fps = doc.frame_rate;
    let count = i64::from(doc.frame_count);
    let mut next = node.clone();
    match &mut next.operation {
        Operation::Footage(veylvik_document::nodes::Footage { timing: t, .. })
        | Operation::Sound(veylvik_document::nodes::Sound { timing: t, .. }) => {
            match part {
                Part::Body => t.start = shifted(t.start, by.max(-frames(t.start, fps)), fps)?,
                Part::Start => {
                    // Trimming the head moves the clip's start and its first source frame together.
                    let by = by.max(-frames(t.start, fps)).max(-frames(t.source_in, fps));
                    if by == 0 {
                        return None;
                    }
                    t.start = shifted(t.start, by, fps)?;
                    t.source_in = shifted(t.source_in, by, fps)?;
                }
                Part::End => {
                    let out = match t.source_out {
                        Some(out) => out,
                        // An open clip gets an end where its bar ended: the composition's.
                        None => shifted(t.source_in, (count - frames(t.start, fps)).max(1), fps)?,
                    };
                    t.source_out = Some(shifted(out, by, fps)?);
                }
            }
            t.validate().ok()?;
            if let Some(out) = t.source_out
                && frames(out, fps) - frames(t.source_in, fps) < 1
            {
                return None;
            }
        }
        Operation::Fade(fade) => {
            let t = &mut fade.timing;
            match part {
                Part::Body => {
                    let by = by.max(-frames(t.start, fps));
                    if by == 0 {
                        return None;
                    }
                    t.start = shifted(t.start, by, fps)?;
                    if let Some(end) = t.end {
                        t.end = Some(shifted(end, by, fps)?);
                    }
                }
                Part::Start => t.start = shifted(t.start, by.max(-frames(t.start, fps)), fps)?,
                Part::End => {
                    let end = match t.end {
                        Some(end) => end,
                        None => Time::at_frame(count, fps).ok()?,
                    };
                    t.end = Some(shifted(end, by, fps)?);
                }
            }
            t.validate().ok()?;
            if let Some(end) = t.end
                && frames(end, fps) - frames(t.start, fps) < 1
            {
                return None;
            }
        }
        _ => return None,
    }
    (bar(&next, fps) != bar(node, fps)).then_some(next)
}
/// `by` adjusted so that the edge being dragged lands on a target frame that is within
/// `reach` frames of it. Dragging the body tries its start first, then its end.
pub(crate) fn snapped(
    by: i64,
    bar: &Bar,
    part: Part,
    count: i64,
    targets: &[i64],
    reach: i64,
) -> i64 {
    let last = bar.last.unwrap_or(count);
    let edges: &[i64] = match part {
        Part::Start => &[bar.first],
        Part::End => &[last],
        Part::Body => &[bar.first, last],
    };
    edges
        .iter()
        .flat_map(|edge| targets.iter().map(move |target| target - (edge + by)))
        .filter(|gap| gap.abs() <= reach)
        .min_by_key(|gap| gap.abs())
        .map_or(by, |gap| by + gap)
}
#[derive(Default)]
pub struct LayerBars {
    pub hidden: bool,
    /// The bar being dragged: node, part, and whole frames moved so far.
    drag: Option<(NodeId, Part, f32)>,
}
impl LayerBars {
    /// Draw the bars across `span`, the ruler's horizontal range.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        doc: &Composition,
        span: Rangef,
        selected: NodeId,
        current: i64,
        editable: bool,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.hidden {
            return actions;
        }
        let all = bars(doc);
        let count = i64::from(doc.frame_count);
        let end = (count - 1).max(1) as f32;
        let x = |frame: i64| span.min + frame as f32 / end * span.span();
        let per_frame = span.span() / end;
        // What a dragged edge may land on: the composition's ends, the playhead, markers
        // and the edges of the other bars. Alt drags freely.
        let free = ui.input(|i| i.modifiers.alt);
        let reach = (6.0 / per_frame.max(1e-6)).round() as i64;
        let targets = |dragged: NodeId| -> Vec<i64> {
            let mut frames = vec![0, count, current];
            frames.extend(
                crate::timeline_markers::frames(doc)
                    .into_iter()
                    .map(|(frame, _)| frame),
            );
            for other in all.iter().filter(|other| other.id != dragged) {
                frames.push(other.first);
                frames.extend(other.last);
            }
            frames
        };
        let steps = |bar: &Bar, part: Part, by: f32| {
            let by = (by / per_frame).round() as i64;
            if free || by == 0 {
                by
            } else {
                snapped(by, bar, part, count, &targets(bar.id), reach)
            }
        };
        for bar in all.iter().take(ROWS) {
            let (row, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::hover());
            let Some(node) = doc.nodes.get(&bar.id) else {
                continue;
            };
            // While dragged, the bar is drawn where it would land.
            let dragged = self
                .drag
                .filter(|(id, ..)| *id == bar.id)
                .and_then(|(_, part, by)| moved(node, part, steps(bar, part, by), doc));
            let shown = dragged
                .as_ref()
                .and_then(|node| self::bar(node, doc.frame_rate))
                .unwrap_or_else(|| bar.clone());
            let lane = Rect::from_x_y_ranges(span, row.y_range()).shrink2(egui::vec2(0.0, 2.0));
            let painter = ui.painter();
            painter.rect_filled(lane, 2.0, crate::theme::palette(ui.ctx()).canvas);
            let gutter = Rect::from_min_max(row.min, egui::pos2(span.min - 6.0, row.max.y));
            if gutter.width() > 20.0 {
                let galley = painter.layout_no_wrap(
                    bar.label.clone(),
                    FontId::proportional(11.0),
                    if bar.id == selected {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    },
                );
                let at = egui::pos2(
                    (gutter.right() - galley.size().x).max(gutter.left()),
                    gutter.center().y - galley.size().y * 0.5,
                );
                ui.painter()
                    .with_clip_rect(gutter)
                    .galley(at, galley, Color32::WHITE);
            }
            let last = shown.last.unwrap_or(count).min(count);
            let body = Rect::from_min_max(
                egui::pos2(x(shown.first.clamp(0, count)), lane.top()),
                egui::pos2(
                    x(last.max(shown.first)).max(x(shown.first) + 3.0),
                    lane.bottom(),
                ),
            )
            .intersect(lane);
            let colour = if bar.footage {
                Color32::from_rgb(70, 125, 190)
            } else {
                Color32::from_rgb(150, 110, 200)
            };
            let painter = ui.painter().with_clip_rect(lane);
            painter.rect_filled(
                body,
                3.0,
                colour.gamma_multiply(if bar.id == selected { 1.0 } else { 0.7 }),
            );
            if shown.last.is_none() {
                // Open ended: it runs as long as the media or the composition does.
                painter.rect_filled(
                    Rect::from_min_max(egui::pos2(body.right() - 3.0, body.top()), body.max),
                    0.0,
                    Color32::from_white_alpha(70),
                );
            }
            if bar.id == selected {
                painter.rect_stroke(
                    body,
                    3.0,
                    Stroke::new(1.0_f32, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            let grip = 6.0_f32.min(body.width() / 3.0);
            for (part, area) in [
                (Part::Body, body.shrink2(egui::vec2(grip, 0.0))),
                (
                    Part::Start,
                    Rect::from_min_max(body.min, egui::pos2(body.left() + grip, body.bottom())),
                ),
                (
                    Part::End,
                    Rect::from_min_max(egui::pos2(body.right() - grip, body.top()), body.max),
                ),
            ] {
                let response = ui.interact(
                    area,
                    ui.id().with(("layer-bar", bar.id, part as u8)),
                    egui::Sense::click_and_drag(),
                );
                let response = response
                    .on_hover_cursor(if part == Part::Body {
                        egui::CursorIcon::Grab
                    } else {
                        egui::CursorIcon::ResizeHorizontal
                    })
                    .on_hover_text(match part {
                        Part::Body => "Drag to slide this layer in time; click to select it. Edges snap to the playhead, markers and other layers; hold Alt to drag freely",
                        Part::Start => "Drag to trim where this layer starts",
                        Part::End => "Drag to trim where this layer ends",
                    });
                if response.clicked() {
                    actions.push(Action::Select(bar.id));
                }
                if !editable {
                    continue;
                }
                if response.drag_started() {
                    self.drag = Some((bar.id, part, 0.0));
                }
                if response.dragged()
                    && let Some((id, held, by)) = &mut self.drag
                    && *id == bar.id
                    && *held == part
                {
                    *by += response.drag_delta().x;
                }
                if response.drag_stopped()
                    && let Some((id, held, by)) = self.drag.take()
                    && id == bar.id
                    && held == part
                    && let Some(node) = moved(node, part, steps(bar, part, by), doc)
                {
                    actions.push(Action::Commit(vec![Command::Put {
                        node: Box::new(node),
                    }]));
                }
            }
        }
        if all.len() > ROWS {
            ui.weak(format!("{} more layers are not shown", all.len() - ROWS));
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::nodes::{Fade, Footage};
    fn t(ticks: i64, rate: u32) -> Time {
        Time::new(ticks, rate).unwrap()
    }
    fn doc() -> Composition {
        let mut doc = veylvik_document::blank(64, 36, t(24, 1)).unwrap();
        doc.frame_count = 240;
        doc
    }
    fn clip(start: Time, source_in: Time, source_out: Option<Time>) -> Node {
        let mut footage = Footage {
            asset: "clip.mp4".into(),
            timing: Default::default(),
            placement: Default::default(),
            colour: Default::default(),
            layer: None,
        };
        (
            footage.timing.start,
            footage.timing.source_in,
            footage.timing.source_out,
        ) = (start, source_in, source_out);
        Node::new("Clip", Operation::Footage(footage), &[])
    }
    fn span(node: &Node) -> (i64, Option<i64>) {
        let bar = bar(node, t(24, 1)).unwrap();
        (bar.first, bar.last)
    }
    #[test]
    fn footage_slides_and_trims_in_whole_frames_without_leaving_time_zero() {
        let doc = doc();
        // Starts at 1 s, plays source 2 s..5 s: frames 24..96.
        let node = clip(t(1, 1), t(2, 1), Some(t(5, 1)));
        assert_eq!(span(&node), (24, Some(96)));
        let slid = moved(&node, Part::Body, 12, &doc).unwrap();
        assert_eq!(span(&slid), (36, Some(108)));
        // Sliding stops at time zero and keeps the length.
        assert_eq!(
            span(&moved(&node, Part::Body, -1000, &doc).unwrap()),
            (0, Some(72))
        );
        // Trimming the head moves start and source in together: the end stays put.
        let head = moved(&node, Part::Start, 10, &doc).unwrap();
        assert_eq!(span(&head), (34, Some(96)));
        let Operation::Footage(f) = &head.operation else {
            unreachable!()
        };
        assert_eq!(frames(f.timing.source_in, t(24, 1)), 58);
        // Pulling the head earlier stops where the timeline or the source begins.
        assert_eq!(
            span(&moved(&node, Part::Start, -1000, &doc).unwrap()),
            (0, Some(96))
        );
        assert_eq!(
            span(&moved(&node, Part::End, -24, &doc).unwrap()),
            (24, Some(72))
        );
        // Never shorter than a frame, and no change is no edit.
        assert!(moved(&node, Part::End, -72, &doc).is_none());
        assert!(moved(&node, Part::Start, 72, &doc).is_none());
        assert!(moved(&node, Part::Body, 0, &doc).is_none());
        assert!(moved(&clip(t(0, 1), t(0, 1), None), Part::Body, -5, &doc).is_none());
        // An open clip gets its end from where the bar ended.
        let open = clip(t(1, 1), t(0, 1), None);
        assert_eq!(span(&open), (24, None));
        assert_eq!(
            span(&moved(&open, Part::End, -24, &doc).unwrap()),
            (24, Some(216))
        );
    }
    #[test]
    fn a_dragged_edge_lands_on_a_nearby_target_and_not_on_a_far_one() {
        let bar = Bar {
            id: uuid_like(),
            label: String::new(),
            first: 24,
            last: Some(96),
            footage: true,
        };
        let targets = [0, 240, 50, 120];
        // The start, 3 frames short of the marker at 50, lands on it.
        assert_eq!(snapped(23, &bar, Part::Start, 240, &targets, 4), 26);
        // Too far from anything: left alone.
        assert_eq!(snapped(10, &bar, Part::Start, 240, &targets, 4), 10);
        // The end near 120.
        assert_eq!(snapped(22, &bar, Part::End, 240, &targets, 4), 24);
        // The body: whichever edge is closest to a target decides.
        assert_eq!(
            snapped(22, &bar, Part::Body, 240, &targets, 4),
            24,
            "the end reaches 120"
        );
        assert_eq!(
            snapped(-23, &bar, Part::Body, 240, &targets, 4),
            -24,
            "the start reaches 0"
        );
        // An open-ended bar's end is the composition's end.
        let open = Bar {
            last: None,
            ..bar.clone()
        };
        assert_eq!(snapped(-118, &open, Part::End, 240, &targets, 4), -120);
        assert_eq!(snapped(5, &bar, Part::Start, 240, &[], 4), 5);
    }
    fn uuid_like() -> NodeId {
        Node::new(
            "x",
            Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
            &[],
        )
        .id
    }
    #[test]
    fn fades_move_with_both_ends_and_bars_list_only_timed_layers() {
        let mut doc = doc();
        let mut fade = Fade {
            opacity: 1.0.into(),
            timing: Default::default(),
        };
        (fade.timing.start, fade.timing.end) = (t(2, 1), Some(t(4, 1)));
        let node = Node::new("Title in", Operation::Fade(fade), &[("image", doc.output)]);
        assert_eq!(span(&node), (48, Some(96)));
        assert_eq!(
            span(&moved(&node, Part::Body, -24, &doc).unwrap()),
            (24, Some(72))
        );
        assert_eq!(
            span(&moved(&node, Part::Start, 12, &doc).unwrap()),
            (60, Some(96))
        );
        assert_eq!(
            span(&moved(&node, Part::End, 24, &doc).unwrap()),
            (48, Some(120))
        );
        assert!(moved(&node, Part::Start, 48, &doc).is_none());
        let background = doc.nodes[&doc.output].clone();
        assert!(bar(&background, doc.frame_rate).is_none());
        assert!(moved(&background, Part::Body, 5, &doc).is_none());
        let id = node.id;
        doc.nodes.insert(id, node);
        let listed = bars(&doc);
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].id, listed[0].footage), (id, false));
    }
}
