//! GPU planar feature detection and batched perspective-aware patch matching.
use crate::estimate::{Consensus, Motion, estimate};
use crate::{Result, error};
use std::sync::Arc;
use veylvik_document::planar::{self, Feature, Homography, Point, Quad};
use veylvik_render::{GpuFrame, Renderer, compute::DeviceKey};

pub struct PlanarTracker {
    detect: wgpu::ComputePipeline,
    correlate: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    aligner: crate::refine::AreaAligner,
    owner: DeviceKey,
}
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub patch_radius: u32,
    pub search_radius: u32,
    pub tolerance_pixels: f64,
    /// Re-detect features on the latest accepted frame when fewer than this many matches
    /// are accepted, so scale and viewpoint changes do not exhaust a fixed reference.
    /// Zero keeps the original reference for the whole range.
    pub reanchor_below: u32,
    /// Keep the analysis area fixed on screen instead of moving it with the surface:
    /// for floors, roads and other planes that keep filling the same part of the frame
    /// while the camera moves, each re-anchor detects fresh texture there.
    pub screen_area: bool,
    /// Transformations the surface may undergo; see [`Motion`].
    pub motion: Motion,
    /// After feature matching, align every pixel of the area directly (perspective and affine
    /// models). Sub-pixel stability on blurred or low-texture surfaces.
    pub refine: bool,
    /// Detection grid cells per side (8–16): one feature per cell, so up to 64–256 features.
    pub grid: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            patch_radius: 6,
            search_radius: 16,
            tolerance_pixels: 2.,
            reanchor_below: 24,
            screen_area: false,
            motion: Motion::Auto,
            refine: true,
            grid: 12,
        }
    }
}
/// Why a range analysis ended before its last frame, if it did.
#[derive(Debug, Clone, PartialEq)]
pub enum Stop {
    Complete,
    Cancelled,
    Lost { frame: i64, reason: String },
    SizeLimit { frame: i64 },
}
impl Stop {
    pub fn message(&self) -> String {
        match self {
            Self::Complete => "Tracking complete".into(),
            Self::Cancelled => "Cancelled; accepted frames are available to save".into(),
            Self::Lost { frame, reason } => format!("Stopped at frame {frame}: {reason}"),
            Self::SizeLimit { .. } => {
                "Stopped before the project size limit; accepted frames remain available".into()
            }
        }
    }
}
pub struct Range {
    pub data: planar::Track,
    /// Why the forward (or only) direction ended.
    pub stop: Stop,
    /// Why the backward direction ended, when tracking both ways from a middle reference.
    pub backward: Option<Stop>,
    /// References used, including the first; more than one means the tracker re-anchored.
    pub anchors: u32,
}
/// Called after every accepted frame with its index and pixels; return false to cancel.
pub type Observer<'a> = dyn FnMut(i64, &Arc<GpuFrame>, &planar::Sample) -> bool + 'a;
pub struct TrackedPlane {
    pub consensus: Consensus,
    /// Includes rejected matches, for diagnostic viewer overlays.
    pub features: Vec<Feature>,
}
impl PlanarTracker {
    /// Track `surface` from `start` (its reference frame) towards `end`, in either direction,
    /// on the rendered output of `plan`. `occlusion` renders a matte per frame whose alpha
    /// excludes foreground objects. Accepted frames are kept when the track is lost or
    /// cancelled; `max_bytes` bounds the encoded diagnostics so the result stays savable.
    #[allow(clippy::too_many_arguments)]
    pub fn track_range(
        &self,
        renderer: &Renderer,
        plan: &veylvik_render::Plan,
        occlusion: Option<(&veylvik_render::Plan, &veylvik_render::ImageInputs)>,
        inputs: &veylvik_render::ImageInputs,
        surface: Quad,
        region: Option<Quad>,
        [start, end]: [i64; 2],
        rate: veylvik_document::Time,
        settings: Settings,
        max_bytes: usize,
        observe: &mut Observer<'_>,
    ) -> Result<Range> {
        let at = |frame: i64| {
            veylvik_document::Time::at_frame(frame, rate).map_err(|e| error(e.to_string()))
        };
        let render_with =
            |plan: &veylvik_render::Plan, inputs: &veylvik_render::ImageInputs, frame: i64| {
                Ok::<_, crate::Error>(renderer.render_gpu_cached(
                    plan,
                    veylvik_render::RenderRequest::full(at(frame)?),
                    inputs,
                )?)
            };
        let render = |plan: &veylvik_render::Plan, frame: i64| render_with(plan, inputs, frame);
        let matte = |frame: i64| {
            occlusion
                .map(|(plan, inputs)| render_with(plan, inputs, frame))
                .transpose()
        };
        if let Some(region) = region {
            planar::validate_quad(region).map_err(|e| error(e.to_string()))?;
        }
        let visible =
            |quad: Quad| quad.map(|[x, y]| [x.clamp(0.005, 0.995), y.clamp(0.005, 0.995)]);
        // Everything needed to match against one reference frame.
        struct Anchor {
            image: Arc<GpuFrame>,
            levels: Vec<GpuFrame>,
            points: Vec<Point>,
            /// Corners and analyzed area as seen on this reference.
            /// Motion from the original reference frame to this anchor's frame.
            from_reference: Homography,
            area: Quad,
        }
        // A candidate anchor on an accepted frame: image, levels, matte, motion from the
        // reference, area.
        type Candidate = Box<(
            Arc<GpuFrame>,
            Vec<GpuFrame>,
            Option<Arc<GpuFrame>>,
            Homography,
            Quad,
        )>;
        // Returns the candidate unchanged when too little texture is visible there.
        let reanchor =
            |candidate: Candidate, minimum: usize| -> std::result::Result<Anchor, Candidate> {
                let detect_in = visible(if settings.screen_area {
                    region.unwrap_or(surface)
                } else {
                    candidate.4
                });
                let points = planar::validate_quad(detect_in).ok().and_then(|_| {
                    self.detect_masked(
                        renderer,
                        &candidate.0,
                        detect_in,
                        settings,
                        candidate.2.as_deref(),
                    )
                    .ok()
                });
                match points {
                    Some(points) if points.len() >= minimum.max(12) => {
                        let (image, levels, _, from_reference, _) = *candidate;
                        Ok(Anchor {
                            image,
                            levels,
                            points,
                            from_reference,
                            area: detect_in,
                        })
                    }
                    _ => Err(candidate),
                }
            };
        let reference_time = at(start)?;
        let image = render(plan, start)?;
        let first_matte = matte(start)?;
        let area = region.unwrap_or(surface);
        let points =
            self.detect_masked(renderer, &image, area, settings, first_matte.as_deref())?;
        let mut anchor = Anchor {
            levels: self.pyramid(renderer, &image)?,
            image,
            points,
            from_reference: Homography::IDENTITY,
            area,
        };
        let initial = self.track_masked(
            renderer,
            &anchor.image,
            &anchor.image,
            &anchor.points,
            Homography::IDENTITY,
            settings,
            first_matte.as_deref(),
        )?;
        let first = planar::Sample {
            time: reference_time,
            corners: surface,
            features: initial.features,
            rms_pixels: initial.consensus.rms_pixels,
            motion: None,
        };
        let mut data = planar::Track {
            reference_time,
            reference: surface,
            region,
            samples: vec![first],
        };
        // Allow for the document's deeper pretty-JSON indentation.
        let size = |value: &dyn erased::Encode| value.encoded_len() * 2;
        let mut bytes = size(&data);
        if bytes > max_bytes {
            return Err(error("Not enough project space for tracking diagnostics"));
        }
        let mut stop = Stop::Complete;
        if !observe(start, &anchor.image, &data.samples[0]) {
            stop = Stop::Cancelled;
        }
        let (mut motion, mut anchors) = (Homography::IDENTITY, 1);
        // The last accepted frame, kept so a failed frame can retry from a fresh anchor there.
        let mut previous: Option<Candidate> = None;
        let step = if end > start { 1 } else { -1 };
        let mut frame = start;
        while stop == Stop::Complete && frame != end {
            frame += step;
            let current = render(plan, frame)?;
            let current_matte = matte(frame)?;
            let current_levels = self.pyramid(renderer, &current)?;
            let solve = |anchor: &Anchor, motion: Homography| {
                self.track_multiscale_masked(
                    renderer,
                    &anchor.image,
                    &current,
                    &anchor.levels,
                    &current_levels,
                    &anchor.points,
                    motion,
                    settings,
                    current_matte.as_deref(),
                )
            };
            let mut attempt = solve(&anchor, motion);
            if attempt.is_err()
                && settings.reanchor_below > 0
                && let Some(Ok(fresh)) = previous.take().map(|candidate| reanchor(candidate, 0))
            {
                anchor = fresh;
                motion = Homography::IDENTITY;
                anchors += 1;
                attempt = solve(&anchor, motion);
            }
            let lost = |e: &dyn std::fmt::Display| Stop::Lost {
                frame,
                reason: e.to_string(),
            };
            let mut found = match attempt {
                Ok(found) => found,
                Err(e) => {
                    stop = lost(&e);
                    break;
                }
            };
            if settings.refine
                && matches!(found.consensus.model, Motion::Perspective | Motion::Affine)
                && let Some(aligned) = self.aligner.align(
                    renderer,
                    &anchor.image,
                    &current,
                    current_matte.as_deref(),
                    visible(anchor.area),
                    found.consensus.motion,
                    found.consensus.model == Motion::Perspective,
                    (settings.tolerance_pixels * 3.).max(2.),
                )?
            {
                found.consensus.motion = aligned;
            }
            let total = found.consensus.motion.compose(anchor.from_reference);
            // Once part of the pinned surface passes behind the camera its corners no longer
            // form a quad; keep tracking and store the motion, which renderers clip.
            let (corners, stored_motion) = match total.transform_quad(surface) {
                Ok(corners) => (corners, None),
                Err(_) => {
                    let last = data.samples.last().map_or(surface, |s| s.corners);
                    let mut corners = last;
                    for (corner, (&p, &fallback)) in
                        corners.iter_mut().zip(surface.iter().zip(&last))
                    {
                        *corner = total
                            .project(p)
                            .map_or(fallback, |q| q.map(|v| v.clamp(-1e6, 1e6)));
                    }
                    (corners, Some(total))
                }
            };
            // The area may legitimately leave the image while the surface is still tracked.
            let seen = found
                .consensus
                .motion
                .transform_quad(anchor.area)
                .unwrap_or(anchor.area);
            motion = found.consensus.motion;
            let accepted = found.features.iter().filter(|f| f.accepted).count();
            let sample = planar::Sample {
                time: at(frame)?,
                corners,
                features: thin(found.features),
                rms_pixels: found.consensus.rms_pixels,
                motion: stored_motion,
            };
            bytes += size(&sample);
            if bytes > max_bytes {
                stop = Stop::SizeLimit { frame };
                break;
            }
            if !observe(frame, &current, &sample) {
                stop = Stop::Cancelled;
            }
            data.samples.push(sample);
            let candidate = Box::new((current, current_levels, current_matte, total, seen));
            if accepted >= settings.reanchor_below as usize {
                previous = Some(candidate);
            } else {
                // Too few matches remain: continue from this frame with freshly detected texture.
                match reanchor(candidate, accepted + 1) {
                    Ok(fresh) => {
                        anchor = fresh;
                        motion = Homography::IDENTITY;
                        anchors += 1;
                    }
                    Err(candidate) => previous = Some(candidate),
                }
            }
        }
        data.samples
            .sort_by(|a, b| a.time.compare(b.time).expect("frame times are comparable"));
        data.validate().map_err(|e| error(e.to_string()))?;
        Ok(Range {
            data,
            stop,
            backward: None,
            anchors,
        })
    }
    pub fn new(renderer: &Renderer) -> Self {
        let module = renderer
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("planar tracking"),
                source: wgpu::ShaderSource::Wgsl(include_str!("planar.wgsl").into()),
            });
        let pipeline = |entry| {
            renderer
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        let downsample_module =
            renderer
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("planar pyramid"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("planar_pyramid.wgsl").into()),
                });
        let downsample =
            renderer
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("planar pyramid"),
                    layout: None,
                    module: &downsample_module,
                    entry_point: Some("downsample"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        Self {
            downsample,
            detect: pipeline("detect"),
            correlate: pipeline("correlate"),
            aligner: crate::refine::AreaAligner::new(renderer),
            owner: renderer.device_key(),
        }
    }
    /// Tracks from the `reference` frame forward to `forward_to` and backward to `back_to`,
    /// joining both at the reference, so a clear middle frame can anchor a whole shot.
    /// Either end may equal the reference to track one way only.
    #[allow(clippy::too_many_arguments)]
    pub fn track_both(
        &self,
        renderer: &Renderer,
        plan: &veylvik_render::Plan,
        occlusion: Option<(&veylvik_render::Plan, &veylvik_render::ImageInputs)>,
        inputs: &veylvik_render::ImageInputs,
        surface: Quad,
        region: Option<Quad>,
        [back_to, reference, forward_to]: [i64; 3],
        rate: veylvik_document::Time,
        settings: Settings,
        max_bytes: usize,
        observe: &mut Observer<'_>,
    ) -> Result<Range> {
        if back_to > reference || forward_to < reference || back_to == forward_to {
            return Err(error("Track ends must surround the reference frame"));
        }
        let one_way = |ends: [i64; 2], budget: usize, observe: &mut Observer<'_>| {
            self.track_range(
                renderer, plan, occlusion, inputs, surface, region, ends, rate, settings, budget,
                observe,
            )
        };
        if back_to == reference {
            return one_way([reference, forward_to], max_bytes, observe);
        }
        if forward_to == reference {
            let mut only = one_way([reference, back_to], max_bytes, observe)?;
            only.backward = Some(std::mem::replace(&mut only.stop, Stop::Complete));
            return Ok(only);
        }
        let forward = one_way([reference, forward_to], max_bytes, observe)?;
        let used = serde_json::to_vec_pretty(&forward.data).map_or(max_bytes, |v| v.len() * 2);
        let mut backward = one_way(
            [reference, back_to],
            max_bytes.saturating_sub(used),
            observe,
        )?;
        // Both directions start with the same reference sample; keep one.
        backward.data.samples.retain(|s| {
            s.time
                .compare(forward.data.reference_time)
                .is_ok_and(|o| o.is_ne())
        });
        let mut data = forward.data;
        data.samples.extend(backward.data.samples);
        data.samples
            .sort_by(|a, b| a.time.compare(b.time).expect("frame times are comparable"));
        data.validate()?;
        Ok(Range {
            data,
            stop: forward.stop,
            backward: Some(backward.stop),
            anchors: forward.anchors + backward.anchors - 1,
        })
    }
    /// Polishes a feature solution by aligning every pixel of `area` (on `reference`) directly;
    /// `None` when that does not clearly improve the match. Exposed for tests and tools.
    #[allow(clippy::too_many_arguments)]
    pub fn align_area(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        occlusion: Option<&GpuFrame>,
        area: Quad,
        initial: Homography,
        perspective: bool,
    ) -> Result<Option<Homography>> {
        self.aligner.align(
            renderer,
            reference,
            current,
            occlusion,
            area,
            initial,
            perspective,
            6.,
        )
    }
    /// Return up to `grid²` distributed corners in normalized image coordinates.
    pub fn detect(
        &self,
        renderer: &Renderer,
        frame: &GpuFrame,
        surface: Quad,
        settings: Settings,
    ) -> Result<Vec<Point>> {
        self.detect_masked(renderer, frame, surface, settings, None)
    }
    /// As [`Self::detect`], skipping corners whose patch touches `occlusion` on this frame.
    pub fn detect_masked(
        &self,
        renderer: &Renderer,
        frame: &GpuFrame,
        surface: Quad,
        settings: Settings,
        occlusion: Option<&GpuFrame>,
    ) -> Result<Vec<Point>> {
        self.validate(renderer, frame, frame, settings)?;
        planar::validate_quad(surface).map_err(|e| error(e.to_string()))?;
        if surface.iter().flatten().any(|v| !(0. ..=1.).contains(v)) {
            return Err(error("Detection surface must be inside the source image"));
        }
        let params = parameters(frame, Homography::IDENTITY, surface, settings)?;
        let values = self.run(
            renderer,
            frame,
            frame,
            &self.detect,
            &params,
            &[[0.; 4]],
            settings.grid * settings.grid * 4,
            [(settings.grid * settings.grid).div_ceil(64), 1],
            occlusion,
        )?;
        let mut points = Vec::new();
        for v in values.as_chunks::<4>().0 {
            if v[2] > 0.0001
                && points.iter().all(|p: &Point| {
                    ((p[0] * f64::from(frame.width()) - f64::from(v[0])).powi(2)
                        + (p[1] * f64::from(frame.height()) - f64::from(v[1])).powi(2))
                        >= f64::from(settings.patch_radius * 2).powi(2)
                })
            {
                points.push([
                    (f64::from(v[0]) + 0.5) / f64::from(frame.width()),
                    (f64::from(v[1]) + 0.5) / f64::from(frame.height()),
                ]);
            }
        }
        if points.len() < 6 {
            return Err(error(
                "Surface has too few distinct features; choose a larger or more textured region",
            ));
        }
        Ok(points)
    }
    /// All features are matched in one GPU dispatch/readback. CPU receives only
    /// correlation scores, then robustly fits their common projective motion.
    pub fn track(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        points: &[Point],
        prediction: Homography,
        settings: Settings,
    ) -> Result<TrackedPlane> {
        self.track_masked(
            renderer, reference, current, points, prediction, settings, None,
        )
    }
    /// As [`Self::track`]; matches whose patch lands on `occlusion` (the current frame's
    /// matte) are rejected before the plane is fitted.
    #[allow(clippy::too_many_arguments)]
    pub fn track_masked(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        points: &[Point],
        prediction: Homography,
        settings: Settings,
        occlusion: Option<&GpuFrame>,
    ) -> Result<TrackedPlane> {
        if !(6..=256).contains(&points.len()) {
            return Err(error("Planar tracking requires 6..256 features"));
        }
        let size = [reference.width(), reference.height()];
        let mut features = self.match_points(
            renderer, reference, current, points, None, prediction, settings, occlusion,
        )?;
        let consensus = estimate(&features, size, settings.tolerance_pixels, settings.motion)
            .map_err(|e| error(e.to_string()))?;
        for (f, &accepted) in features.iter_mut().zip(&consensus.accepted) {
            f.accepted = accepted;
        }
        Ok(TrackedPlane {
            consensus,
            features,
        })
    }
    /// Match every point independently: its best patch correlation within the search
    /// window around where `prediction` puts it (plus its own pixel offset, if given),
    /// to sub-pixel precision. No common motion is fitted, so points may move
    /// differently (parallax). 1..256 points.
    #[allow(clippy::too_many_arguments)]
    pub fn match_points(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        points: &[Point],
        offsets: Option<&[[f64; 2]]>,
        prediction: Homography,
        settings: Settings,
        occlusion: Option<&GpuFrame>,
    ) -> Result<Vec<Feature>> {
        if offsets.is_some_and(|o| o.len() != points.len()) {
            return Err(error("One offset per point"));
        }
        let offset = |i: usize| offsets.map_or([0.0; 2], |o| o[i]);
        self.validate(renderer, reference, current, settings)?;
        if !(1..=256).contains(&points.len()) {
            return Err(error("Matching takes 1..256 points"));
        }
        let size = [reference.width(), reference.height()];
        let mut gpu_points = Vec::with_capacity(points.len());
        let mut origins = Vec::with_capacity(points.len());
        for (i, p) in points.iter().enumerate() {
            let pixel = [0, 1].map(|axis| (p[axis] * f64::from(size[axis]) - 0.5).round());
            if pixel.iter().enumerate().any(|(axis, &v)| {
                !v.is_finite()
                    || v < f64::from(settings.patch_radius)
                    || v + f64::from(settings.patch_radius) >= f64::from(size[axis])
            }) {
                return Err(error(
                    "Planar feature patch must fit inside the reference image",
                ));
            }
            let shift = offset(i);
            gpu_points.push([
                pixel[0] as f32,
                pixel[1] as f32,
                shift[0] as f32,
                shift[1] as f32,
            ]);
            origins.push([
                (pixel[0] + 0.5) / f64::from(size[0]),
                (pixel[1] + 0.5) / f64::from(size[1]),
            ]);
        }
        let params = parameters(
            reference,
            prediction,
            [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            settings,
        )?;
        let side = settings.search_radius * 2 + 1;
        let count = side * side;
        let values = self.run(
            renderer,
            reference,
            current,
            &self.correlate,
            &params,
            &gpu_points,
            count * points.len() as u32,
            [count.div_ceil(64), points.len() as u32],
            occlusion,
        )?;
        let mut features = Vec::with_capacity(points.len());
        for (i, (&origin, scores)) in origins
            .iter()
            .zip(values.chunks_exact(count as usize))
            .enumerate()
        {
            let shift = offset(i);
            let (best, &score) = scores
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .ok_or_else(|| error("Missing planar scores"))?;
            let x = best as u32 % side;
            let y = best as u32 / side;
            let second = scores
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    (*i as u32 % side).abs_diff(x) > 2 || (*i as u32 / side).abs_diff(y) > 2
                })
                .map(|(_, &v)| v)
                .fold(-2_f32, f32::max);
            let subpixel = |stride: usize, coord: u32| -> f64 {
                if coord == 0 || coord + 1 == side {
                    return 0.;
                }
                let a = scores[best - stride];
                let b = scores[best];
                let c = scores[best + stride];
                let d = a - 2. * b + c;
                if a < -1. || c < -1. || d >= -1e-6 {
                    0.
                } else {
                    f64::from((0.5 * (a - c) / d).clamp(-0.5, 0.5))
                }
            };
            let predicted = prediction
                .project(origin)
                .ok_or_else(|| error("Invalid planar prediction"))?;
            features.push(Feature {
                reference: origin,
                position: [
                    predicted[0]
                        + (shift[0] + f64::from(x) - f64::from(settings.search_radius)
                            + subpixel(1, x))
                            / f64::from(size[0]),
                    predicted[1]
                        + (shift[1] + f64::from(y) - f64::from(settings.search_radius)
                            + subpixel(side as usize, y))
                            / f64::from(size[1]),
                ],
                correlation: f64::from(score.clamp(-1., 1.)),
                separation: f64::from((score - second).clamp(0., 3.)),
                accepted: false,
            });
        }
        Ok(features)
    }
    /// Filtered GPU image pyramid. Pixels remain on the renderer's device;
    /// retained levels count against its shared memory budget.
    pub fn pyramid(&self, renderer: &Renderer, frame: &GpuFrame) -> Result<Vec<GpuFrame>> {
        self.validate(renderer, frame, frame, Settings::default())?;
        let mut levels = Vec::new();
        for _ in 0..3 {
            let input = levels.last().unwrap_or(frame);
            if input.width().min(input.height()) < 64 {
                break;
            }
            let width = input.width() / 2;
            let height = input.height() / 2;
            let level = renderer.compute(|scratch| {
                let uniform = scratch.buffer(
                    16,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    "pyramid size",
                )?;
                let output = scratch.buffer(
                    u64::from(width) * u64::from(height) * 16,
                    wgpu::BufferUsages::STORAGE,
                    "planar pyramid",
                )?;
                scratch.queue().write_buffer(
                    &uniform,
                    0,
                    bytemuck::cast_slice(&[input.width(), input.height(), width, height]),
                );
                let bind = scratch
                    .device()
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("planar pyramid"),
                        layout: &self.downsample.get_bind_group_layout(0),
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: uniform.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: input.buffer().as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: output.as_entire_binding(),
                            },
                        ],
                    });
                let mut encoder = scratch.device().create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.downsample);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
                }
                scratch.run(encoder)?;
                scratch.frame(&output, width, height)
            })?;
            levels.push(level);
        }
        Ok(levels)
    }
    /// Coarse-to-fine projective matching, followed by full-resolution refinement.
    /// A coarse level may lack texture; only a successful full-resolution solve
    /// is returned. Reference levels can be reused throughout an analysis.
    #[allow(clippy::too_many_arguments)]
    pub fn track_multiscale(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        reference_levels: &[GpuFrame],
        current_levels: &[GpuFrame],
        points: &[Point],
        prediction: Homography,
        settings: Settings,
    ) -> Result<TrackedPlane> {
        self.track_multiscale_masked(
            renderer,
            reference,
            current,
            reference_levels,
            current_levels,
            points,
            prediction,
            settings,
            None,
        )
    }
    /// As [`Self::track_multiscale`]. Coarse levels only predict; the matte applies to
    /// the full-resolution solves that decide acceptance.
    #[allow(clippy::too_many_arguments)]
    pub fn track_multiscale_masked(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        reference_levels: &[GpuFrame],
        current_levels: &[GpuFrame],
        points: &[Point],
        mut prediction: Homography,
        settings: Settings,
        occlusion: Option<&GpuFrame>,
    ) -> Result<TrackedPlane> {
        self.validate(renderer, reference, current, settings)?;
        if reference_levels.len() != current_levels.len() {
            return Err(error("Mismatched tracking pyramids"));
        }
        let initial_prediction = prediction;
        for (a, b) in reference_levels.iter().zip(current_levels).rev() {
            let usable: Vec<_> = points
                .iter()
                .copied()
                .filter(|p| {
                    [a.width(), a.height()]
                        .iter()
                        .enumerate()
                        .all(|(axis, &size)| {
                            let pixel = (p[axis] * f64::from(size) - 0.5).round();
                            pixel >= f64::from(settings.patch_radius)
                                && pixel + f64::from(settings.patch_radius) < f64::from(size)
                        })
                })
                .collect();
            let coarse = Settings {
                tolerance_pixels: settings.tolerance_pixels.max(1.5),
                ..settings
            };
            if let Ok(found) = self.track(renderer, a, b, &usable, prediction, coarse) {
                prediction = found.consensus.motion;
            }
        }
        // Coarse texture can be ambiguous even when fine detail is distinctive.
        // Preserve the local solve as a fallback rather than regressing it.
        let found = self
            .track_masked(
                renderer, reference, current, points, prediction, settings, occlusion,
            )
            .or_else(|_| {
                self.track_masked(
                    renderer,
                    reference,
                    current,
                    points,
                    initial_prediction,
                    settings,
                    occlusion,
                )
            })?;
        Ok(self
            .track_masked(
                renderer,
                reference,
                current,
                points,
                found.consensus.motion,
                settings,
                occlusion,
            )
            .unwrap_or(found))
    }
    fn validate(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        settings: Settings,
    ) -> Result<()> {
        if self.owner != renderer.device_key()
            || !renderer.owns(reference)
            || !renderer.owns(current)
        {
            return Err(error("Planar frames belong to another renderer"));
        }
        if reference.region().x != 0
            || reference.region().y != 0
            || reference.region().width != reference.width()
            || reference.region().height != reference.height()
            || current.region() != reference.region()
            || current.width() != reference.width()
            || current.height() != reference.height()
        {
            return Err(error("Planar tracking requires matching uncropped frames"));
        }
        if !(3..=16).contains(&settings.patch_radius)
            || !(1..=48).contains(&settings.search_radius)
            || !(0.1..=32.).contains(&settings.tolerance_pixels)
            || !(8..=16).contains(&settings.grid)
        {
            return Err(error("Invalid planar tracking settings"));
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        pipeline: &wgpu::ComputePipeline,
        params: &[u32; 24],
        features: &[[f32; 4]],
        count: u32,
        dispatch: [u32; 2],
        occlusion: Option<&GpuFrame>,
    ) -> Result<Vec<f32>> {
        if let Some(matte) = occlusion
            && (!renderer.owns(matte)
                || matte.width() != reference.width()
                || matte.height() != reference.height())
        {
            return Err(error("Occlusion matte must match the tracked frames"));
        }
        Ok(renderer.compute(|scratch| {
            let uniform = scratch.buffer(
                96,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "planar settings",
            )?;
            let points = scratch.buffer(
                (features.len() * 16) as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                "planar features",
            )?;
            let bytes = u64::from(count) * 4;
            let output = scratch.buffer(
                bytes,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "planar scores",
            )?;
            let readback = scratch.buffer(
                bytes,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                "planar score readback",
            )?;
            scratch
                .queue()
                .write_buffer(&uniform, 0, bytemuck::cast_slice(params));
            scratch
                .queue()
                .write_buffer(&points, 0, bytemuck::cast_slice(features));
            // Auto layouts omit unused shader bindings for the detection entry point.
            let empty =
                scratch.buffer(16, wgpu::BufferUsages::STORAGE, "planar empty occlusion")?;
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: occlusion.map_or(empty.as_entire_binding(), |m| {
                        m.buffer().as_entire_binding()
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: reference.buffer().as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: output.as_entire_binding(),
                },
            ];
            if std::ptr::eq(pipeline, &self.correlate) {
                entries.push(wgpu::BindGroupEntry {
                    binding: 2,
                    resource: current.buffer().as_entire_binding(),
                });
                entries.push(wgpu::BindGroupEntry {
                    binding: 4,
                    resource: points.as_entire_binding(),
                });
            }
            let bind = scratch
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("planar tracking"),
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
            let mut encoder = scratch.device().create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(dispatch[0], dispatch[1], 1);
            }
            encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
            scratch.read(encoder, &readback)
        })?)
    }
}
fn parameters(
    frame: &GpuFrame,
    h: Homography,
    quad: Quad,
    settings: Settings,
) -> Result<[u32; 24]> {
    h.transform_quad([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
        .map_err(|e| error(e.to_string()))?;
    let w = f64::from(frame.width());
    let height = f64::from(frame.height());
    let scale = Homography {
        matrix: [w, 0., -0.5, 0., height, -0.5, 0., 0., 1.],
    };
    let pixel = scale.compose(h).compose(
        scale
            .inverse()
            .ok_or_else(|| error("Invalid image dimensions"))?,
    );
    let matrix = pixel.matrix.map(|v| (v / pixel.matrix[8]) as f32);
    if matrix.iter().any(|v| !v.is_finite()) {
        return Err(error("Invalid planar prediction"));
    }
    let mut params = [0; 24];
    params[..4].copy_from_slice(&[
        frame.width(),
        frame.height(),
        settings.patch_radius,
        settings.search_radius,
    ]);
    for (dst, v) in params[4..12].iter_mut().zip(matrix) {
        *dst = v.to_bits();
    }
    for (dst, p) in params[12..20].as_chunks_mut::<2>().0.iter_mut().zip(quad) {
        dst[0] = ((p[0] * w) as f32).to_bits();
        dst[1] = ((p[1] * height) as f32).to_bits();
    }
    params[20] = settings.grid;
    Ok(params)
}

mod erased {
    pub trait Encode {
        fn encoded_len(&self) -> usize;
    }
    impl<T: serde::Serialize> Encode for T {
        fn encoded_len(&self) -> usize {
            serde_json::to_vec_pretty(self).map_or(usize::MAX / 4, |v| v.len())
        }
    }
}

/// Saved diagnostics per frame: every rejected match up to a third of the budget, then
/// accepted matches spread evenly, so overlays stay representative and projects small.
fn thin(features: Vec<Feature>) -> Vec<Feature> {
    const KEEP: usize = 96;
    if features.len() <= KEEP {
        return features;
    }
    let (accepted, rejected): (Vec<_>, Vec<_>) = features.into_iter().partition(|f| f.accepted);
    let rejected_kept = rejected.len().min(KEEP / 3);
    let accepted_kept = (KEEP - rejected_kept).min(accepted.len());
    let spread = |items: Vec<Feature>, keep: usize| -> Vec<Feature> {
        let n = items.len();
        items
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep > 0 && (i * keep) / n != ((i + 1) * keep) / n)
            .map(|(_, f)| f)
            .collect()
    };
    let mut kept = spread(accepted, accepted_kept);
    kept.extend(spread(rejected, rejected_kept));
    kept
}
