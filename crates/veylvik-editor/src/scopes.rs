//! GPU scope accumulation and rendering. No pixel readback on the UI thread.
//! Keep trace colours, intensity midpoint, scale units and 75% targets aligned
//! with Klipp (klipp-effects/gpu_scopes and klipp-ui/widgets/scopes).
use crate::{Studio, display::ViewSettings};
use eframe::{egui, egui_wgpu};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use veylvik_render::GpuFrame;
use wgpu::util::DeviceExt;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Measure {
    cols: u32,
    step: u32,
    low_pass: bool,
}
impl Measure {
    fn words(self) -> u64 {
        4096 + u64::from(self.cols) * 4096 + 4194304 + 4104
    }
    fn samples(self, source: &GpuFrame) -> u32 {
        source.width().div_ceil(self.step) * source.height().div_ceil(self.step)
    }
}

const NAMES: [&str; 4] = ["Histogram", "Waveform", "RGB parade", "Vectorscope"];
struct Engine {
    compute: wgpu::ComputePipeline,
    histogram: wgpu::ComputePipeline,
    draw: wgpu::RenderPipeline,
    srgb: bool,
    queue: wgpu::Queue,
}
impl Engine {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let compute = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scope accumulation"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scopes_compute.wgsl").into()),
        });
        let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scope compute bindings"),
            entries: &[0, 1, 2].map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 2 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding == 0,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scope shared layout"),
            bind_group_layouts: &[Some(&bindings)],
            immediate_size: 0,
        });
        let histogram_shader = compute.clone();
        let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("scope accumulation"),
            layout: Some(&layout),
            module: &compute,
            entry_point: Some("collect"),
            compilation_options: Default::default(),
            cache: None,
        });
        let histogram = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("scope histogram smoothing"),
            layout: Some(&layout),
            module: &histogram_shader,
            entry_point: Some("histogram"),
            compilation_options: Default::default(),
            cache: None,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scope traces"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scopes_draw.wgsl").into()),
        });
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scope traces"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            compute,
            histogram,
            draw,
            srgb: format.is_srgb(),
            queue: queue.clone(),
        }
    }
    fn frame(
        self: &Arc<Self>,
        device: &wgpu::Device,
        source: Arc<GpuFrame>,
        view: ViewSettings,
        measure: Measure,
    ) -> Arc<ScopeFrame> {
        let bins = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scope bins"),
            size: measure.words() * 4,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scope input"),
            contents: bytemuck::cast_slice(&[
                source.width(),
                source.height(),
                u32::from(view.filmic),
                0,
                view.exposure.to_bits(),
                0,
                0,
                0,
                measure.cols,
                measure.step,
                u32::from(measure.low_pass),
                0,
            ]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scope accumulation"),
            layout: &self.compute.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.buffer().as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bins.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Arc::new(ScopeFrame {
            engine: self.clone(),
            source,
            view,
            measure,
            bins,
            bind,
            prepared: AtomicBool::new(false),
        })
    }
}
struct ScopeFrame {
    engine: Arc<Engine>,
    source: Arc<GpuFrame>,
    view: ViewSettings,
    measure: Measure,
    bins: wgpu::Buffer,
    bind: wgpu::BindGroup,
    prepared: AtomicBool,
}
impl Drop for ScopeFrame {
    fn drop(&mut self) {
        let source = self.source.clone();
        self.engine
            .queue
            .on_submitted_work_done(move || drop(source));
    }
}
impl ScopeFrame {
    fn prepare(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.prepared.swap(true, Ordering::Relaxed) {
            return;
        }
        encoder.clear_buffer(&self.bins, 0, None);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("scope accumulation"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.engine.compute);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.dispatch_workgroups(
            self.source.width().div_ceil(self.measure.step).div_ceil(8),
            self.source.height().div_ceil(self.measure.step).div_ceil(8),
            1,
        );
        drop(pass);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("scope histogram smoothing"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.engine.histogram);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
}
struct Paint {
    frame: Arc<ScopeFrame>,
    bind: wgpu::BindGroup,
}
impl Paint {
    fn new(
        device: &wgpu::Device,
        frame: Arc<ScopeFrame>,
        mode: usize,
        intensity: f32,
        size: [u32; 2],
    ) -> Self {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scope plot"),
            contents: bytemuck::cast_slice(&[
                mode as u32,
                u32::from(frame.engine.srgb),
                intensity.to_bits(),
                frame.measure.cols,
                size[0],
                size[1],
                frame.measure.samples(&frame.source),
                0,
            ]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scope plot"),
            layout: &frame.engine.draw.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.bins.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Self { frame, bind }
    }
}
impl egui_wgpu::CallbackTrait for Paint {
    fn prepare(
        &self,
        _: &wgpu::Device,
        _: &wgpu::Queue,
        _: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        _: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        self.frame.prepare(encoder);
        vec![]
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &egui_wgpu::CallbackResources,
    ) {
        pass.set_pipeline(&self.frame.engine.draw);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
pub struct Scopes {
    pub visible: bool,
    engine: Arc<Engine>,
    frame: Option<Arc<ScopeFrame>>,
    layout: usize,
    reference: bool,
    intensity: f32,
    low_pass: bool,
    scale: usize,
    updated: std::time::Instant,
}
impl Scopes {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            visible: false,
            engine: Arc::new(Engine::new(device, queue, format)),
            frame: None,
            layout: 4,
            reference: false,
            intensity: 0.5,
            low_pass: true,
            scale: 0,
            updated: std::time::Instant::now(),
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"visible":self.visible,"layout":if self.layout==4 {"Four-up"} else {NAMES[self.layout]},"source":if self.reference {"A"} else {"B"},"domain":"display-referred full-range sRGB","intensity":self.intensity,"low_pass":self.low_pass,"scale":(["10-bit","%","IRE"][self.scale]),"columns":self.frame.as_ref().map(|f|f.measure.cols),"samples":self.frame.as_ref().map(|f|[f.source.width().div_ceil(f.measure.step),f.source.height().div_ceil(f.measure.step)])})
    }
    pub fn clear(&mut self) {
        self.frame = None;
        self.reference = false;
    }
}
impl Studio {
    pub(super) fn scopes_panel(&mut self, ui: &mut egui::Ui) {
        if !self.scopes.visible {
            self.scopes.frame = None;
            return;
        }
        egui::Panel::bottom("colour-scopes").default_size(300.).min_size(170.).resizable(true).show_inside(ui,|ui| {
   ui.horizontal_wrapped(|ui| {
    ui.strong("Scopes");
    egui::ComboBox::from_id_salt("scope-layout").selected_text(if self.scopes.layout==4 {"Four-up"} else {NAMES[self.scopes.layout]}).show_ui(ui,|ui| {
     ui.selectable_value(&mut self.scopes.layout,4,"Four-up");
     for (i,name) in NAMES.iter().enumerate() {ui.selectable_value(&mut self.scopes.layout,i,*name);}
    }).response.on_hover_text("Show all four scopes or expand one. Double-click a plot to expand or return to Four-up.");
    egui::ComboBox::from_id_salt("scope-source").selected_text(if self.scopes.reference {"A: reference"} else {"B: live"}).show_ui(ui,|ui| {
     ui.selectable_value(&mut self.scopes.reference,false,"B: live");
     ui.add_enabled_ui(self.comparison.source().is_some(),|ui| {ui.selectable_value(&mut self.scopes.reference,true,"A: reference");});
    });
    ui.add_enabled(self.scopes.layout!=0,egui::Slider::new(&mut self.scopes.intensity,0.0..=1.).text("Trace")).on_hover_text("Waveform/parade/vector intensity only. Histogram uses fixed log-count scaling. 0.5 is normal intensity; 0 is six times dimmer, 1 six times brighter.");
    ui.add_enabled(matches!(self.scopes.layout,1|2|4),egui::Checkbox::new(&mut self.scopes.low_pass,"Low pass")).on_hover_text("Smooth waveform/parade measurement with a source-resolution-scaled Gaussian, like Klipp. Histogram and vectorscope always measure original pixels.");
    egui::ComboBox::from_id_salt("scope-scale").selected_text(["10-bit","%","IRE"][self.scopes.scale]).show_ui(ui,|ui|{for (i,name) in ["10-bit","%","IRE"].iter().enumerate(){ui.selectable_value(&mut self.scopes.scale,i,*name);}});
    ui.label("Display RGB · full range").on_hover_text("Post-preview OCIO, exposure and display transform, sRGB encoded. Straight RGB; fully transparent pixels excluded. Checkerboard, alpha view and zoom are not measured. Rec.709 luma coefficients. End bins include clipped values. This is not a scene-linear or legal-range signal.");
   });
   let source=if self.scopes.reference {self.comparison.source()} else {self.displayed.clone()};
   let Some(source)=source else {self.scopes.frame=None;ui.weak("Capture A in the viewer to measure a reference.");return;};
   let mut settings=self.effective_view();settings.alpha=false;settings.fast_filter=false;settings.filmic=source.settings.filmic;
   // A played picture already has exposure and the display transform in it.
   if source.baked {settings.exposure=0.0;settings.filmic=false;}
   let wide=ui.available_width()>ui.available_height()*2.5;
   let pane_columns=if self.scopes.layout==4 {if wide {4}else{2}}else{1};
   let plot_width=(ui.available_width()/pane_columns as f32-48.)*ui.ctx().pixels_per_point();
   let mut step=1;
   while self.transport.playing && u64::from(source.frame.width().div_ceil(step*2))*u64::from(source.frame.height().div_ceil(step*2))>=2_000_000 {step*=2;}
   while u64::from(source.frame.width().div_ceil(step))*u64::from(source.frame.height().div_ceil(step))>16_000_000 {step*=2;}
   let measure=Measure {cols:(plot_width.ceil() as u32).clamp(256,4096),step,low_pass:self.scopes.low_pass};
   let changed=self.scopes.frame.as_ref().is_none_or(|f| f.view!=settings || f.measure!=measure);
   let new_source=self.scopes.frame.as_ref().is_none_or(|f| !Arc::ptr_eq(&f.source,&source.frame));
   if changed || new_source && (!self.transport.playing || self.scopes.updated.elapsed()>=std::time::Duration::from_millis(66)) {
    self.scopes.frame=Some(self.scopes.engine.frame(self.renderer.device(),source.frame.clone(),settings,measure));
    self.scopes.updated=std::time::Instant::now();
   }
   let frame=self.scopes.frame.as_ref().unwrap().clone();
   ui.weak(format!("{} × {} samples · 10-bit levels · {}",frame.source.width().div_ceil(frame.measure.step),frame.source.height().div_ceil(frame.measure.step),if self.scopes.reference {"captured A"} else if self.displayed_ticket!=self.wanted || !Arc::ptr_eq(&frame.source,&source.frame) {"B · last measured preview"} else {"completed B preview"}));
   let area=ui.available_rect_before_wrap();
   let four=self.scopes.layout==4;
   let columns=pane_columns;let rows=if four {4/columns}else{1};
   let cell=egui::vec2(area.width()/columns as f32,area.height()/rows as f32);
   for n in 0..if four {4} else {1} {
    let mode=if four {n} else {self.scopes.layout};
    let rect=egui::Rect::from_min_size(area.min+egui::vec2((n%columns) as f32*cell.x,(n/columns) as f32*cell.y),cell).shrink(4.);
    let response=ui.interact(rect,ui.id().with(("scope",mode)),egui::Sense::click()).on_hover_text("Double-click to expand this scope or return to Four-up");
    if response.double_clicked() {self.scopes.layout=if four {mode} else {4};}
    plot(ui,self.renderer.device(),frame.clone(),rect,mode,self.scopes.intensity,self.scopes.scale);
   }
   ui.allocate_rect(area,egui::Sense::hover());
  });
    }
}
fn plot(
    ui: &egui::Ui,
    device: &wgpu::Device,
    frame: Arc<ScopeFrame>,
    rect: egui::Rect,
    mode: usize,
    intensity: f32,
    scale: usize,
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        [
            "Histogram · RGB overlay · log count",
            "Waveform · luma",
            "RGB parade",
            "Vectorscope · 75% targets",
        ][mode],
        egui::FontId::proportional(12.),
        ui.visuals().text_color(),
    );
    let mut chart = egui::Rect::from_min_max(
        rect.min + egui::vec2(32., 20.),
        rect.max - egui::vec2(8., 16.),
    );
    if chart.width() < 8. || chart.height() < 8. {
        return;
    }
    if mode == 3 {
        let side = chart.width().min(chart.height());
        chart = egui::Rect::from_center_size(chart.center(), egui::vec2(side, side));
    }
    painter.add(egui_wgpu::Callback::new_paint_callback(
        chart,
        Paint::new(
            device,
            frame,
            mode,
            intensity,
            [
                (chart.width() * ui.ctx().pixels_per_point())
                    .round()
                    .max(1.) as u32,
                (chart.height() * ui.ctx().pixels_per_point())
                    .round()
                    .max(1.) as u32,
            ],
        ),
    ));
    let grid = egui::Stroke::new(
        1.0_f32,
        egui::Color32::from_rgba_unmultiplied(180, 145, 75, 65),
    );
    let label = egui::Color32::from_gray(170);
    if mode == 3 {
        let radius = chart.width() * 0.5;
        for fraction in [0.25, 0.5, 0.75] {
            painter.circle_stroke(chart.center(), radius * fraction, grid);
        }
        for i in 0..120 {
            let a = i as f32 / 120. * std::f32::consts::TAU;
            let b = (i + 1) as f32 / 120. * std::f32::consts::TAU;
            let color = egui::Color32::from(egui::ecolor::Hsva::new(i as f32 / 120., 1., 1., 0.55));
            painter.line_segment(
                [
                    chart.center() + egui::vec2(a.cos(), a.sin()) * radius,
                    chart.center() + egui::vec2(b.cos(), b.sin()) * radius,
                ],
                egui::Stroke::new(1.2_f32, color),
            );
        }
        let skin = 123_f32.to_radians();
        let direction = egui::vec2(skin.cos(), -skin.sin());
        let amber = egui::Color32::from_rgb(220, 170, 80);
        painter.line_segment(
            [chart.center(), chart.center() + direction * radius * 0.92],
            egui::Stroke::new(1.0_f32, amber),
        );
        painter.text(
            chart.center() + direction * radius * 0.98,
            egui::Align2::CENTER_CENTER,
            "I",
            egui::FontId::monospace(10.),
            amber,
        );
        painter.circle_stroke(chart.center(), chart.width() * 0.5, grid);
        painter.circle_stroke(chart.center(), chart.width() * 0.25, grid);
        painter.line_segment(
            [
                egui::pos2(chart.left(), chart.center().y),
                egui::pos2(chart.right(), chart.center().y),
            ],
            grid,
        );
        painter.line_segment(
            [
                egui::pos2(chart.center().x, chart.top()),
                egui::pos2(chart.center().x, chart.bottom()),
            ],
            grid,
        );
        for (name, rgb) in [
            ("R", [1., 0., 0.]),
            ("Y", [1., 1., 0.]),
            ("G", [0., 1., 0.]),
            ("C", [0., 1., 1.]),
            ("B", [0., 0., 1.]),
            ("M", [1., 0., 1.]),
        ] {
            let rgb = rgb.map(|c| c * 0.75);
            let l = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            let point = egui::pos2(
                chart.left() + ((rgb[2] - l) / 1.8556 + 0.5) * chart.width(),
                chart.bottom() - ((rgb[0] - l) / 1.5748 + 0.5) * chart.height(),
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(point, egui::vec2(6., 6.)),
                0.,
                grid,
                egui::StrokeKind::Inside,
            );
            painter.text(
                point.lerp(chart.center(), 0.12),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::monospace(10.),
                label,
            );
        }
    } else {
        if mode != 0 {
            for tick in 0..=8 {
                let code = (tick * 128).min(1023);
                let y = chart.bottom() - chart.height() * code as f32 / 1023.;
                painter.line_segment(
                    [egui::pos2(chart.left(), y), egui::pos2(chart.right(), y)],
                    grid,
                );
                let text = match scale {
                    1 => format!("{:.0}", code as f32 / 1023. * 100.),
                    2 => format!("{:.0}", (code as f32 - 64.) / 876. * 100.),
                    _ => code.to_string(),
                };
                if chart.height() > 180. || tick % 2 == 0 {
                    painter.text(
                        egui::pos2(chart.left() - 4., y),
                        egui::Align2::RIGHT_CENTER,
                        text,
                        egui::FontId::monospace(10.),
                        label,
                    );
                }
            }
        }
        if mode == 0 {
            for tick in 0..=4 {
                let code = (tick * 256).min(1023);
                let x = chart.left() + chart.width() * code as f32 / 1023.;
                painter.line_segment(
                    [egui::pos2(x, chart.top()), egui::pos2(x, chart.bottom())],
                    grid,
                );
                let text = match scale {
                    1 => format!("{:.0}", code as f32 / 1023. * 100.),
                    2 => format!("{:.0}", (code as f32 - 64.) / 876. * 100.),
                    _ => code.to_string(),
                };
                let align = if tick == 0 {
                    egui::Align2::LEFT_TOP
                } else if tick == 4 {
                    egui::Align2::RIGHT_TOP
                } else {
                    egui::Align2::CENTER_TOP
                };
                if chart.width() > 250. || tick % 2 == 0 {
                    painter.text(
                        egui::pos2(x, chart.bottom() + 3.),
                        align,
                        text,
                        egui::FontId::monospace(10.),
                        label,
                    );
                }
            }
        }
        if scale == 2 {
            for code in [64., 940.] {
                let stroke = egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgba_unmultiplied(120, 200, 255, 90),
                );
                if mode == 0 {
                    let x = chart.left() + chart.width() * code / 1023.;
                    painter.line_segment(
                        [egui::pos2(x, chart.top()), egui::pos2(x, chart.bottom())],
                        stroke,
                    );
                } else {
                    let y = chart.bottom() - chart.height() * code / 1023.;
                    painter.line_segment(
                        [egui::pos2(chart.left(), y), egui::pos2(chart.right(), y)],
                        stroke,
                    );
                }
            }
        }
        if mode == 2 {
            for (i, name) in ["R", "G", "B"].iter().enumerate() {
                let x = chart.left() + chart.width() * (i as f32 + 0.5) / 3.;
                painter.text(
                    egui::pos2(x, chart.bottom() + 3.),
                    egui::Align2::CENTER_TOP,
                    *name,
                    egui::FontId::monospace(10.),
                    label,
                );
                if i > 0 {
                    let x = chart.left() + chart.width() * i as f32 / 3.;
                    painter.line_segment(
                        [egui::pos2(x, chart.top()), egui::pos2(x, chart.bottom())],
                        grid,
                    );
                }
            }
        } else if mode != 0 {
            painter.text(
                chart.left_bottom() + egui::vec2(0., 3.),
                egui::Align2::LEFT_TOP,
                if mode == 0 {
                    "RGB level"
                } else {
                    "Left · image position"
                },
                egui::FontId::monospace(10.),
                label,
            );
            painter.text(
                chart.right_bottom() + egui::vec2(0., 3.),
                egui::Align2::RIGHT_TOP,
                if mode == 0 {
                    match scale {
                        1 => "100%",
                        2 => "109.5 IRE",
                        _ => "1023",
                    }
                } else {
                    "Right"
                },
                egui::FontId::monospace(10.),
                label,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires Vulkan"]
    fn full_uhd_white_keeps_counts_without_overflow() {
        use veylvik_document::{Operation, Time, nodes};
        let renderer = veylvik_render::Renderer::new().unwrap();
        let engine = Arc::new(Engine::new(
            renderer.device(),
            renderer.queue(),
            wgpu::TextureFormat::Rgba8Unorm,
        ));
        let mut doc = veylvik_document::blank(3840, 2160, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            Operation::Solid(nodes::Solid { color: [1.; 4] });
        let source = Arc::new(
            renderer
                .render_gpu(
                    &veylvik_render::Plan::compile(&doc).unwrap(),
                    veylvik_render::RenderRequest::full(Time::new(0, 1).unwrap()),
                    &Default::default(),
                )
                .unwrap(),
        );
        let frame = engine.frame(
            renderer.device(),
            source,
            ViewSettings::default(),
            Measure {
                cols: 256,
                step: 1,
                low_pass: true,
            },
        );
        let mut encoder = renderer
            .device()
            .create_command_encoder(&Default::default());
        frame.prepare(&mut encoder);
        let read = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&frame.bins, 1023 * 4, &read, 0, 4);
        renderer.queue().submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        renderer
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(20)),
            })
            .unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .unwrap();
        let data = read.slice(..).get_mapped_range();
        assert_eq!(
            u32::from_ne_bytes(data[..4].try_into().unwrap()),
            3840 * 2160 * 256
        );
        drop(data);
        read.unmap();
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn gpu_ten_bit_scopes_preserve_counts_and_fractional_levels() {
        use veylvik_document::{Operation, Time, nodes};
        let renderer = veylvik_render::Renderer::new().unwrap();
        let engine = Arc::new(Engine::new(
            renderer.device(),
            renderer.queue(),
            wgpu::TextureFormat::Rgba8Unorm,
        ));
        let mut doc = veylvik_document::blank(4, 1, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
            asset: "pixels".into(),
        });
        let mut inputs = veylvik_render::ImageInputs::default();
        inputs.insert(
            "pixels",
            Arc::new(
                veylvik_render::Image::new(
                    4,
                    1,
                    vec![
                        [0.25, 0.25, 0.25, 1.],
                        [0.5, 0., 0., 0.5],
                        [0., 0., 0., 0.],
                        [2., 2., 2., 1.],
                    ],
                )
                .unwrap(),
            ),
        );
        let source = Arc::new(
            renderer
                .render_gpu(
                    &veylvik_render::Plan::compile(&doc).unwrap(),
                    veylvik_render::RenderRequest::full(Time::new(0, 1).unwrap()),
                    &inputs,
                )
                .unwrap(),
        );
        for cols in [256, 1024] {
            let measure = Measure {
                cols,
                step: 1,
                low_pass: false,
            };
            let frame = engine.frame(
                renderer.device(),
                source.clone(),
                ViewSettings::default(),
                measure,
            );
            let mut encoder = renderer
                .device()
                .create_command_encoder(&Default::default());
            frame.prepare(&mut encoder);
            frame.prepare(&mut encoder);
            let size = measure.words() * 4;
            let read = renderer.device().create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&frame.bins, 0, &read, 0, size);
            renderer.queue().submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            renderer
                .device()
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(10)),
                })
                .unwrap();
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
                .unwrap();
            let mapped = read.slice(..).get_mapped_range();
            let bins: &[u32] = bytemuck::cast_slice(&mapped);
            assert_eq!(bins[..1024].iter().sum::<u32>(), 768);
            assert_eq!(bins[1023], 512); // unpremultiplied red and HDR white
            assert_eq!(bins[549] + bins[550], 256); // fractional sRGB grey, not an 8-bit staircase
            let vector = 4096 + cols as usize * 4096;
            let count = bins[vector..vector + 4194304].iter().sum::<u32>();
            assert!((count as i32 - 768).abs() <= 2);
            let hist = vector + 4194304;
            assert!(bins[hist + 549] > 0 && bins[hist + 4096] > 0);
            drop(mapped);
            read.unmap();
        }
    }
}
