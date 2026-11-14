//! Explicit CPU/GPU image and render-request contracts. No filesystem access.
use crate::{Result, error};
use std::{collections::BTreeMap, sync::Arc};
use veylvik_document::Time;

pub(crate) fn next_identity() -> Result<u64> {
    static NEXT_RESOURCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let ordering = std::sync::atomic::Ordering::Relaxed;
    let mut identity = NEXT_RESOURCE.load(ordering);
    loop {
        let next = identity
            .checked_add(1)
            .ok_or_else(|| error("image identity exhausted"))?;
        match NEXT_RESOURCE.compare_exchange_weak(identity, next, ordering, ordering) {
            Ok(_) => break,
            Err(current) => identity = current,
        }
    }
    Ok(identity)
}

/// Scene-linear Rec.709 premultiplied pixels. RGB may be negative or exceed one.
#[derive(Debug)]
pub struct Image {
    identity: u64,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}
impl Image {
    pub fn new(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> Result<Self> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || pixels.len() as u64 != width as u64 * height as u64
        {
            return Err(error("invalid image dimensions or pixel count"));
        }
        if pixels.iter().any(|p| {
            p.iter().any(|v| !v.is_finite())
                || !(0.0..=1.0).contains(&p[3])
                || (p[3] == 0.0 && p[..3].iter().any(|v| *v != 0.0))
        }) {
            return Err(error(
                "image pixels must be finite and premultiplied, with alpha in [0,1] and zero RGB at zero alpha",
            ));
        }
        let identity = next_identity()?;
        Ok(Self {
            identity,
            width,
            height,
            pixels,
        })
    }
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[[f32; 4]] {
        &self.pixels
    }
    pub fn byte_size(&self) -> u64 {
        self.pixels.len() as u64 * 16
    }
}
/// Time-aware host source. Decoding runs on the render worker, never the UI thread.
/// The host must bound its cache and return immutable frames with stable identities.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub width: u32,
    pub height: u32,
    pub duration: Option<Time>,
    pub frame_rate: Option<Time>,
    pub frames: Option<u32>,
    pub description: String,
}
pub trait FrameSource: Send + Sync {
    fn info(&self) -> Option<SourceInfo> {
        None
    }
    fn frame(&self, time: Time) -> Result<Arc<Image>>;
    fn gpu_frame(&self, _time: Time, _renderer: &crate::Renderer) -> Result<Option<Arc<GpuFrame>>> {
        Ok(None)
    }
    /// A frame no larger than `target` (rotated width and height), when the source can reduce
    /// during conversion; otherwise the full-size frame. Placement scales either correctly.
    fn gpu_frame_sized(
        &self,
        time: Time,
        renderer: &crate::Renderer,
        _target: Option<[u32; 2]>,
    ) -> Result<Option<Arc<GpuFrame>>> {
        self.gpu_frame(time, renderer)
    }
    /// As [`FrameSource::gpu_frame_sized`] with the Footage node's input colour; only sources
    /// that can reinterpret their code values (video) honour a non-automatic choice.
    fn gpu_frame_with(
        &self,
        time: Time,
        renderer: &crate::Renderer,
        target: Option<[u32; 2]>,
        colour: veylvik_document::media::InputColour,
    ) -> Result<Option<Arc<GpuFrame>>> {
        let _ = colour;
        self.gpu_frame_sized(time, renderer, target)
    }
    /// As [`FrameSource::frame`] with the Footage node's input colour.
    fn frame_with(
        &self,
        time: Time,
        colour: veylvik_document::media::InputColour,
    ) -> Result<Arc<Image>> {
        let _ = colour;
        self.frame(time)
    }
    /// Files whose changes invalidate a playback frame. Unknown/custom sources opt out.
    fn cache_dependencies(&self) -> Option<Vec<std::path::PathBuf>> {
        None
    }
    fn byte_budget(&self) -> u64;
}
/// The host supplies immutable images; a later render can supply new content under the same key.
#[derive(Default, Clone)]
pub struct ImageInputs(
    BTreeMap<String, Arc<Image>>,
    BTreeMap<String, Arc<GpuFrame>>,
    BTreeMap<String, Arc<dyn FrameSource>>,
    std::path::PathBuf,
);
impl ImageInputs {
    pub(crate) fn same_sources(&self, other: &Self) -> bool {
        self.3 == other.3
            && self.0.len() == other.0.len()
            && self.1.len() == other.1.len()
            && self.2.len() == other.2.len()
            && self
                .0
                .iter()
                .all(|(k, v)| other.0.get(k).is_some_and(|w| Arc::ptr_eq(v, w)))
            && self
                .1
                .iter()
                .all(|(k, v)| other.1.get(k).is_some_and(|w| Arc::ptr_eq(v, w)))
            && self
                .2
                .iter()
                .all(|(k, v)| other.2.get(k).is_some_and(|w| Arc::ptr_eq(v, w)))
    }
    pub(crate) fn playback_dependencies(&self) -> Option<Vec<std::path::PathBuf>> {
        let mut paths = Vec::new();
        for source in self.2.values() {
            paths.extend(source.cache_dependencies()?);
        }
        Some(paths)
    }

    pub fn set_base_directory(&mut self, base: impl Into<std::path::PathBuf>) {
        self.3 = base.into();
    }
    pub fn resolve_config(&self, config: &str) -> String {
        if config.starts_with("ocio://") {
            config.to_owned()
        } else {
            self.3.join(config).to_string_lossy().into_owned()
        }
    }
    /// Declared CPU pixel bytes, counting shared images once.
    pub fn byte_size(&self) -> u64 {
        let mut seen = std::collections::BTreeSet::new();
        self.0
            .values()
            .filter(|image| seen.insert(image.identity()))
            .map(|image| image.byte_size())
            .sum::<u64>()
            + self
                .2
                .values()
                .map(|source| source.byte_budget())
                .sum::<u64>()
    }
    pub fn insert(&mut self, key: impl Into<String>, image: Arc<Image>) {
        let key = key.into();
        self.1.remove(&key);
        self.2.remove(&key);
        self.0.insert(key, image);
    }
    /// Retain a completed frame from the same renderer as a source. Its crop is
    /// presented at the image node's top-left, without packing or pixel readback.
    /// The renderer checks ownership before using it; held allocations remain budgeted.
    pub fn insert_gpu(&mut self, key: impl Into<String>, frame: Arc<GpuFrame>) {
        let key = key.into();
        self.0.remove(&key);
        self.2.remove(&key);
        self.1.insert(key, frame);
    }
    pub fn insert_source(&mut self, key: impl Into<String>, source: Arc<dyn FrameSource>) {
        let key = key.into();
        self.0.remove(&key);
        self.1.remove(&key);
        self.2.insert(key, source);
    }
    pub fn info(&self, key: &str) -> Option<SourceInfo> {
        self.2.get(key).and_then(|s| s.info()).or_else(|| {
            self.get(key).map(|i| SourceInfo {
                width: i.width(),
                height: i.height(),
                duration: None,
                frame_rate: None,
                frames: None,
                description: "Still image".into(),
            })
        })
    }
    pub fn source_budget(&self, key: &str) -> u64 {
        self.2.get(key).map_or_else(
            || self.get(key).map_or(0, |i| i.byte_size()),
            |s| s.byte_budget(),
        )
    }
    pub fn resolve(&self, plan: &crate::Plan, time: Time) -> Result<Self> {
        self.resolve_on(plan, time, None)
    }
    pub(crate) fn resolve_on(
        &self,
        plan: &crate::Plan,
        time: Time,
        renderer: Option<&crate::Renderer>,
    ) -> Result<Self> {
        let mut result = self.clone();
        let mut decoded_bytes = 0u64;
        // Hidden branches (behind a fully transparent blend) get an empty stand-in, not a decode.
        let visible = plan.visible_at(time)?;
        for node in &plan.nodes {
            if let veylvik_document::Operation::Footage(veylvik_document::nodes::Footage {
                asset,
                timing,
                placement,
                colour,
            }) = &node.operation
            {
                let key = crate::plan::footage_key(node.id);
                if self.contains_key(&key) {
                    continue;
                }
                let info = self.info(asset);
                let target = info.as_ref().and_then(|i| {
                    placed_size(*placement, [i.width, i.height], [plan.width, plan.height])
                });
                let time = timing.source_time(time, info.and_then(|i| i.duration))?;
                let time = time.filter(|_| visible.contains(&node.id));
                let image = if let Some(time) = time {
                    if let Some(source) = self.2.get(asset) {
                        if let Some(renderer) = renderer
                            && let Some(frame) =
                                source.gpu_frame_with(time, renderer, target, *colour)?
                        {
                            decoded_bytes += frame.byte_size();
                            if decoded_bytes > 256 * 1024 * 1024 {
                                return Err(error(
                                    "Decoded footage sources exceed 256 MiB per frame",
                                ));
                            }
                            result.insert_gpu(key, frame);
                            continue;
                        }
                        source.frame_with(time, *colour)?
                    } else {
                        self.get(asset)
                            .cloned()
                            .ok_or_else(|| error(format!("missing footage: {asset}")))?
                    }
                } else {
                    static EMPTY: std::sync::OnceLock<Arc<Image>> = std::sync::OnceLock::new();
                    EMPTY
                        .get_or_init(|| {
                            Arc::new(Image::new(1, 1, vec![[0.0; 4]]).expect("valid empty image"))
                        })
                        .clone()
                };
                decoded_bytes += image.byte_size();
                if decoded_bytes > 256 * 1024 * 1024 {
                    return Err(error("Decoded footage sources exceed 256 MiB per frame"));
                }
                result.insert(key, image);
            } else if let veylvik_document::Operation::Image(veylvik_document::nodes::Image {
                asset,
            }) = &node.operation
                && let Some(source) = self.2.get(asset)
                && visible.contains(&node.id)
            {
                if let Some(renderer) = renderer
                    && let Some(frame) = source.gpu_frame(time, renderer)?
                {
                    decoded_bytes += frame.byte_size();
                    if decoded_bytes > 256 * 1024 * 1024 {
                        return Err(error("Decoded footage sources exceed 256 MiB per frame"));
                    }
                    result.insert_gpu(asset, frame);
                    continue;
                }
                let image = source.frame(time)?;
                decoded_bytes += image.byte_size();
                if decoded_bytes > 256 * 1024 * 1024 {
                    return Err(error("Decoded footage sources exceed 256 MiB per frame"));
                }
                result.insert(asset, image);
            }
        }
        Ok(result)
    }
    pub fn get_gpu(&self, key: &str) -> Option<&Arc<GpuFrame>> {
        self.1.get(key)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key) || self.1.contains_key(key) || self.2.contains_key(key)
    }
    pub(crate) fn source(&self, key: &str) -> Option<Source<'_>> {
        self.0
            .get(key)
            .map(Source::Cpu)
            .or_else(|| self.1.get(key).map(Source::Gpu))
    }
    pub fn get(&self, key: &str) -> Option<&Arc<Image>> {
        self.0.get(key)
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Source<'a> {
    Cpu(&'a Arc<Image>),
    Gpu(&'a Arc<GpuFrame>),
}
impl Source<'_> {
    pub fn identity(self) -> u64 {
        match self {
            Self::Cpu(image) => image.identity(),
            Self::Gpu(frame) => frame.identity,
        }
    }
    pub fn byte_size(self) -> u64 {
        match self {
            Self::Cpu(image) => image.byte_size(),
            Self::Gpu(frame) => frame.byte_size(),
        }
    }
    pub fn storage_width(self) -> u32 {
        match self {
            Self::Cpu(image) => image.width(),
            Self::Gpu(frame) => frame.width,
        }
    }
    pub fn region(self) -> Region {
        match self {
            Self::Cpu(image) => Region {
                x: 0,
                y: 0,
                width: image.width(),
                height: image.height(),
            },
            Self::Gpu(frame) => frame.region,
        }
    }
}
/// A crop in composition pixels. The initial executor evaluates the whole graph before cropping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Region {
    pub fn validate(self, width: u32, height: u32) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|end| end > width)
            || self
                .y
                .checked_add(self.height)
                .is_none_or(|end| end > height)
        {
            return Err(error(
                "render region must be nonempty and inside the composition",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct RenderRequest {
    pub time: Time,
    pub region: Option<Region>,
}
impl RenderRequest {
    pub fn full(time: Time) -> Self {
        Self { time, region: None }
    }
}
pub struct RenderedFrame {
    pub region: Region,
    pub pixels: Vec<[f32; 4]>,
}

/// Completed, immutable scene-linear RGBA32F output on the renderer's device.
/// Retain this handle until all GPU consumers have completed their submissions.
/// Dropping it releases its allocation; it is never recycled as an intermediate.
/// A crop describes a view into the full-size buffer, not a packed allocation.
#[derive(Clone)]
pub struct GpuFrame {
    pub(crate) flow_report: Option<crate::flow::FlowReport>,
    pub(crate) identity: u64,
    pub(crate) storage: Arc<crate::pool::PinnedBuffer>,
    pub(crate) owner: Arc<DeviceIdentity>,
    pub(crate) region: Region,
    pub(crate) width: u32,
    pub(crate) height: u32,
}
pub(crate) struct DeviceIdentity;
impl GpuFrame {
    pub fn flow_report(&self) -> Option<crate::flow::FlowReport> {
        self.flow_report
    }
    pub fn region(&self) -> Region {
        self.region
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Read-only storage buffer of row-major premultiplied `[f32; 4]` pixels.
    /// Use only with this renderer's device. Do not destroy or modify the buffer.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.storage.buffer
    }
    pub fn byte_size(&self) -> u64 {
        self.width as u64 * self.height as u64 * 16
    }
}

/// Size to decode footage at for its placement on a `canvas`: never larger than the source,
/// and `None` when the placement needs native pixels or no reduction is possible.
pub(crate) fn placed_size(
    placement: veylvik_document::media::Placement,
    source: [u32; 2],
    canvas: [u32; 2],
) -> Option<[u32; 2]> {
    use veylvik_document::media::Placement;
    let [sw, sh] = source.map(f64::from);
    let [cw, ch] = canvas.map(f64::from);
    let (sx, sy) = match placement {
        Placement::Fit => {
            let s = (cw / sw).min(ch / sh);
            (s, s)
        }
        Placement::Fill => {
            let s = (cw / sw).max(ch / sh);
            (s, s)
        }
        Placement::Stretch => (cw / sw, ch / sh),
        Placement::Original | Placement::Legacy => return None,
    };
    let size = [
        (sw * sx.min(1.)).ceil() as u32,
        (sh * sy.min(1.)).ceil() as u32,
    ];
    (size != source && size.iter().all(|&v| v > 0)).then_some(size)
}

#[cfg(test)]
mod placement_tests {
    use super::placed_size;
    use veylvik_document::media::Placement;
    #[test]
    fn footage_decodes_at_the_size_its_placement_needs() {
        let phone = [2160, 3840];
        assert_eq!(
            placed_size(Placement::Fill, phone, [1080, 1920]),
            Some([1080, 1920])
        );
        // Fill covers the canvas, so a wider canvas keeps more source height.
        assert_eq!(
            placed_size(Placement::Fill, phone, [1920, 1080]),
            Some([1920, 3414])
        );
        assert_eq!(
            placed_size(Placement::Fit, phone, [1920, 1080]),
            Some([608, 1080])
        );
        assert_eq!(
            placed_size(Placement::Stretch, phone, [1920, 1080]),
            Some([1920, 1080])
        );
        // Native pixels are kept, and nothing is ever decoded larger than the source.
        assert_eq!(placed_size(Placement::Original, phone, [1080, 1920]), None);
        assert_eq!(placed_size(Placement::Legacy, phone, [1080, 1920]), None);
        assert_eq!(
            placed_size(Placement::Fill, [1280, 720], [3840, 2160]),
            None
        );
    }
}
