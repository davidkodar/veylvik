//! GPU scope accumulation and rendering. No pixel readback on the UI thread.
use crate::{Studio, display::ViewSettings};
use eframe::{egui, egui_wgpu};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use veylvik_render::GpuFrame;
use wgpu::util::DeviceExt;
const WORDS: u64 = 82949;
const NAMES: [&str; 4] = ["Histogram", "Waveform", "RGB parade", "Vectorscope"];
struct Engine {
    compute: wgpu::ComputePipeline,
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
        let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("scope accumulation"),
            layout: None,
            module: &compute,
            entry_point: Some("collect"),
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
    ) -> Arc<ScopeFrame> {
        let bins = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scope bins"),
            size: WORDS * 4,
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
            self.source.width().min(512).div_ceil(8),
            self.source.height().min(512).div_ceil(8),
            1,
        );
    }
}
struct Paint {
    frame: Arc<ScopeFrame>,
    bind: wgpu::BindGroup,
}
impl Paint {
    fn new(device: &wgpu::Device, frame: Arc<ScopeFrame>, mode: usize, intensity: f32) -> Self {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scope plot"),
            contents: bytemuck::cast_slice(&[
                mode as u32,
                u32::from(frame.engine.srgb),
                intensity.to_bits(),
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
}
impl Scopes {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            visible: false,
            engine: Arc::new(Engine::new(device, queue, format)),
            frame: None,
            layout: 4,
            reference: false,
            intensity: 1.,
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"visible":self.visible,"layout":if self.layout==4 {"Four-up"} else {NAMES[self.layout]},"source":if self.reference {"A"} else {"B"},"domain":"display-referred full-range sRGB","intensity":self.intensity,"samples":self.frame.as_ref().map(|f|[f.source.width().min(512),f.source.height().min(512)])})
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
    ui.add(egui::Slider::new(&mut self.scopes.intensity,0.25..=3.).text("Trace")).on_hover_text("Trace intensity only; does not change pixels or measured levels. Density is normalised to each plot's peak.");
    ui.label("Display RGB · full range").on_hover_text("Post-preview OCIO, exposure and display transform, sRGB encoded. Straight RGB; fully transparent pixels excluded. Checkerboard, alpha view and zoom are not measured. Rec.709 luma coefficients. End bins include clipped values. This is not a scene-linear or legal-range signal.");
   });
   let source=if self.scopes.reference {self.comparison.source()} else {self.displayed.clone()};
   let Some(source)=source else {self.scopes.frame=None;ui.weak("Capture A in the viewer to measure a reference.");return;};
   let mut settings=self.effective_view();settings.alpha=false;settings.fast_filter=false;settings.filmic=source.settings.filmic;
   if self.scopes.frame.as_ref().is_none_or(|f| !Arc::ptr_eq(&f.source,&source.frame) || f.view!=settings) {
    self.scopes.frame=Some(self.scopes.engine.frame(self.renderer.device(),source.frame.clone(),settings));
   }
   let frame=self.scopes.frame.as_ref().unwrap().clone();
   ui.weak(format!("{} × {} samples · {}",source.frame.width().min(512),source.frame.height().min(512),if self.scopes.reference {"captured A"} else if self.displayed_ticket!=self.wanted {"B updating — last completed preview"} else {"completed B preview"}));
   let area=ui.available_rect_before_wrap();
   let four=self.scopes.layout==4;
   let columns=if four {2} else {1};let rows=columns;
   let cell=egui::vec2(area.width()/columns as f32,area.height()/rows as f32);
   for n in 0..if four {4} else {1} {
    let mode=if four {n} else {self.scopes.layout};
    let rect=egui::Rect::from_min_size(area.min+egui::vec2((n%columns) as f32*cell.x,(n/columns) as f32*cell.y),cell).shrink(4.);
    let response=ui.interact(rect,ui.id().with(("scope",mode)),egui::Sense::click()).on_hover_text("Double-click to expand this scope or return to Four-up");
    if response.double_clicked() {self.scopes.layout=if four {mode} else {4};}
    plot(ui,self.renderer.device(),frame.clone(),rect,mode,self.scopes.intensity);
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
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        [
            "Histogram · relative RGB density",
            "Waveform · luma",
            "RGB parade",
            "Vectorscope · Cb / Cr · 100% targets",
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
        Paint::new(device, frame, mode, intensity),
    ));
    let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(35));
    let label = egui::Color32::from_gray(170);
    if mode == 3 {
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
        for step in 0..=4 {
            let y = chart.bottom() - chart.height() * step as f32 / 4.;
            painter.line_segment(
                [egui::pos2(chart.left(), y), egui::pos2(chart.right(), y)],
                grid,
            );
            let text = if mode == 0 {
                format!("{}", step * 25)
            } else {
                format!("{:.2}", step as f32 / 4.)
            };
            painter.text(
                egui::pos2(chart.left() - 4., y),
                egui::Align2::RIGHT_CENTER,
                text,
                egui::FontId::monospace(10.),
                label,
            );
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
        } else {
            painter.text(
                chart.left_bottom() + egui::vec2(0., 3.),
                egui::Align2::LEFT_TOP,
                if mode == 0 {
                    "0 · RGB level"
                } else {
                    "Left · image position"
                },
                egui::FontId::monospace(10.),
                label,
            );
            painter.text(
                chart.right_bottom() + egui::vec2(0., 3.),
                egui::Align2::RIGHT_TOP,
                if mode == 0 { "1" } else { "Right" },
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
    fn gpu_scopes_measure_display_rgb_unpremultiply_and_exclude_transparency() {
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
        let frame = engine.frame(renderer.device(), source, ViewSettings::default());
        let mut encoder = renderer
            .device()
            .create_command_encoder(&Default::default());
        frame.prepare(&mut encoder);
        // Four-up callbacks must not clear/recompute an already prepared result.
        frame.prepare(&mut encoder);
        let read = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: WORDS * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&frame.bins, 0, &read, 0, WORDS * 4);
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
        assert_eq!(bins[..256].iter().sum::<u32>(), 3);
        assert_eq!(bins[137], 1); // sRGB encoding of linear 0.25.
        assert_eq!(bins[255], 2); // unpremultiplied red and clipped HDR white.
        assert_eq!(bins[256], 1);
        assert_eq!(bins[1024..17408].iter().sum::<u32>(), 3);
        assert_eq!(bins[17408..66560].iter().sum::<u32>(), 9);
        assert_eq!(bins[66560..82944].iter().sum::<u32>(), 3);
        assert_eq!(bins[66560 + 64 * 128 + 64], 2); // neutral grey and white at centre.
        drop(mapped);
        read.unmap();
    }
}
