//! Deterministic branch-aware layout of the graph the user actually sees.
use super::graph::VisualNode;
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
                egui::pos2(25.0 + column as f32 * 220.0, base_y + chosen as f32 * 100.0),
            );
        }
        base_y += row.values().copied().max().unwrap_or(0) as f32 * 100.0 + 180.0;
    }
    result
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
        assert_eq!(layout[&ids[4]].x - layout[&ids[0]].x, 220.0);
        assert_eq!(layout[&ids[1]].y, layout[&ids[3]].y);
        assert!(layout[&ids[5]].y > layout[&ids[4]].y + 60.0);
        for n in &nodes {
            for (_, source) in &n.inputs {
                assert!(layout[source].x < layout[&n.id].x);
            }
        }
        for (a, p) in &layout {
            for (b, q) in &layout {
                if a != b {
                    assert!((p.x - q.x).abs() >= 160.0 || (p.y - q.y).abs() >= 60.0);
                }
            }
        }
        let mut reversed = nodes;
        reversed.reverse();
        assert_eq!(layout, arrange(&reversed, ids[4]));
    }
}
