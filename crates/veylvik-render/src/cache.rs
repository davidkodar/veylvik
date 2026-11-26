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
/// The words a picture is made of, and whether every input among them is known by a
/// name of its maker's: then the words mean the same picture in any session.
pub(crate) struct Key(Vec<u32>, bool);
impl Key {
    pub fn words(&self) -> &[u32] {
        &self.0
    }
    pub fn portable(&self) -> bool {
        self.1
    }
    pub fn new(plan: &Plan, request: RenderRequest, images: &ImageInputs) -> Result<Self> {
        Self::with(plan, request, images, &Default::default())
    }
    /// As [`Key::new`], with `names` standing for the pictures the plan makes apart
    /// (its branches of other moments), by their keys: what each is made of, and
    /// whether that holds in another session.
    pub fn with(
        plan: &Plan,
        request: RenderRequest,
        images: &ImageInputs,
        names: &std::collections::BTreeMap<String, (u64, bool)>,
    ) -> Result<Self> {
        request.time.seconds()?;
        let region = request.region.unwrap_or(Region {
            x: 0,
            y: 0,
            width: plan.width,
            height: plan.height,
        });
        region.validate(plan.width, plan.height)?;
        let mut portable = true;
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
            pass_words(
                plan,
                pass,
                request,
                images,
                names,
                &mut words,
                &mut portable,
                &mut |slot, words| words.push(slot as u32),
            )?;
        }
        Ok(Self(words, portable))
    }
}
/// The words that say what one pass makes: its kernel and output, its node's settings
/// at the time, and each input. `value` writes what stands for a value input read from
/// a slot (the slot itself in a whole frame's key, the name of what wrote it in a pass's
/// own identity).
#[allow(clippy::too_many_arguments)]
fn pass_words(
    plan: &Plan,
    pass: &crate::plan::Pass,
    request: RenderRequest,
    images: &ImageInputs,
    names: &std::collections::BTreeMap<String, (u64, bool)>,
    words: &mut Vec<u32>,
    portable: &mut bool,
    value: &mut dyn FnMut(usize, &mut Vec<u32>),
) -> Result<()> {
    words.extend([pass.kernel as u32, pass.output as u32]);
    let values = params(
        &plan.nodes[pass.node],
        &plan.nodes,
        &plan.curves[pass.node],
        plan.width,
        plan.height,
        request.time,
    )?;
    words.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(&values)));
    if let veylvik_document::Operation::Shader(shader) = &plan.nodes[pass.node].operation {
        // A custom shader's own text is part of what makes its picture.
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        shader.code.hash(&mut hasher);
        let code = hasher.finish();
        words.extend([5, code as u32, (code >> 32) as u32]);
    }
    // What a special pass's picture is made of besides its node's settings.
    crate::special::words(plan, pass, request, words, value)?;
    if let Some(lighting) =
        crate::shadow_lighting::for_node(&plan.nodes[pass.node], plan, request.time)?
    {
        words.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(&lighting)));
    }
    for input in &pass.inputs {
        match input {
            Input::Empty => words.extend([0, 0, 0]),
            Input::Value(slot) => {
                words.push(1);
                value(*slot, words);
                words.push(0);
            }
            Input::Vector(index) => {
                crate::layers::identity_words(&plan.nodes[*index].operation, request.time, words)?
            }
            Input::Asset(asset) if names.contains_key(asset) => {
                let (id, lasting) = names[asset];
                *portable &= lasting;
                words.extend([5, id as u32, (id >> 32) as u32]);
            }
            Input::Asset(asset) => {
                let image = images
                    .source(asset)
                    .ok_or_else(|| error(format!("missing image input: {asset}")))?;
                let id = image.identity();
                *portable &= crate::frame::is_named(id);
                words.extend([2, id as u32, (id >> 32) as u32]);
            }
        }
    }
    Ok(())
}
/// Each pass's own identity: what it makes, named through the passes that feed it,
/// whatever slots they used, so a pass's result can be known again in another frame
/// when nothing it is made of has changed. `None` for a pass that reads a slot nothing
/// before it wrote.
pub(crate) fn pass_identities(
    plan: &Plan,
    request: RenderRequest,
    images: &ImageInputs,
) -> Result<Vec<Option<u64>>> {
    let names = Default::default();
    let mut writer: Vec<Option<usize>> = vec![None; plan.slot_count];
    let mut ids: Vec<Option<u64>> = Vec::with_capacity(plan.passes.len());
    let mut portable = true;
    for (at, pass) in plan.passes.iter().enumerate() {
        let mut words = vec![plan.width, plan.height];
        let mut known = true;
        pass_words(
            plan,
            pass,
            request,
            images,
            &names,
            &mut words,
            &mut portable,
            &mut |slot, words| match writer.get(slot).copied().flatten().and_then(|q| ids[q]) {
                Some(id) => words.extend([id as u32, (id >> 32) as u32]),
                None => known = false,
            },
        )?;
        ids.push(known.then(|| crate::fnv(bytemuck::cast_slice(&words), 0x5eed)));
        writer[pass.output] = Some(at);
    }
    Ok(ids)
}
/// Entries a cache holds at most, whatever their size: enough for any one frame's parts.
const ENTRIES: usize = 1024;
pub(crate) struct FrameCache<K = Key> {
    entries: VecDeque<(K, Arc<GpuFrame>)>,
    stats: CacheStats,
    /// Pictures asked for lately and not kept (digests of their keys): what may be kept
    /// once asked for again.
    asked: VecDeque<u64>,
}
impl<K> Default for FrameCache<K> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            stats: CacheStats::default(),
            asked: VecDeque::new(),
        }
    }
}
/// How many unkept pictures the cache remembers having been asked for.
const ASKED: usize = 256;
impl FrameCache<Key> {
    /// Whether the picture of `key` was asked for lately: a picture asked for once is
    /// remembered, and true is said the second time. A picture made apart for one moment
    /// (a sun's mask at this frame) is never asked for again, and keeping each one only
    /// to push it out later left the device's allocator holding gigabytes more than was
    /// in use during an export; one asked for again (a still sky) is kept from then on.
    pub fn asked_again(&mut self, key: &Key) -> bool {
        let words: Vec<u8> = key.0.iter().flat_map(|w| w.to_le_bytes()).collect();
        let digest = crate::fnv(&words, u64::from(key.1));
        if let Some(at) = self.asked.iter().position(|d| *d == digest) {
            self.asked.remove(at);
            return true;
        }
        if self.asked.len() >= ASKED {
            self.asked.pop_front();
        }
        self.asked.push_back(digest);
        false
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
        // The byte limit is what bounds it. A count of sixteen once did, and a logo of
        // seventeen shapes then pushed each one out just before it was needed again.
        while self.stats.bytes + frame.byte_size() > limit || self.entries.len() >= ENTRIES {
            self.evict();
        }
        self.stats.bytes += frame.byte_size();
        self.stats.entries += 1;
        self.entries.push_back((key, frame));
    }
}
#[cfg(test)]
mod asked_tests {
    use super::*;
    #[test]
    fn a_picture_is_kept_only_when_it_is_asked_for_again() {
        let mut cache = FrameCache::<Key>::default();
        let key = |n: u32| Key(vec![1, 2, n], true);
        // Asked once: not yet; asked again: yes, and then forgotten (it is kept now).
        assert!(!cache.asked_again(&key(7)));
        assert!(cache.asked_again(&key(7)));
        assert!(!cache.asked_again(&key(7)));
        // A picture of every frame, each asked for once, is never said to be asked again,
        // and the memory of them stays bounded.
        for n in 100..100 + 3 * ASKED as u32 {
            assert!(!cache.asked_again(&key(n)));
        }
        assert_eq!(cache.asked.len(), ASKED);
        // The same words in another session's sense are another picture.
        assert!(!cache.asked_again(&Key(vec![1, 2, 9], false)));
        assert!(!cache.asked_again(&Key(vec![1, 2, 9], true)));
    }
}
