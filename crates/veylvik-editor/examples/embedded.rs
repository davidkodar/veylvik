//! The editor inside another program: a stand-in host with tabs and a side panel of its own gives Veylvik's editor the area that is left, on the host's own GPU device. Run: cargo run -p veylvik-editor --example embedded -- [PROJECT.veylvik]
use eframe::{egui, egui_wgpu};
use std::sync::Arc;
use veylvik_editor::{
    Capabilities, Editor, HOST_INTERFACE, LiveControl, Setup,
    host::{Pick, Picked, Purpose},
};

/// The host's own media pool: one clip it "decodes" itself, moving colour bars.
struct Pool;
struct Bars;
impl veylvik_render::FrameSource for Bars {
    fn info(&self) -> Option<veylvik_render::SourceInfo> {
        let time = |ticks| veylvik_document::Time { ticks, rate: 1 };
        Some(veylvik_render::SourceInfo {
            width: 640,
            height: 360,
            duration: Some(time(4)),
            frame_rate: Some(time(25)),
            frames: Some(100),
            description: "The host's own clip, decoded by the host".into(),
        })
    }
    fn byte_budget(&self) -> u64 {
        640 * 360 * 16
    }
    fn frame(
        &self,
        time: veylvik_document::Time,
    ) -> veylvik_render::Result<Arc<veylvik_render::Image>> {
        let shift = (time.ticks as f64 / f64::from(time.rate.max(1)) * 160.0) as u32;
        let colours = [
            [0.8, 0.8, 0.8],
            [0.8, 0.8, 0.1],
            [0.1, 0.8, 0.8],
            [0.1, 0.8, 0.1],
            [0.8, 0.1, 0.8],
            [0.8, 0.1, 0.1],
            [0.1, 0.1, 0.8],
        ];
        let pixels = (0..360u32)
            .flat_map(|_| 0..640u32)
            .map(|x| {
                let [r, g, b] = colours[(((x + shift) % 640) * 7 / 640) as usize];
                [r, g, b, 1.0]
            })
            .collect();
        Ok(Arc::new(veylvik_render::Image::new(640, 360, pixels)?))
    }
}
impl veylvik_editor::host::Host for Pool {
    fn pick(&self, request: &Pick) -> Result<Option<Picked>, String> {
        match request.purpose {
            // The host has no file dialog here: importing gives its one clip.
            Purpose::Import => Ok(Some(Picked::Media {
                key: "host-media:bars".into(),
                label: "Colour bars".into(),
                file: None,
            })),
            _ => Err("this host only offers its own media".into()),
        }
    }
    fn media(&self, key: &str) -> Option<Arc<dyn veylvik_render::FrameSource>> {
        (key == "host-media:bars").then(|| Arc::new(Bars) as Arc<dyn veylvik_render::FrameSource>)
    }
}

struct Host {
    editor: Editor,
    tab: usize,
    notes: String,
}
impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        egui::Panel::top("host tabs").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("A HOST EDITOR").strong());
                ui.separator();
                for (index, name) in ["Edit", "Colour", "Veylvik", "Export"]
                    .into_iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut self.tab, index, name);
                }
                ui.separator();
                ui.label(format!(
                    "the host reads: revision {}{}, {} nodes",
                    self.editor.revision(),
                    if self.editor.unsaved() {
                        ", unsaved"
                    } else {
                        ""
                    },
                    self.editor.composition().nodes.len()
                ));
            });
        });
        egui::Panel::left("host side")
            .exact_size(170.0)
            .show_inside(ui, |ui| {
                ui.heading("The host");
                ui.label("The host's own panel, in the host's own look.");
                let _ = ui.button("A host button");
                ui.label("Typing here is the host's:");
                ui.text_edit_singleline(&mut self.notes);
                ui.label(if self.editor.has_keyboard() {
                    "Keys go to Veylvik."
                } else {
                    "Keys go to the host."
                });
                ui.label("Everything right of here, under the host's tabs, is Veylvik's editor.");
            });
        if self.tab == 2 {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| self.editor.ui(ui));
        } else {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("Another page of the host. The Veylvik tab keeps its state.")
                });
            });
        }
    }
    fn on_exit(&mut self) {
        self.editor.close();
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    // The host owns the GPU device; the editor is handed it.
    let (instance, adapter, device, queue) = pollster::block_on(veylvik_render::create_gpu())?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("A host with Veylvik inside")
            .with_inner_size([1600.0, 960.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetupExisting {
                instance,
                adapter,
                device,
                queue,
            }
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "host",
        options,
        Box::new(move |cc| {
            // A look of the host's own, to show that the editor does not change it.
            cc.egui_ctx.set_theme(egui::Theme::Light);
            let state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("Vulkan render state unavailable")?;
            let editor = Editor::new(Setup {
                interface: HOST_INTERFACE,
                context: cc.egui_ctx.clone(),
                device: state.device.clone(),
                queue: state.queue.clone(),
                adapter: state.adapter.get_info(),
                target_format: state.target_format,
                opens: match project {
                    // A composition the host holds: read from a file here, as a host would
                    // read it from its own project.
                    Some(path) => veylvik_editor::Opens::Composition {
                        composition: Box::new(
                            veylvik_editor::load_composition(&path)
                                .map_err(|error| error.to_string())?,
                        ),
                        assets: path.parent().unwrap_or(&path).to_path_buf(),
                    },
                    None => veylvik_editor::Opens::File(None),
                },
                live_control: LiveControl::Off,
                capabilities: Capabilities::embedded(),
                host: Some(Arc::new(Pool)),
                renderer: None,
            })
            .map_err(|error| error.to_string())?;
            Ok(Box::new(Host {
                editor,
                tab: 2,
                notes: String::new(),
            }))
        }),
    )?;
    Ok(())
}
