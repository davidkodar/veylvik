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
    /// Desktop preference fallback; an explicit environment value retains precedence.
    pub fn from_env_with_default(mib: u64) -> Result<Self> {
        match std::env::var_os(VARIABLE) {
            Some(value) => Self::parse(Some(&value)),
            None => Self::parse(Some(OsStr::new(&mib.to_string()))),
        }
    }
    fn parse(value: Option<&OsStr>) -> Result<Self> {
        let mib = match value {
            None => 2048,
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
        renderer.cache_limit_bytes = renderer.cache_limit_bytes.min(self.budget_bytes / 8);
        renderer.vector_cache_limit_bytes = renderer
            .vector_cache_limit_bytes
            .min(self.budget_bytes / 16);
    }
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
pub fn renderer() -> Result<Renderer> {
    let limits = RenderLimits::from_env()?;
    let mut renderer = Renderer::new()?;
    limits.apply(&mut renderer);
    Ok(renderer)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_configuration_is_bounded_strict_and_defaults_without_environment_mutation() {
        assert_eq!(RenderLimits::parse(None).unwrap().budget_bytes, 2048 * MIB);
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
        assert_eq!(stats.frame_cache_limit_bytes, 8 * MIB);
        assert_eq!(stats.vector_cache_limit_bytes, 4 * MIB);
        let mut large = Renderer::new().unwrap();
        RenderLimits::parse(Some(OsStr::new("1024")))
            .unwrap()
            .apply(&mut large);
        let stats = large.try_diagnostics().unwrap().unwrap();
        assert_eq!(stats.budget_bytes, 1024 * MIB);
        assert_eq!(stats.idle_limit_bytes, 128 * MIB);
        assert_eq!(stats.frame_cache_limit_bytes, 64 * MIB);
        assert_eq!(stats.vector_cache_limit_bytes, 32 * MIB);
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
