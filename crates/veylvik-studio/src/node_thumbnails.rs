//! Small pictures of what each node outputs, on its tile in the graph. Off unless asked for; rendered one node at a time on a worker and only while Studio is idle, and kept until that node, anything upstream of it or the frame changes.
use crate::Studio;
use eframe::egui;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use veylvik_document::{Composition, NodeId, registry::PortKind};

/// Longest edge of a thumbnail in pixels.
const EDGE: u32 = 112;
/// How long the frame must stand still before pictures are made for it.
const SETTLE: Duration = Duration::from_millis(350);

type Made = Result<egui::ColorImage, String>;
#[derive(Default)]
pub struct Thumbnails {
    keys: Option<((NodeId, u64), BTreeMap<NodeId, u64>)>,
    /// What each node's picture was made from, and the picture.
    cache: BTreeMap<NodeId, ((u64, i64), egui::TextureHandle)>,
    /// Nodes whose render failed for this input; not tried again until it changes.
    failed: BTreeMap<NodeId, (u64, i64)>,
    pending: Option<(NodeId, (u64, i64), mpsc::Receiver<Made>)>,
    frame: Option<(i64, Instant)>,
    /// The graph's switch has been set from preferences once.
    started: bool,
}
/// For every node, a number that changes when it or anything upstream of it changes.
pub(crate) fn keys(doc: &Composition) -> BTreeMap<NodeId, u64> {
    let mut keys = BTreeMap::new();
    for start in doc.nodes.keys() {
        // Depth first without recursion: a node is finished once its inputs are.
        let mut stack = vec![*start];
        while let Some(id) = stack.last().copied() {
            if keys.contains_key(&id) {
                stack.pop();
                continue;
            }
            let Some(node) = doc.nodes.get(&id) else {
                stack.pop();
                continue;
            };
            let waiting: Vec<NodeId> = node
                .inputs
                .values()
                .filter(|input| !keys.contains_key(*input) && doc.nodes.contains_key(*input))
                .copied()
                .collect();
            // A validated composition has no cycles; the length check guards one anyway.
            if !waiting.is_empty() && stack.len() <= doc.nodes.len() {
                stack.extend(waiting);
                continue;
            }
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            serde_json::to_string(&node.operation)
                .unwrap_or_default()
                .hash(&mut hasher);
            (node.bypass, doc.width, doc.height).hash(&mut hasher);
            for (port, input) in &node.inputs {
                (port, keys.get(input)).hash(&mut hasher);
            }
            keys.insert(id, hasher.finish());
            stack.pop();
        }
    }
    keys
}
/// Average premultiplied pixels down so the longest edge is at most `edge`.
pub(crate) fn reduced(
    pixels: &[[f32; 4]],
    width: u32,
    height: u32,
    edge: u32,
) -> (u32, u32, Vec<[f32; 4]>) {
    let step = width.max(height).div_ceil(edge.max(1)).max(1);
    let (w, h) = (width.div_ceil(step), height.div_ceil(step));
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (mut sum, mut count) = ([0.0_f32; 4], 0.0_f32);
            for sy in y * step..((y + 1) * step).min(height) {
                for sx in x * step..((x + 1) * step).min(width) {
                    let pixel = pixels[(sy * width + sx) as usize];
                    for channel in 0..4 {
                        sum[channel] += pixel[channel];
                    }
                    count += 1.0;
                }
            }
            out.push(sum.map(|v| v / count.max(1.0)));
        }
    }
    (w, h, out)
}
impl Studio {
    /// Hand the graph its pictures and, when idle, make the next missing one.
    pub(crate) fn thumbnails_tick(&mut self, ctx: &egui::Context) {
        if !self.thumbnails.started {
            self.thumbnails.started = true;
            self.graph.show_thumbnails = self.preferences.values.node_thumbnails;
        }
        if self.graph.show_thumbnails != self.preferences.values.node_thumbnails {
            self.preferences.values.node_thumbnails = self.graph.show_thumbnails;
            if let Err(error) = self.preferences.persist() {
                self.fail(format!("Preferences: {error}"));
            }
        }
        if !self.graph.show_thumbnails {
            if !self.thumbnails.cache.is_empty() {
                self.thumbnails = Thumbnails {
                    started: true,
                    ..Default::default()
                };
                self.graph.thumbnails.clear();
            }
            return;
        }
        if let Some((id, key, rx)) = &self.thumbnails.pending
            && let Ok(made) = rx.try_recv()
        {
            let (id, key) = (*id, *key);
            self.thumbnails.pending = None;
            match made {
                Ok(image) => {
                    let texture =
                        ctx.load_texture(format!("node-{id}"), image, egui::TextureOptions::LINEAR);
                    self.thumbnails.cache.insert(id, (key, texture));
                }
                Err(_) => {
                    self.thumbnails.failed.insert(id, key);
                }
            }
        }
        let doc = self.project.document.snapshot();
        let seen = (self.project.session, self.project.document.revision());
        if self.thumbnails.keys.as_ref().map(|(at, _)| *at) != Some(seen) {
            self.thumbnails.keys = Some((seen, keys(doc)));
            self.thumbnails
                .cache
                .retain(|id, _| doc.nodes.contains_key(id));
            self.thumbnails
                .failed
                .retain(|id, _| doc.nodes.contains_key(id));
        }
        self.graph.thumbnails = self
            .thumbnails
            .cache
            .iter()
            .map(|(id, (_, texture))| (*id, (texture.id(), texture.size_vec2())))
            .collect();
        let frame = self.project.frame;
        if self.thumbnails.frame.map(|(at, _)| at) != Some(frame) {
            self.thumbnails.frame = Some((frame, Instant::now()));
        }
        let settled = self
            .thumbnails
            .frame
            .is_some_and(|(_, since)| since.elapsed() >= SETTLE);
        let Some((_, keys)) = &self.thumbnails.keys else {
            return;
        };
        // The next tile on screen whose picture is missing or out of date.
        let next = self.graph.visible.iter().copied().find_map(|id| {
            let node = doc.nodes.get(&id)?;
            let wanted = (*keys.get(&id)?, frame);
            (node.operation.descriptor().output == PortKind::Image
                && self.thumbnails.cache.get(&id).map(|(key, _)| *key) != Some(wanted)
                && self.thumbnails.failed.get(&id) != Some(&wanted))
            .then_some((id, wanted))
        });
        let Some((id, key)) = next else {
            return;
        };
        let idle = !self.playing
            && !self.draft_dirty
            && self.displayed_ticket == self.wanted
            && !self.graph.dragging()
            && !self.files.busy();
        if self.thumbnails.pending.is_some() || !idle || !settled {
            // Look again shortly; nothing else may wake the interface.
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        let Ok(time) = self.project.time() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let (renderer, inputs, document, ctx) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            doc.clone(),
            ctx.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("node-thumbnail".into())
            .spawn(move || {
                let made = (|| -> Made {
                    let plan = veylvik_render::Plan::compile_output(&document, id)
                        .map_err(|e| e.to_string())?;
                    let frame = renderer
                        .render_gpu(&plan, veylvik_render::RenderRequest::full(time), &inputs)
                        .map_err(|e| e.to_string())?;
                    // Shrink on the GPU first, so little is read back.
                    let frame = renderer
                        .reduce_preview(std::sync::Arc::new(frame), 4)
                        .map_err(|e| e.to_string())?;
                    let read = renderer.read_frame(&frame).map_err(|e| e.to_string())?;
                    let (w, h, small) =
                        reduced(&read.pixels, read.region.width, read.region.height, EDGE);
                    Ok(egui::ColorImage::from_rgba_unmultiplied(
                        [w as usize, h as usize],
                        &veylvik_render::to_srgba8(&small),
                    ))
                })();
                let _ = tx.send(made);
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.thumbnails.pending = Some((id, key, rx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, Operation, Time, nodes};
    #[test]
    fn a_key_changes_only_for_the_edited_node_and_what_it_feeds() {
        let mut doc = veylvik_document::blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
        let source = doc.output;
        let blur = Node::new(
            "Blur",
            Operation::Blur(nodes::Blur {
                radius: 2.0.into(),
                filter: Default::default(),
            }),
            &[("image", source)],
        );
        let other = Node::new(
            "Other",
            Operation::Solid(nodes::Solid { color: [1.0; 4] }),
            &[],
        );
        let (blur_id, other_id) = (blur.id, other.id);
        doc.nodes.insert(blur_id, blur);
        doc.nodes.insert(other_id, other);
        let before = keys(&doc);
        assert_eq!(before.len(), 3);
        assert_eq!(keys(&doc), before, "the same graph gives the same keys");
        // Editing the source changes it and the blur after it, not the unrelated node.
        doc.nodes.get_mut(&source).unwrap().operation = Operation::Solid(nodes::Solid {
            color: [0.5, 0.0, 0.0, 1.0],
        });
        let after = keys(&doc);
        assert_ne!(after[&source], before[&source]);
        assert_ne!(after[&blur_id], before[&blur_id]);
        assert_eq!(after[&other_id], before[&other_id]);
        // A label, tag or note is not part of the picture; the frame size is.
        let node = doc.nodes.get_mut(&blur_id).unwrap();
        node.label = "Soft".into();
        node.note = "check".into();
        assert_eq!(keys(&doc), after);
        doc.width = 128;
        assert_ne!(keys(&doc)[&other_id], after[&other_id]);
    }
    #[test]
    fn reduction_averages_blocks_and_keeps_the_shape() {
        // 4 x 2 with a bright left half, reduced to at most 2 wide.
        let pixels: Vec<[f32; 4]> = (0..8)
            .map(|i| if i % 4 < 2 { [1.0; 4] } else { [0.0; 4] })
            .collect();
        let (w, h, small) = reduced(&pixels, 4, 2, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(small, vec![[1.0; 4], [0.0; 4]]);
        // Already small enough: unchanged. Odd sizes: the last block is partial.
        let (w, h, same) = reduced(&pixels, 4, 2, 8);
        assert_eq!((w, h, same.len()), (4, 2, 8));
        let (w, h, odd) = reduced(&[[2.0; 4]; 15], 5, 3, 2);
        assert_eq!((w, h), (2, 1));
        assert!(odd.iter().all(|p| *p == [2.0; 4]));
    }
}
