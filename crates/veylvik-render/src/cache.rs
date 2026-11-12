//! Exact request keys and bounded LRU storage for completed GPU output.
use crate::{
    GpuFrame, ImageInputs, Plan, Region, RenderRequest, Result, error, params, plan::Input,
};
use std::{collections::VecDeque, sync::Arc};
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    pub bytes: u64,
}
#[derive(PartialEq, Eq)]
pub(crate) struct Key(Vec<u32>);
impl Key {
    pub fn new(plan: &Plan, request: RenderRequest, images: &ImageInputs) -> Result<Self> {
        request.time.seconds()?;
        let region = request.region.unwrap_or(Region {
            x: 0,
            y: 0,
            width: plan.width,
            height: plan.height,
        });
        region.validate(plan.width, plan.height)?;
        let mut words = vec![
            plan.width,
            plan.height,
            plan.output_slot as u32,
            region.x,
            region.y,
            region.width,
            region.height,
        ];
        for pass in &plan.passes {
            words.extend([pass.kernel as u32, pass.output as u32]);
            let values = params(
                &plan.nodes[pass.node],
                plan.width,
                plan.height,
                request.time,
            )?;
            words.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(&values)));
            for input in &pass.inputs {
                match input {
                    Input::Empty => words.extend([0, 0, 0]),
                    Input::Value(slot) => words.extend([1, *slot as u32, 0]),
                    Input::Vector(index) => {
                        use veylvik_document::vector::PathCommand::*;
                        let veylvik_document::Operation::Path { geometry } =
                            &plan.nodes[*index].operation
                        else {
                            unreachable!()
                        };
                        words.extend([
                            3,
                            geometry.fill_rule as u32,
                            geometry.commands.len() as u32,
                        ]);
                        for command in &geometry.commands {
                            match command {
                                Move { to } => {
                                    words.push(0);
                                    words.extend(to.map(f32::to_bits));
                                }
                                Line { to } => {
                                    words.push(1);
                                    words.extend(to.map(f32::to_bits));
                                }
                                Quad { control, to } => {
                                    words.push(2);
                                    words.extend(control.map(f32::to_bits));
                                    words.extend(to.map(f32::to_bits));
                                }
                                Cubic {
                                    control1,
                                    control2,
                                    to,
                                } => {
                                    words.push(3);
                                    words.extend(control1.map(f32::to_bits));
                                    words.extend(control2.map(f32::to_bits));
                                    words.extend(to.map(f32::to_bits));
                                }
                                Close {} => words.push(4),
                            }
                        }
                    }
                    Input::Asset(asset) => {
                        let image = images
                            .source(asset)
                            .ok_or_else(|| error(format!("missing image input: {asset}")))?;
                        let id = image.identity();
                        words.extend([2, id as u32, (id >> 32) as u32]);
                    }
                }
            }
        }
        Ok(Self(words))
    }
}
pub(crate) struct FrameCache<K = Key> {
    entries: VecDeque<(K, Arc<GpuFrame>)>,
    stats: CacheStats,
}
impl<K> Default for FrameCache<K> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            stats: CacheStats::default(),
        }
    }
}
impl<K: PartialEq> FrameCache<K> {
    pub fn contains(&self, key: &K) -> bool {
        self.entries.iter().any(|(candidate, _)| candidate == key)
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }
    pub fn lookup(&mut self, key: &K) -> Option<Arc<GpuFrame>> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)
        {
            let entry = self.entries.remove(index).unwrap();
            let frame = entry.1.clone();
            self.entries.push_back(entry);
            self.stats.hits += 1;
            Some(frame)
        } else {
            self.stats.misses += 1;
            None
        }
    }
    pub fn evict(&mut self) -> bool {
        let Some((_, frame)) = self.entries.pop_front() else {
            return false;
        };
        self.stats.bytes -= frame.byte_size();
        self.stats.entries -= 1;
        self.stats.evictions += 1;
        true
    }
    pub fn trim(&mut self, limit: u64) {
        while self.stats.bytes > limit {
            self.evict();
        }
    }
    pub fn insert(&mut self, key: K, frame: Arc<GpuFrame>, limit: u64) {
        if frame.byte_size() > limit {
            return;
        }
        while self.stats.bytes + frame.byte_size() > limit || self.entries.len() >= 16 {
            self.evict();
        }
        self.stats.bytes += frame.byte_size();
        self.stats.entries += 1;
        self.entries.push_back((key, frame));
    }
}
