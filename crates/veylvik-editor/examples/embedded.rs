//! The editor inside another program: a stand-in host with tabs and a side panel of its own gives Veylvik's editor the area that is left, on the host's own GPU device. Run: cargo run -p veylvik-editor --example embedded -- [PROJECT.veylvik]
use eframe::{egui, egui_wgpu};
use veylvik_editor::{Capabilities, Editor, HOST_INTERFACE, LiveControl, Setup};

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
