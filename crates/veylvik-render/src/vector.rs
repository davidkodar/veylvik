//! GPU path coverage through Vello Hybrid. Geometry preparation runs on CPU.
use crate::{GpuFrame, Renderer, Result, TextureEncoding, error};
use vello_common::{
    kurbo::BezPath,
    peniko::{Color, Fill},
};
use veylvik_document::vector::{FillRule, PathCommand, PathMask};

pub const MAX_VECTOR_DIMENSION: u32 = 4096;
pub(crate) struct Backend {
    renderer: vello_hybrid::Renderer,
    resources: vello_hybrid::Resources,
}
impl Backend {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let mut settings = vello_hybrid::RenderSettings::default();
        // Only plain filled geometry is admitted. No images, layers or filters.
        settings.memory_settings.layers_config.max_textures = Some(0);
        settings
            .memory_settings
            .image_atlas_config
            .initial_atlas_count = 0;
        settings.memory_settings.image_atlas_config.max_atlases = 1;
        settings.memory_settings.image_atlas_config.atlas_size = (16, 16);
        settings.memory_settings.image_atlas_config.auto_grow = false;
        let (renderer, resources) = vello_hybrid::Renderer::new_with(
            device,
            &vello_hybrid::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width,
                height,
            },
            settings,
        );
        Self {
            renderer,
            resources,
        }
    }
}
impl Renderer {
    /// Rasterize closed vector contours into a white premultiplied alpha mask.
    /// Feed the returned frame to an Image node, then Mask a scene-linear color
    /// or image; this keeps HDR color separate from the 8-bit coverage target.
    /// Curves are prepared on CPU, while coverage rasterization runs on Vulkan.
    /// Vello's bounded-input scratch allocations are outside `resource_stats` and
    /// the compositor buffer budget. Call `release_vector_resources` to free them.
    pub fn render_path_mask(&self, path: &PathMask, width: u32, height: u32) -> Result<GpuFrame> {
        path.validate()?;
        if width == 0
            || height == 0
            || width > MAX_VECTOR_DIMENSION
            || height > MAX_VECTOR_DIMENSION
        {
            return Err(error("vector mask dimensions must be within 1..4096"));
        }
        // Reject impossible output allocations before preparing geometry or shaders.
        let needed = u64::from(width) * u64::from(height) * 16 + 24;
        if self.resource_stats()?.held_bytes.saturating_add(needed) > self.budget_bytes {
            return Err(error("vector mask output exceeds GPU buffer budget"));
        }
        let mut geometry = BezPath::new();
        let point = |p: [f32; 2]| (f64::from(p[0]), f64::from(p[1]));
        for command in &path.commands {
            match command {
                PathCommand::Move { to } => geometry.move_to(point(*to)),
                PathCommand::Line { to } => geometry.line_to(point(*to)),
                PathCommand::Quad { control, to } => geometry.quad_to(point(*control), point(*to)),
                PathCommand::Cubic {
                    control1,
                    control2,
                    to,
                } => geometry.curve_to(point(*control1), point(*control2), point(*to)),
                PathCommand::Close {} => geometry.close_path(),
            }
        }
        let mut scene = vello_hybrid::Scene::new(width as u16, height as u16);
        scene.set_paint(Color::from_rgb8(255, 255, 255));
        scene.set_fill_rule(match path.fill_rule {
            FillRule::NonZero => Fill::NonZero,
            FillRule::EvenOdd => Fill::EvenOdd,
        });
        scene.fill_path(&geometry);
        let target = self.create_texture_target(width, height, wgpu::TextureFormat::Rgba8Unorm)?;
        let mut backend = self
            .vector_backend
            .lock()
            .map_err(|_| error("vector renderer poisoned"))?;
        // Serialize device error scopes with compositor work. Release this lock
        // before importing, which performs its own bounded buffer allocation.
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        if pool.failed {
            return Err(error("renderer unavailable after a GPU failure"));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let current = backend.get_or_insert_with(|| Backend::new(&self.device, width, height));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let result = current
            .renderer
            .render(
                &scene,
                &mut current.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &vello_hybrid::RenderSize { width, height },
                &target.texture().create_view(&Default::default()),
                &vello_hybrid::TextureBindings::new(),
            )
            .map_err(error);
        let submission = self.queue.submit([encoder.finish()]);
        let completed = self.wait(submission);
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            *backend = None;
            pool.failed = true;
            return Err(error(e));
        }
        if let Err(e) = completed {
            *backend = None;
            pool.failed = true;
            return Err(e);
        }
        result?;
        drop(pool);
        self.import_texture(&target, TextureEncoding::LinearPremultiplied)
    }

    /// Release persistent vector pipelines and scratch allocations after completed
    /// work. Existing output frames remain valid and are accounted by the pool.
    pub fn release_vector_resources(&self) -> Result<()> {
        *self
            .vector_backend
            .lock()
            .map_err(|_| error("vector renderer poisoned"))? = None;
        Ok(())
    }
}

/// The box a path can cover in a frame, `[left, top, width, height]`, a pixel wider on
/// every side for soft edges: its points and control points bound it. Never empty.
fn bounds(path: &PathMask, width: u32, height: u32) -> [u32; 4] {
    let (mut low, mut high) = ([f32::MAX; 2], [f32::MIN; 2]);
    let mut take = |point: [f32; 2]| {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    };
    for command in &path.commands {
        match command {
            PathCommand::Move { to } | PathCommand::Line { to } => take(*to),
            PathCommand::Quad { control, to } => {
                take(*control);
                take(*to);
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                take(*control1);
                take(*control2);
                take(*to);
            }
            PathCommand::Close {} => {}
        }
    }
    let frame = [width as f32, height as f32];
    let left = (low[0] - 1.0).floor().clamp(0.0, frame[0] - 1.0) as u32;
    let top = (low[1] - 1.0).floor().clamp(0.0, frame[1] - 1.0) as u32;
    let right = ((high[0] + 1.0).ceil().clamp(0.0, frame[0]) as u32).max(left + 1);
    let bottom = ((high[1] + 1.0).ceil().clamp(0.0, frame[1]) as u32).max(top + 1);
    [left, top, right - left, bottom - top]
}
/// The same path with `area`'s corner as its origin.
fn moved(path: &PathMask, area: [u32; 4]) -> PathMask {
    let by = |point: [f32; 2]| [point[0] - area[0] as f32, point[1] - area[1] as f32];
    PathMask {
        keys: Vec::new(),
        fill_rule: path.fill_rule,
        commands: path
            .commands
            .iter()
            .map(|command| match command {
                PathCommand::Move { to } => PathCommand::Move { to: by(*to) },
                PathCommand::Line { to } => PathCommand::Line { to: by(*to) },
                PathCommand::Quad { control, to } => PathCommand::Quad {
                    control: by(*control),
                    to: by(*to),
                },
                PathCommand::Cubic {
                    control1,
                    control2,
                    to,
                } => PathCommand::Cubic {
                    control1: by(*control1),
                    control2: by(*control2),
                    to: by(*to),
                },
                PathCommand::Close {} => PathCommand::Close {},
            })
            .collect(),
    }
}
/// Each drawn node's picture and where its top left corner sits in the frame.
pub(crate) type Drawn = std::collections::BTreeMap<usize, (std::sync::Arc<GpuFrame>, [u32; 2])>;
#[derive(Clone, PartialEq)]
pub(crate) struct Key {
    geometry: PathMask,
    width: u32,
    height: u32,
}
impl Renderer {
    pub fn vector_cache_stats(&self) -> Result<crate::CacheStats> {
        Ok(self
            .vector_cache
            .lock()
            .map_err(|_| error("vector cache poisoned"))?
            .stats())
    }
    /// What drawn shapes and text may keep: the set limit, or what the last frame drew
    /// when that is more, up to half the budget.
    fn vector_room(&self) -> u64 {
        self.vector_cache_limit_bytes
            .max(
                self.vector_wanted
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .min(self.budget_bytes / 2)
    }
    pub(crate) fn trim_vector_cache_to_budget(&self) -> Result<()> {
        let mut cache = self
            .vector_cache
            .lock()
            .map_err(|_| error("vector cache poisoned"))?;
        cache.trim(self.vector_room());
        while self.resource_stats()?.held_bytes > self.budget_bytes && cache.evict() {}
        Ok(())
    }
    pub(crate) fn vector_sources(
        &self,
        plan: &crate::Plan,
        request: crate::RenderRequest,
        inputs: &crate::ImageInputs,
        download: bool,
    ) -> Result<Drawn> {
        {
            let mut pool = self
                .pool
                .lock()
                .map_err(|_| error("renderer resource pool poisoned"))?;
            let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
            pool.trim(self.pool_limit_bytes.min(available));
            if pool.failed {
                return Err(error("renderer unavailable after a GPU failure"));
            }
        }
        request.time.seconds()?;
        if let Some(region) = request.region {
            region.validate(plan.width, plan.height)?;
        }
        for (node, curves) in plan.nodes.iter().zip(&plan.curves) {
            crate::params(
                node,
                &plan.nodes,
                curves,
                plan.width,
                plan.height,
                request.time,
            )?;
        }
        let base = (if download {
            plan.working_bytes()
        } else {
            plan.gpu_working_bytes()
        }) - plan.vector_bytes()
            + self.source_bytes(plan, inputs)?;
        let mut keys = Vec::new();
        // A text or shape that cannot reach the picture at this time is not drawn: its
        // passes are skipped too (see `encode_and_read`).
        let visible = plan.visible_at(request.time)?;
        for (index, node) in plan.nodes.iter().enumerate() {
            if !visible.contains(&node.id) {
                continue;
            }
            let geometry = match &node.operation {
                veylvik_document::Operation::Path(veylvik_document::nodes::Path { geometry }) => {
                    geometry.at(request.time)?
                }
                veylvik_document::Operation::Text(veylvik_document::nodes::Text { .. }) => {
                    veylvik_text::outline(&node.operation, request.time).map_err(error)?
                }
                _ => continue,
            };
            keys.push((
                index,
                Key {
                    geometry,
                    width: plan.width,
                    height: plan.height,
                },
            ));
        }
        let frame_bytes = u64::from(plan.width) * u64::from(plan.height) * 16;
        let mut cache = self
            .vector_cache
            .lock()
            .map_err(|_| error("vector cache poisoned"))?;
        // Room for everything this one frame draws, up to half the budget: with room for
        // all but one, each shape drawn pushes out the one needed next, and all of them
        // are drawn again for every frame. It still gives way to rendering below.
        self.vector_wanted.store(
            (keys.len() as u64).saturating_mul(frame_bytes),
            std::sync::atomic::Ordering::Relaxed,
        );
        let limit = self.vector_room();
        cache.trim(limit);
        // A cached source is already a held buffer. Charge only missing masks,
        // and evict unused retained coverage before declaring the render too big.
        loop {
            let missing = keys.iter().filter(|(_, key)| !cache.contains(key)).count() as u64;
            let needed = self.resource_stats()?.held_bytes + base + missing * frame_bytes;
            if needed <= self.budget_bytes {
                break;
            }
            if !cache.evict() {
                let mib = |bytes: u64| bytes / (1024 * 1024);
                return Err(error(format!(
                    "this render needs {} MiB of GPU buffers and the budget is {} MiB ({} MiB are held by other work): lower the resolution, or raise the GPU budget in Preferences or with VEYLVIK_GPU_BUDGET_MIB",
                    mib(needed),
                    mib(self.budget_bytes),
                    mib(needed.saturating_sub(base + missing * frame_bytes))
                )));
            }
        }
        let mut frames = std::collections::BTreeMap::new();
        let mut prepared: Vec<(Key, std::sync::Arc<GpuFrame>)> = Vec::new();
        for (index, key) in keys {
            // A shape is drawn and kept over its own box only: a logo's part covers a
            // thousandth of a 4K frame. Text is kept whole (its pass reads it so).
            let area = match &plan.nodes[index].operation {
                veylvik_document::Operation::Path(_) => {
                    bounds(&key.geometry, key.width, key.height)
                }
                _ => [0, 0, key.width, key.height],
            };
            if let Some((_, frame)) = prepared.iter().find(|(candidate, _)| candidate == &key) {
                frames.insert(index, (frame.clone(), [area[0], area[1]]));
                continue;
            }
            let frame = if let Some(frame) = cache.lookup(&key) {
                frame
            } else {
                let frame = std::sync::Arc::new(if area == [0, 0, key.width, key.height] {
                    self.render_path_mask(&key.geometry, key.width, key.height)?
                } else {
                    self.render_path_mask(&moved(&key.geometry, area), area[2], area[3])?
                });
                cache.insert(key.clone(), frame.clone(), limit);
                frame
            };
            prepared.push((key, frame.clone()));
            frames.insert(index, (frame, [area[0], area[1]]));
        }
        Ok(frames)
    }
}
