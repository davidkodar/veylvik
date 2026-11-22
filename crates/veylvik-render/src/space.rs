//! What a whole plan says about space, asked of its nodes: where the output can be other than clear, and how much of a source an area of it is made from. A node that says nothing counts as everything, so an answer is never too small.
use super::*;

impl Plan {
    fn sampler(
        &self,
        index: usize,
        time: veylvik_document::Time,
    ) -> impl Fn(&str) -> Result<f32> + '_ {
        move |name: &str| {
            self.curves[index]
                .get(name)
                .ok_or_else(|| error("compiled scalar parameter missing"))?
                .at(time)
                .map_err(Into::into)
        }
    }
    /// Where the output can be other than clear at `time`, when each source (by its asset
    /// key) can be within `sources(key)`. Nodes that do not say are taken to fill the frame.
    pub fn extent(
        &self,
        time: veylvik_document::Time,
        sources: &dyn Fn(&str) -> crate::nodes::Extent,
    ) -> Result<crate::nodes::Extent> {
        let index: BTreeMap<NodeId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        fn of(
            plan: &Plan,
            index: &BTreeMap<NodeId, usize>,
            id: NodeId,
            time: veylvik_document::Time,
            sources: &dyn Fn(&str) -> crate::nodes::Extent,
            depth: u32,
        ) -> Result<crate::nodes::Extent> {
            let Some(&i) = index.get(&id) else {
                return Ok(crate::nodes::Extent::Everything);
            };
            if depth > 4096 {
                return Ok(crate::nodes::Extent::Everything);
            }
            let node = &plan.nodes[i];
            if node.inputs.is_empty()
                && let Some(key) = node.operation.source_key()
            {
                // A picture the plan makes apart (other moments, a size of its own) is not
                // followed here: it may be anywhere.
                return Ok(if key.starts_with('\0') {
                    crate::nodes::Extent::Everything
                } else {
                    sources(&key)
                });
            }
            // Asked of each input only when the node asks.
            let failed = std::cell::RefCell::new(None);
            let input = |port: &str| match node.inputs.get(port) {
                None => crate::nodes::Extent::Nothing,
                Some(source) => {
                    of(plan, index, *source, time, sources, depth + 1).unwrap_or_else(|e| {
                        *failed.borrow_mut() = Some(e);
                        crate::nodes::Extent::Everything
                    })
                }
            };
            let sample = plan.sampler(i, time);
            let extent = veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::extent(n, &sample, &input))?;
            match failed.into_inner() {
                Some(e) => Err(e),
                None => Ok(extent),
            }
        }
        of(self, &index, self.root, time, sources, 0)
    }
    /// Every node's extent at `time`, by node index, as [`Plan::extent`] has it for the
    /// root; `known` answers first for a node whose extent is known otherwise (a shape's
    /// drawn box).
    pub(crate) fn extents(
        &self,
        time: veylvik_document::Time,
        sources: &dyn Fn(&str) -> crate::nodes::Extent,
        known: &dyn Fn(usize) -> Option<crate::nodes::Extent>,
    ) -> Result<Vec<crate::nodes::Extent>> {
        use crate::nodes::Extent;
        let index: BTreeMap<NodeId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let mut done: Vec<Option<Extent>> = vec![None; self.nodes.len()];
        // Inputs before the nodes that read them; a node met again while open (a loop
        // cannot be in a plan, but nothing is assumed) counts as everything.
        #[allow(clippy::too_many_arguments)]
        fn of(
            plan: &Plan,
            index: &BTreeMap<NodeId, usize>,
            i: usize,
            time: veylvik_document::Time,
            sources: &dyn Fn(&str) -> Extent,
            known: &dyn Fn(usize) -> Option<Extent>,
            done: &mut Vec<Option<Extent>>,
            depth: u32,
        ) -> Result<Extent> {
            if let Some(extent) = done[i] {
                return Ok(extent);
            }
            if depth > 4096 {
                return Ok(Extent::Everything);
            }
            let node = &plan.nodes[i];
            let extent = if let Some(extent) = known(i) {
                extent
            } else if node.inputs.is_empty()
                && let Some(key) = node.operation.source_key()
            {
                if key.starts_with('\0') {
                    Extent::Everything
                } else {
                    sources(&key)
                }
            } else {
                let mut inputs = BTreeMap::new();
                for (port, source) in &node.inputs {
                    let extent = match index.get(source) {
                        Some(&j) => of(plan, index, j, time, sources, known, done, depth + 1)?,
                        None => Extent::Everything,
                    };
                    inputs.insert(port.clone(), extent);
                }
                let input = |port: &str| inputs.get(port).copied().unwrap_or(Extent::Nothing);
                let sample = plan.sampler(i, time);
                veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::extent(n, &sample, &input))?
            };
            done[i] = Some(extent);
            Ok(extent)
        }
        let mut all = Vec::with_capacity(self.nodes.len());
        for i in 0..self.nodes.len() {
            all.push(of(self, &index, i, time, sources, known, &mut done, 0)?);
        }
        Ok(all)
    }
    /// How much of the source with asset key `key` an area of the output is made from at
    /// `time`; `None` when the output does not come from that source at all.
    pub fn reach(
        &self,
        time: veylvik_document::Time,
        key: &str,
    ) -> Result<Option<crate::nodes::Reach>> {
        let index: BTreeMap<NodeId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        fn of(
            plan: &Plan,
            index: &BTreeMap<NodeId, usize>,
            id: NodeId,
            time: veylvik_document::Time,
            key: &str,
            depth: u32,
        ) -> Result<Option<crate::nodes::Reach>> {
            let Some(&i) = index.get(&id) else {
                return Ok(None);
            };
            if depth > 4096 {
                return Ok(Some(crate::nodes::Reach::Everything));
            }
            let node = &plan.nodes[i];
            if node.inputs.is_empty() {
                let own = node.operation.source_key();
                // A picture the plan makes apart may be made of anything, this source too.
                return Ok(match own.as_deref() {
                    Some(k) if k == key => Some(crate::nodes::Reach::Same),
                    Some(k) if k.starts_with('\0') => Some(crate::nodes::Reach::Everything),
                    _ => None,
                });
            }
            let sample = plan.sampler(i, time);
            let mut found: Option<crate::nodes::Reach> = None;
            for (port, source) in &node.inputs {
                let Some(above) = of(plan, index, *source, time, key, depth + 1)? else {
                    continue;
                };
                let here = veylvik_document::each_node!(&node.operation, n => crate::nodes::RenderNode::reach(n, &sample, port))?;
                let through = here.then(above);
                found = Some(found.map_or(through, |f| f.or(through)));
            }
            Ok(found)
        }
        of(self, &index, self.root, time, key, 0)
    }
}
