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
    pub(crate) fn trim_vector_cache_to_budget(&self) -> Result<()> {
        let mut cache = self
            .vector_cache
            .lock()
            .map_err(|_| error("vector cache poisoned"))?;
        cache.trim(self.vector_cache_limit_bytes.min(self.budget_bytes));
        while self.resource_stats()?.held_bytes > self.budget_bytes && cache.evict() {}
        Ok(())
    }
    pub(crate) fn vector_sources(
        &self,
        plan: &crate::Plan,
        request: crate::RenderRequest,
        inputs: &crate::ImageInputs,
        download: bool,
    ) -> Result<std::collections::BTreeMap<usize, std::sync::Arc<GpuFrame>>> {
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
        for node in &plan.nodes {
            crate::params(node, plan.width, plan.height, request.time)?;
        }
        let base = (if download {
            plan.working_bytes()
        } else {
            plan.gpu_working_bytes()
        }) - plan.vector_bytes()
            + self.source_bytes(plan, inputs)?;
        let keys: Vec<_> = plan
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if let veylvik_document::Operation::Path { geometry } = &node.operation {
                    Some((
                        index,
                        Key {
                            geometry: geometry.clone(),
                            width: plan.width,
                            height: plan.height,
                        },
                    ))
                } else {
                    None
                }
            })
            .collect();
        let frame_bytes = u64::from(plan.width) * u64::from(plan.height) * 16;
        let mut cache = self
            .vector_cache
            .lock()
            .map_err(|_| error("vector cache poisoned"))?;
        cache.trim(self.vector_cache_limit_bytes.min(self.budget_bytes));
        // A cached source is already a held buffer. Charge only missing masks,
        // and evict unused retained coverage before declaring the render too big.
        loop {
            let missing = keys.iter().filter(|(_, key)| !cache.contains(key)).count() as u64;
            let needed = self.resource_stats()?.held_bytes + base + missing * frame_bytes;
            if needed <= self.budget_bytes {
                break;
            }
            if !cache.evict() {
                return Err(error(
                    "render including vector masks exceeds GPU buffer budget",
                ));
            }
        }
        let mut frames = std::collections::BTreeMap::new();
        let mut prepared: Vec<(Key, std::sync::Arc<GpuFrame>)> = Vec::new();
        for (index, key) in keys {
            if let Some((_, frame)) = prepared.iter().find(|(candidate, _)| candidate == &key) {
                frames.insert(index, frame.clone());
                continue;
            }
            let frame = if let Some(frame) = cache.lookup(&key) {
                frame
            } else {
                let frame = std::sync::Arc::new(self.render_path_mask(
                    &key.geometry,
                    key.width,
                    key.height,
                )?);
                cache.insert(
                    key.clone(),
                    frame.clone(),
                    self.vector_cache_limit_bytes.min(self.budget_bytes),
                );
                frame
            };
            prepared.push((key, frame.clone()));
            frames.insert(index, frame);
        }
        Ok(frames)
    }
}
