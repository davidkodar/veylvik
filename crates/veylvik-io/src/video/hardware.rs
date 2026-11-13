//! Hardware candidates are accepted only after decoding a frame through a hardware-only filter.
use std::{path::PathBuf, process::Command};
use veylvik_render::VideoFormat;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodePolicy {
    Auto,
    Software,
    HardwareRequired,
}
#[derive(Clone)]
pub(super) enum Backend {
    Software,
    Nvdec(&'static str),
    Vaapi(PathBuf),
}
impl Backend {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Software => "Software fallback",
            Self::Nvdec(_) => "NVIDIA NVDEC",
            Self::Vaapi(_) => "VA-API",
        }
    }
    pub fn hardware(&self) -> bool {
        !matches!(self, Self::Software)
    }
    pub fn configure(&self, cmd: &mut Command) {
        match self {
            Self::Software => {}
            Self::Nvdec(codec) => {
                cmd.args([
                    "-hwaccel",
                    "cuda",
                    "-hwaccel_output_format",
                    "cuda",
                    "-c:v",
                    codec,
                ]);
            }
            Self::Vaapi(device) => {
                cmd.args([
                    "-hwaccel",
                    "vaapi",
                    "-hwaccel_output_format",
                    "vaapi",
                    "-hwaccel_device",
                ])
                .arg(device);
            }
        }
    }
}
pub(super) fn candidates(
    codec: &str,
    pix: &str,
    width: u32,
    height: u32,
) -> (VideoFormat, Vec<Backend>) {
    let format = match pix {
        "yuv420p" | "nv12" => VideoFormat::Nv12,
        "yuv420p10le" | "p010le" => VideoFormat::P010,
        _ => return (VideoFormat::Rgba64, vec![]),
    };
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return (VideoFormat::Rgba64, vec![]);
    }
    let decoder = match codec {
        "h264" => "h264_cuvid",
        "hevc" => "hevc_cuvid",
        "av1" => "av1_cuvid",
        "vp9" => "vp9_cuvid",
        "vp8" => "vp8_cuvid",
        "mpeg2video" => "mpeg2_cuvid",
        _ => return (VideoFormat::Rgba64, vec![]),
    };
    let mut candidates = vec![Backend::Nvdec(decoder)];
    // NVIDIA's VA-API bridge can return blank AV1/VP9 frames. Only native
    // Intel/AMD render nodes participate; NVDEC is the NVIDIA path.
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        let mut devices = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name();
                if !name.to_string_lossy().starts_with("renderD") {
                    return None;
                }
                let vendor = std::fs::read_to_string(e.path().join("device/vendor")).ok()?;
                matches!(vendor.trim(), "0x8086" | "0x1002")
                    .then(|| PathBuf::from("/dev/dri").join(name))
            })
            .collect::<Vec<_>>();
        devices.sort();
        candidates.extend(devices.into_iter().map(Backend::Vaapi));
    }
    (format, candidates)
}
