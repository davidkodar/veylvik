//! Build identity and local system facts, sampled only while About is open.
use eframe::egui;
use std::{
    io::Read,
    time::{Duration, Instant},
};
use veylvik_render::Renderer;

fn read(path: &str) -> String {
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(2 * 1024 * 1024).read_to_string(&mut text);
    }
    text
}
fn field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        line.split_once(':')
            .filter(|(key, _)| key.trim() == name)
            .map(|(_, value)| value.trim())
    })
}
fn kib(text: &str, name: &str) -> Option<u64> {
    let mut parts = field(text, name)?.split_whitespace();
    let amount = parts.next()?.parse::<u64>().ok()?;
    (parts.next()? == "kB")
        .then(|| amount.checked_mul(1024))
        .flatten()
}
fn memory(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "Unavailable".into(),
        |bytes| format!("{:.2} GiB", bytes as f64 / 1_073_741_824.0),
    )
}
fn cpu(text: &str) -> String {
    let model = field(text, "model name")
        .or_else(|| field(text, "Hardware"))
        .unwrap_or(std::env::consts::ARCH);
    let threads = text
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "processor")
        })
        .count();
    if threads > 0 {
        format!("{model} · {threads} logical CPUs")
    } else {
        model.into()
    }
}
fn os() -> String {
    let text = read("/etc/os-release");
    let name = text
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"'))
        .unwrap_or(std::env::consts::OS);
    format!("{name} · {}", read("/proc/sys/kernel/osrelease").trim())
}
#[derive(Default)]
pub struct Controls {
    open: bool,
    cpu: String,
    os: String,
    ram: String,
    available: String,
    process: String,
    sampled: Option<Instant>,
    copied: bool,
}
impl Controls {
    pub fn begin(&mut self) {
        self.open = true;
        self.copied = false;
        self.sampled = None;
        if self.cpu.is_empty() {
            self.cpu = cpu(&read("/proc/cpuinfo"));
            self.os = os();
        }
    }
    fn rows(&self, renderer: &Renderer) -> Vec<(&'static str, String)> {
        let modified = match env!("VEYLVIK_BUILD_MODIFIED") {
            "true" => " · modified source",
            "false" => "",
            _ => " · source state unknown",
        };
        let adapter = &renderer.adapter;
        let device = match adapter.device_type {
            wgpu::DeviceType::DiscreteGpu => "Discrete GPU",
            wgpu::DeviceType::IntegratedGpu => "Integrated GPU",
            wgpu::DeviceType::VirtualGpu => "Virtual GPU",
            wgpu::DeviceType::Cpu => "Software / CPU",
            wgpu::DeviceType::Other => "Other device",
        };
        vec![
            ("Version", env!("CARGO_PKG_VERSION").into()),
            (
                "Build",
                format!(
                    "{} · {} · {}{modified}",
                    env!("VEYLVIK_BUILD_NUMBER"),
                    env!("VEYLVIK_BUILD_COMMIT"),
                    env!("VEYLVIK_BUILD_PROFILE")
                ),
            ),
            ("Platform", env!("VEYLVIK_BUILD_TARGET").into()),
            ("System", self.os.clone()),
            ("CPU", self.cpu.clone()),
            ("GPU", adapter.name.clone()),
            ("Graphics API", format!("{:?} · {device}", adapter.backend)),
            (
                "GPU driver",
                [adapter.driver.as_str(), adapter.driver_info.as_str()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
            (
                "System RAM",
                format!("{} total · {} available", self.ram, self.available),
            ),
            ("App RAM", format!("{} resident", self.process)),
            ("GPU buffer budget", memory(Some(renderer.budget_bytes))),
        ]
    }
    pub fn show(&mut self, ctx: &egui::Context, renderer: &Renderer) {
        if !self.open {
            return;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.open = false;
            return;
        }
        if self
            .sampled
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(1))
        {
            let mem = read("/proc/meminfo");
            self.ram = memory(kib(&mem, "MemTotal"));
            self.available = memory(kib(&mem, "MemAvailable"));
            self.process = memory(kib(&read("/proc/self/status"), "VmRSS"));
            self.sampled = Some(Instant::now());
        }
        let rows = self.rows(renderer);
        let mut open = self.open;
        let mut close = false;
        egui::Window::new("About Veylvik").open(&mut open)
            .collapsible(false).resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(520.0_f32.min(ctx.content_rect().width() - 40.0));
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(72.0), egui::Sense::hover());
                    crate::icons::paint_mark(ui.painter(), mark, true);
                    ui.add_space(6.0);
                    let cap = 28.0;
                    let (word, _) = ui.allocate_exact_size(
                        egui::vec2(crate::icons::WORDMARK_WIDTH * cap, cap * 1.04),
                        egui::Sense::hover(),
                    );
                    let ink = ui.visuals().strong_text_color();
                    crate::icons::paint_wordmark(
                        ui.painter(),
                        word.left_top() + egui::vec2(0.0, cap * 0.02),
                        cap,
                        ink,
                    );
                    ui.add_space(4.0);
                    ui.label("Compositing, effects and animation");
                    ui.add_space(12.0);
                });
                ui.separator();
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 210.0).max(120.0))
                    .show(ui, |ui| {
                        egui::Grid::new("about-details").num_columns(2).spacing([16.0, 8.0])
                            .show(ui, |ui| {
                                for (name, value) in &rows {
                                    ui.strong(*name);
                                    ui.add(egui::Label::new(if value.is_empty() { "Unavailable" } else { value }).wrap());
                                    ui.end_row();
                                }
                            });
                        ui.add_space(8.0);
                        ui.weak("GPU buffer budget is Veylvik’s configured limit, not total graphics memory.");
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(if self.copied { "Copied" } else { "Copy details" }).clicked() {
                        ctx.copy_text(format!("Veylvik\n{}", rows.iter().map(|(name, value)| format!("{name}: {value}")).collect::<Vec<_>>().join("\n")));
                        self.copied = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() { close = true; }
                    });
                });
            });
        self.open = open && !close;
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_memory_units_and_cpu_models_are_reported_without_guessing() {
        assert_eq!(
            kib("MemTotal: 1024 kB\nMemAvailable: 512 kB", "MemTotal"),
            Some(1_048_576)
        );
        assert_eq!(kib("VmRSS: 123 bytes", "VmRSS"), None);
        assert_eq!(kib("MemTotal: 18446744073709551615 kB", "MemTotal"), None);
        assert_eq!(kib("", "MemTotal"), None);
        assert_eq!(memory(None), "Unavailable");
        assert_eq!(
            cpu("processor: 0\nmodel name: Example CPU\nprocessor: 1"),
            "Example CPU · 2 logical CPUs"
        );
        assert_eq!(
            cpu("Hardware: ARM board\nprocessor: 0"),
            "ARM board · 1 logical CPUs"
        );
    }
}
