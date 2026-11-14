//! Deterministic branch-aware layout of the graph the user actually sees.
use super::graph::{NODE_SIZE, VisualNode};
use eframe::egui::{self, Pos2};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::NodeId;
pub(super) fn arrange(nodes: &[VisualNode], output: NodeId) -> BTreeMap<NodeId, Pos2> {
    let by_id: BTreeMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
    let mut consumers = BTreeMap::<NodeId, Vec<NodeId>>::new();
    for node in nodes {
        for (_, source) in &node.inputs {
            if by_id.contains_key(source) {
                consumers.entry(*source).or_default().push(node.id);
            }
        }
    }
    let mut roots: Vec<_> = nodes
        .iter()
        .filter(|n| !consumers.contains_key(&n.id))
        .map(|n| n.id)
        .collect();
    roots.sort_by_key(|id| (*id != output, by_id[id].label.as_str(), *id));
    // Rank against every sink, including branches that share sources with the output.
    let mut pending: BTreeMap<_, _> = nodes
        .iter()
        .map(|n| (n.id, consumers.get(&n.id).map_or(0, Vec::len)))
        .collect();
    let mut ready = roots.clone();
    let mut ranks = BTreeMap::new();
    while let Some(id) = ready.pop() {
        let rank = ranks.get(&id).copied().unwrap_or(0usize);
        ranks.entry(id).or_insert(rank);
        for (_, source) in &by_id[&id].inputs {
            if let Some(count) = pending.get_mut(source) {
                ranks
                    .entry(*source)
                    .and_modify(|r| *r = (*r).max(rank + 1))
                    .or_insert(rank + 1);
                *count -= 1;
                if *count == 0 {
                    ready.push(*source);
                }
            }
        }
    }
    let max_rank = ranks.values().copied().max().unwrap_or(0);
    if by_id.contains_key(&output) {
        roots.retain(|id| *id != output);
        roots.insert(0, output);
    }
    let mut result = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut base_y = 20.0;
    for root in roots {
        if visited.contains(&root) {
            continue;
        }
        let mut post = Vec::new();
        let mut todo = vec![(root, false)];
        while let Some((id, expanded)) = todo.pop() {
            if expanded {
                post.push(id);
            } else if visited.insert(id) {
                todo.push((id, true));
                for (_, source) in by_id[&id].inputs.iter().rev() {
                    if by_id.contains_key(source) {
                        todo.push((*source, false));
                    }
                }
            }
        }
        let mut lanes = BTreeMap::<usize, BTreeSet<i32>>::new();
        let mut row = BTreeMap::new();
        let mut next_leaf = 0;
        for id in post {
            let column = max_rank - ranks[&id];
            let mut inputs: Vec<_> = by_id[&id]
                .inputs
                .iter()
                .filter_map(|(_, source)| row.get(source).copied())
                .collect();
            inputs.sort_unstable();
            let preferred = if inputs.is_empty() {
                let r = next_leaf;
                next_leaf += 1;
                r
            } else {
                inputs[inputs.len() / 2]
            };
            let occupied = lanes.entry(column).or_default();
            let mut chosen = preferred;
            while occupied.contains(&chosen) {
                chosen += 1;
            }
            occupied.insert(chosen);
            row.insert(id, chosen);
            result.insert(
                id,
                egui::pos2(
                    25.0 + column as f32 * (NODE_SIZE.x + 60.0),
                    base_y + chosen as f32 * (NODE_SIZE.y + 40.0),
                ),
            );
        }
        base_y += row.values().copied().max().unwrap_or(0) as f32 * (NODE_SIZE.y + 40.0)
            + NODE_SIZE.y
            + 120.0;
    }
    result
}
/// Spread the branch layout across the pane while retaining readable minimum gaps.
pub(super) fn adapt(positions: &mut BTreeMap<NodeId, Pos2>, size: egui::Vec2) {
    if positions.is_empty() || size.x <= 30.0 || size.y <= 30.0 {
        return;
    }
    let max_x = positions
        .values()
        .map(|p| p.x - 25.0)
        .fold(0.0_f32, f32::max);
    let max_y = positions
        .values()
        .map(|p| p.y - 20.0)
        .fold(0.0_f32, f32::max);
    let ratio = (size.x - 30.0) / (size.y - 30.0);
    let current = (max_x + NODE_SIZE.x) / (max_y + NODE_SIZE.y);
    let (sx, sy) = if ratio > current {
        (
            ((ratio * (max_y * 0.8 + NODE_SIZE.y) - NODE_SIZE.x) / max_x.max(1.0)).clamp(1.0, 2.5),
            0.8,
        )
    } else {
        (
            1.0,
            (((max_x + NODE_SIZE.x) / ratio - NODE_SIZE.y) / max_y.max(1.0)).clamp(0.8, 2.5),
        )
    };
    for p in positions.values_mut() {
        p.x = 25.0 + (p.x - 25.0) * sx;
        p.y = 20.0 + (p.y - 20.0) * sy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: NodeId, label: &str, inputs: &[NodeId]) -> VisualNode {
        VisualNode {
            id,
            label: label.into(),
            subtitle: String::new(),
            group: None,
            inputs: inputs
                .iter()
                .enumerate()
                .map(|(i, id)| (i.to_string(), *id))
                .collect(),
        }
    }
    #[test]
    fn responsive_spacing_uses_both_axes_without_overlapping_nodes() {
        let a = NodeId::new_v4();
        let b = NodeId::new_v4();
        let c = NodeId::new_v4();
        let base = BTreeMap::from([
            (a, egui::pos2(25.0, 20.0)),
            (b, egui::pos2(245.0, 120.0)),
            (c, egui::pos2(465.0, 220.0)),
        ]);
        let mut wide = base.clone();
        let mut tall = base;
        adapt(&mut wide, egui::vec2(1600.0, 300.0));
        adapt(&mut tall, egui::vec2(400.0, 1000.0));
        assert!(wide[&c].x > tall[&c].x);
        assert!(tall[&c].y > wide[&c].y);
        for layout in [wide, tall] {
            assert!(layout[&b].x - layout[&a].x >= 220.0);
            assert!(layout[&b].y - layout[&a].y >= 80.0);
        }
    }
    #[test]
    fn short_branches_stay_near_merge_and_disconnected_nodes_stay_separate() {
        let ids: Vec<_> = (0..7).map(|_| NodeId::new_v4()).collect();
        let nodes = vec![
            node(ids[0], "Background", &[]),
            node(ids[1], "Text", &[]),
            node(ids[2], "Fill", &[ids[1]]),
            node(ids[3], "Glow", &[ids[2]]),
            node(ids[4], "Composite", &[ids[0], ids[3]]),
            node(ids[5], "Unused", &[]),
            node(ids[6], "Unused effect", &[ids[5]]),
        ];
        let layout = arrange(&nodes, ids[4]);
        assert_eq!(layout[&ids[4]].x - layout[&ids[0]].x, NODE_SIZE.x + 60.0);
        assert_eq!(layout[&ids[1]].y, layout[&ids[3]].y);
        assert!(layout[&ids[5]].y > layout[&ids[4]].y + NODE_SIZE.y);
        for n in &nodes {
            for (_, source) in &n.inputs {
                assert!(layout[source].x < layout[&n.id].x);
            }
        }
        for (a, p) in &layout {
            for (b, q) in &layout {
                if a != b {
                    assert!((p.x - q.x).abs() >= NODE_SIZE.x || (p.y - q.y).abs() >= NODE_SIZE.y);
                }
            }
        }
        let mut reversed = nodes;
        reversed.reverse();
        assert_eq!(layout, arrange(&reversed, ids[4]));
    }
}
