//! Host runtime configuration shared by CLI, MCP and the native editor.
use crate::Result;
use std::ffi::OsStr;
use veylvik_render::Renderer;
const MIB: u64 = 1024 * 1024;
const VARIABLE: &str = "VEYLVIK_GPU_BUDGET_MIB";
#[derive(Debug, Clone, Copy)]
pub struct RenderLimits {
    budget_bytes: u64,
}
impl RenderLimits {
    /// Read the process-local GPU buffer budget. Does not initialize Vulkan.
    pub fn from_env() -> Result<Self> {
        Self::parse(std::env::var_os(VARIABLE).as_deref())
    }
    /// Desktop preference fallback (None: follow the card); an explicit environment
    /// value retains precedence.
    pub fn from_env_with_default(mib: Option<u64>) -> Result<Self> {
        match (std::env::var_os(VARIABLE), mib) {
            (Some(value), _) => Self::parse(Some(&value)),
            (None, Some(mib)) => Self::parse(Some(OsStr::new(&mib.to_string()))),
            (None, None) => Self::parse(None),
        }
    }
    fn parse(value: Option<&OsStr>) -> Result<Self> {
        Self::parse_for(value, card_memory_mib())
    }
    /// With no setting the budget follows the card's memory.
    fn parse_for(value: Option<&OsStr>, card: Option<u64>) -> Result<Self> {
        let mib = match value {
            None => automatic_mib(card),
            Some(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| format!("{VARIABLE} must be UTF-8 decimal digits"))?;
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(format!("{VARIABLE} must be an integer in 64..=16384").into());
                }
                value
                    .parse::<u64>()
                    .map_err(|_| format!("{VARIABLE} must be an integer in 64..=16384"))?
            }
        };
        if !(64..=16384).contains(&mib) {
            return Err(format!("{VARIABLE} must be in 64..=16384 MiB").into());
        }
        Ok(Self {
            budget_bytes: mib * MIB,
        })
    }
    /// Configure a renderer before sharing it with workers. Budgets cover declared
    /// GPU buffers, not driver allocations, vector scratch or UI textures.
    pub fn apply(self, renderer: &mut Renderer) {
        renderer.budget_bytes = self.budget_bytes;
        renderer.pool_limit_bytes = renderer.pool_limit_bytes.min(self.budget_bytes / 4);
        // Finished frames may keep a quarter of the budget: a 4K frame is 127 MiB, and a
        // picture rendered at its own size can be far larger, so a small fixed cap kept nothing.
        renderer.cache_limit_bytes = self.budget_bytes / 4;
        // Drawn shapes and text may keep a quarter too: each is kept as a whole frame
        // (127 MiB at 4K), so the fixed 32 MiB held none, and a logo of fourteen shapes
        // was drawn again for every frame. It gives way to rendering like the rest.
        renderer.vector_cache_limit_bytes = self.budget_bytes / 4;
    }
}
/// The budget when nothing sets one: half the card's memory, between 256 MiB and
/// 8 GiB, leaving the rest to the desktop, video decoding, AI models and other
/// applications; 2 GiB when the card's memory is not known.
pub fn automatic_mib(card: Option<u64>) -> u64 {
    card.map_or(2048, |card| (card / 2).clamp(256, 8192))
}
/// The graphics card's memory in MiB, where the system says: the kernel's figure
/// (AMD and some other drivers) or nvidia-smi's. With several cards the largest
/// counts. Read once; None when neither source answers.
pub fn card_memory_mib() -> Option<u64> {
    static CARD: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *CARD.get_or_init(|| {
        let kernel = std::fs::read_dir("/sys/class/drm")
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                std::fs::read_to_string(entry.path().join("device/mem_info_vram_total")).ok()
            })
            .filter_map(|text| text.trim().parse::<u64>().ok())
            .map(|bytes| bytes / MIB)
            .max()
            .filter(|mib| *mib > 0);
        kernel.or_else(nvidia_memory_mib)
    })
}
/// Asks nvidia-smi, giving it two seconds.
fn nvidia_memory_mib() -> Option<u64> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if started.elapsed() < std::time::Duration::from_secs(2) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut text).ok()?;
    largest_mib(&text)
}
fn largest_mib(lines: &str) -> Option<u64> {
    lines
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .max()
        .filter(|mib| *mib > 0)
}
/// Construct the headless renderer after validating the environment setting.
/// Host memory the editor may retain for decoded footage across all sources: 1/16 of
/// physical RAM, clamped to 256 MiB..2 GiB (256 MiB when RAM size is unknown).
pub fn source_budget() -> u64 {
    static BUDGET: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        let total = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| mem_total_bytes(&text));
        source_budget_for(total)
    })
}
fn mem_total_bytes(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let mut fields = line.split_whitespace().skip(1);
    let kib: u64 = fields.next()?.parse().ok()?;
    (fields.next() == Some("kB")).then(|| kib.saturating_mul(1024))
}
fn source_budget_for(total: Option<u64>) -> u64 {
    total.map_or(256 * MIB, |total| (total / 16).clamp(256 * MIB, 2048 * MIB))
}
/// A headless renderer with its limits applied and the 3D renderer registered.
pub fn renderer() -> Result<Renderer> {
    let limits = RenderLimits::from_env()?;
    let mut renderer = Renderer::new()?;
    limits.apply(&mut renderer);
    renderer.set_scene_renderer(std::sync::Arc::new(veylvik_render3d::Engine::new(
        renderer.device(),
    )));
    Ok(renderer)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_configuration_is_bounded_strict_and_defaults_without_environment_mutation() {
        // Unset, the budget follows the card: half of it, within bounds.
        for (card, mib) in [
            (None, 2048),
            (Some(16376), 8188),
            (Some(24564), 8192),
            (Some(4096), 2048),
            (Some(300), 256),
        ] {
            assert_eq!(
                RenderLimits::parse_for(None, card).unwrap().budget_bytes,
                mib * MIB
            );
        }
        assert_eq!(largest_mib("16376\n 8192 \n"), Some(16376));
        assert_eq!(largest_mib("No devices were found\n"), None);
        for mib in [64, 512, 1024, 16384] {
            assert_eq!(
                RenderLimits::parse(Some(OsStr::new(&mib.to_string())))
                    .unwrap()
                    .budget_bytes,
                mib * MIB
            );
        }
        for invalid in [
            "",
            "0",
            "63",
            "16385",
            "-512",
            "+512",
            "512.0",
            "512 ",
            " 512",
            "18446744073709551616",
        ] {
            assert!(
                RenderLimits::parse(Some(OsStr::new(invalid))).is_err(),
                "{invalid}"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert!(RenderLimits::parse(Some(OsStr::from_bytes(&[255]))).is_err());
        }
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn configured_budget_reaches_renderer_accounting_and_cache_caps() {
        let mut renderer = Renderer::new().unwrap();
        RenderLimits::parse(Some(OsStr::new("64")))
            .unwrap()
            .apply(&mut renderer);
        let stats = renderer.try_diagnostics().unwrap().unwrap();
        assert_eq!(stats.budget_bytes, 64 * MIB);
        assert_eq!(stats.idle_limit_bytes, 16 * MIB);
        assert_eq!(stats.frame_cache_limit_bytes, 16 * MIB);
        assert_eq!(stats.vector_cache_limit_bytes, 16 * MIB);
        let mut large = Renderer::new().unwrap();
        RenderLimits::parse(Some(OsStr::new("1024")))
            .unwrap()
            .apply(&mut large);
        let stats = large.try_diagnostics().unwrap().unwrap();
        assert_eq!(stats.budget_bytes, 1024 * MIB);
        assert_eq!(stats.idle_limit_bytes, 128 * MIB);
        assert_eq!(stats.frame_cache_limit_bytes, 256 * MIB);
        assert_eq!(stats.vector_cache_limit_bytes, 256 * MIB);
    }
    #[test]
    fn source_budget_scales_with_ram_within_bounds() {
        let info = "MemTotal:       32768000 kB\nMemFree: 1 kB\n";
        assert_eq!(mem_total_bytes(info), Some(32_768_000 * 1024));
        assert_eq!(mem_total_bytes("MemTotal: 12 MB\n"), None);
        assert_eq!(source_budget_for(None), 256 * MIB);
        assert_eq!(source_budget_for(Some(2048 * MIB)), 256 * MIB);
        assert_eq!(source_budget_for(Some(16384 * MIB)), 1024 * MIB);
        assert_eq!(source_budget_for(Some(128 * 1024 * MIB)), 2048 * MIB);
    }
}
