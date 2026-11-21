//! A node's result kept between frames while it does not change: a pass is known by what it is made of (cache::pass_identities), a node whose result was the same in the frame before is kept where a changing one reads it, and the next frame copies it in and skips every pass that made it.
use crate::plan::{Input, Plan};
use std::collections::{HashMap, VecDeque};

/// What one render does with each pass.
pub(crate) struct Decision {
    /// Whether the pass runs; a pass whose every reader is answered from a kept result
    /// does not.
    pub run: Vec<bool>,
    /// A kept result that stands for the pass: copied into its slot instead of running it.
    pub kept: Vec<Option<wgpu::Buffer>>,
    /// Whether the pass's result is to be kept once it is made.
    pub keep: Vec<bool>,
}
#[derive(Default)]
pub(crate) struct Results {
    /// Each node's identity in the frame before, by node id.
    last: HashMap<veylvik_document::NodeId, u64>,
    /// Kept results, most recently used first.
    kept: VecDeque<(u64, wgpu::Buffer)>,
    bytes: u64,
    /// Passes answered from a kept result, since the start.
    pub hits: u64,
}
/// Whether pass `at` is the last of its node (the one whose result other nodes read).
fn last_of_node(plan: &Plan, at: usize) -> bool {
    plan.passes
        .get(at + 1)
        .is_none_or(|next| next.node != plan.passes[at].node)
}
impl Results {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.kept.len()
    }
    /// Let go of the least recently used result; false when none is kept.
    pub fn evict(&mut self) -> bool {
        match self.kept.pop_back() {
            Some((_, buffer)) => {
                self.bytes -= buffer.size();
                true
            }
            None => false,
        }
    }
    pub fn clear(&mut self) {
        while self.evict() {}
        self.last.clear();
    }
    /// Decide for every pass of `plan`: `ids` are the passes' identities, `visible` the
    /// nodes that reach the picture now. With `room` at 0 nothing new is kept.
    pub fn decide(
        &mut self,
        plan: &Plan,
        ids: &[Option<u64>],
        visible: &std::collections::BTreeSet<veylvik_document::NodeId>,
        room: u64,
    ) -> Decision {
        let count = plan.passes.len();
        // Which pass wrote each value a pass reads.
        let mut writer: Vec<Option<usize>> = vec![None; plan.slot_count];
        let mut feeds: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (at, pass) in plan.passes.iter().enumerate() {
            for input in pass.inputs.iter().chain(pass.cards.iter().map(|(_, i)| i)) {
                if let Input::Value(slot) = input
                    && let Some(q) = writer[*slot]
                {
                    feeds[at].push(q);
                }
            }
            writer[pass.output] = Some(at);
        }
        let shown = |at: usize| visible.contains(&plan.nodes[plan.passes[at].node].id);
        // A node's result is kept or reused only as a whole, never the final picture
        // (the frame cache holds that) and never a pass whose identity is unknown.
        let candidate = |at: usize| {
            last_of_node(plan, at)
                && shown(at)
                && ids[at].is_some()
                && Some(at) != writer[plan.output_slot]
        };
        let mut kept: Vec<Option<wgpu::Buffer>> = vec![None; count];
        for at in 0..count {
            if candidate(at)
                && let Some(id) = ids[at]
                && let Some(place) = self.kept.iter().position(|(k, _)| *k == id)
            {
                // Used again: to the front.
                let entry = self.kept.remove(place).expect("found above");
                self.hits += 1;
                kept[at] = Some(entry.1.clone());
                self.kept.push_front(entry);
            }
        }
        // What has to run: from the picture back, stopping at kept results.
        let mut run = vec![false; count];
        let mut pending: Vec<usize> = writer[plan.output_slot].into_iter().collect();
        while let Some(at) = pending.pop() {
            if run[at] {
                continue;
            }
            run[at] = true;
            if kept[at].is_none() {
                pending.extend(feeds[at].iter().copied());
            }
        }
        // A node unchanged since the frame before, read by a node that changed: keep it,
        // so the next frame starts from there.
        let stable = |at: usize| {
            ids[at]
                .is_some_and(|id| self.last.get(&plan.nodes[plan.passes[at].node].id) == Some(&id))
        };
        let last_pass = |at: usize| (at..count).find(|&q| last_of_node(plan, q)).unwrap_or(at);
        let mut keep = vec![false; count];
        let mut wanted = 0;
        let frame = u64::from(plan.width) * u64::from(plan.height) * 16;
        for at in 0..count {
            if run[at] && kept[at].is_none() && candidate(at) && stable(at) {
                let changing_reader =
                    (0..count).any(|c| run[c] && feeds[c].contains(&at) && !stable(last_pass(c)));
                if changing_reader && wanted + frame <= room {
                    keep[at] = true;
                    wanted += frame;
                }
            }
        }
        for (at, id) in ids.iter().enumerate() {
            if last_of_node(plan, at)
                && let Some(id) = *id
                && shown(at)
            {
                self.last.insert(plan.nodes[plan.passes[at].node].id, id);
            }
        }
        Decision { run, kept, keep }
    }
    /// Keep `buffer` as the result named `id`, letting the oldest go past `limit` bytes.
    pub fn insert(&mut self, id: u64, buffer: wgpu::Buffer, limit: u64) {
        self.bytes += buffer.size();
        self.kept.push_front((id, buffer));
        while self.bytes > limit && self.kept.len() > 1 {
            self.evict();
        }
    }
}
