//! Read-only GPU accounting, sampled without blocking the preview worker.
use eframe::egui;
use veylvik_render::Renderer;
pub fn show(ctx: &egui::Context, open: &mut bool, renderer: &Renderer) {
    if !*open {
        return;
    }
    egui::Window::new("Renderer statistics").open(open).show(ctx, |ui| {
        ui.label(&renderer.adapter.name);
        match renderer.try_diagnostics() {
            Ok(Some(stats)) => {
                let mib=|bytes:u64|format!("{:.1} MiB",bytes as f64/(1024.0*1024.0));
                egui::Grid::new("renderer-statistics").striped(true).show(ui, |ui| {
                    for (label,value) in [
                        ("Resident GPU buffers",mib(stats.resources.resident_bytes)),
                        ("Held frames",mib(stats.resources.held_bytes)),
                        ("Idle buffer pool",mib(stats.resources.idle_bytes)),
                        ("Peak resident buffers",mib(stats.resources.peak_resident_bytes)),
                        ("Buffer budget",mib(stats.budget_bytes)),
                        ("Frame cache",format!("{} frames / {}",stats.frames.entries,mib(stats.frames.bytes))),
                        ("Frame cache hits / misses",format!("{} / {}",stats.frames.hits,stats.frames.misses)),
                        ("Vector cache",format!("{} masks / {}",stats.vectors.entries,mib(stats.vectors.bytes))),
                        ("Vector cache hits / misses",format!("{} / {}",stats.vectors.hits,stats.vectors.misses)),
                        ("Buffers created / reused",format!("{} / {}",stats.resources.buffers_created,stats.resources.buffers_reused)),
                        ("Source uploads / reused",format!("{} / {}",stats.resources.source_uploads,stats.resources.source_reuses)),
                    ] { ui.label(label);ui.monospace(value);ui.end_row(); }
                });
                ui.separator();ui.weak("Declared buffers only; excludes driver and vector rasterizer scratch memory.");
            }
            Ok(None) => { ui.label("Renderer busy; counters will update between frames."); }
            Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED,error.to_string()); }
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}
