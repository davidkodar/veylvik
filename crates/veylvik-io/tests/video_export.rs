use std::{path::PathBuf, process::Command};
use veylvik_document::{Operation, Time};
use veylvik_io::video_export::{self, Codec, Settings};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "veylvik-export-{}",
            veylvik_document::NodeId::new_v4()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn export_timing_keeps_duration_when_rate_changes_and_rejects_invalid_ranges() {
    let mut doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    doc.frame_count = 48;
    let mut s = Settings::for_document(&doc);
    s.start = 24;
    s.end = 47;
    s.frame_rate = Time::new(30000, 1001).unwrap();
    let t = s.times(&doc).unwrap();
    assert_eq!(t.len(), 30);
    assert_eq!(t[0].checked_sub(Time::new(1, 1).unwrap()).unwrap().ticks, 0);
    assert_eq!(
        t[29]
            .checked_sub(Time::new(59029, 30000).unwrap())
            .unwrap()
            .ticks,
        0
    );
    s.width = 321;
    assert!(s.times(&doc).is_err());
    s.width = 320;
    s.end = 48;
    assert!(s.times(&doc).is_err());
    s.end = 47;
    s.frame_rate = Time::new(241, 1).unwrap();
    assert!(s.times(&doc).is_err());
}
#[test]
#[ignore = "requires Vulkan, NVIDIA NVENC and FFmpeg"]
fn gpu_planes_and_nvenc_mp4_are_correct_and_cancellation_is_atomic() {
    let dir = Scratch::new();
    let renderer = Renderer::new().unwrap();
    let mut doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    doc.frame_count = 8;
    let inputs = ImageInputs::default();
    // GPU transfer/limited-range/bit placement: transparent over white and black.
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    for ten in [false, true] {
        for (matte, y) in [
            ([0.; 3], if ten { 64u16 << 6 } else { 16 }),
            ([1.; 3], if ten { 940u16 << 6 } else { 235 }),
        ] {
            let bytes = renderer
                .encode_video_planes(&frame, 322, 182, ten, matte)
                .unwrap();
            if ten {
                let samples = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .collect::<Vec<_>>();
                assert!(samples[..322 * 182].iter().all(|v| *v == y));
                assert!(samples[322 * 182..].iter().all(|v| *v == 512 << 6));
            } else {
                assert!(bytes[..322 * 182].iter().all(|v| *v == y as u8));
                assert!(bytes[322 * 182..].iter().all(|v| *v == 128));
            }
        }
    }
    // Mid-grey distinguishes Rec.709 transfer from sRGB; red checks matrix/chroma.
    for (matte, expected) in [
        ([0.18; 3], [106u8, 128, 128]),
        ([1., 0., 0.], [63, 102, 240]),
    ] {
        let bytes = renderer
            .encode_video_planes(&frame, 320, 180, false, matte)
            .unwrap();
        assert!(
            bytes[..320 * 180]
                .iter()
                .all(|v| v.abs_diff(expected[0]) <= 1)
        );
        assert!(
            bytes[320 * 180..]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|v| v[0].abs_diff(expected[1]) <= 1 && v[1].abs_diff(expected[2]) <= 1)
        );
    }
    // P010 at 4K spans more than Vulkan's minimum 65535 workgroups in X.
    let bytes = renderer
        .encode_video_planes(&frame, 3840, 2160, true, [1.; 3])
        .unwrap();
    assert_eq!(bytes.len(), 3840 * 2160 * 3);
    assert!(
        bytes[..3840 * 2160 * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .all(|p| u16::from_le_bytes([p[0], p[1]]) == 940 << 6)
    );
    drop(frame);
    // White opaque composition catches wrong transfer/range in actual MP4 too.
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Solid {
        color: [1., 1., 1., 1.],
    };
    for codec in [Codec::H264, Codec::Hevc10] {
        let mut settings = Settings::for_document(&doc);
        settings.codec = codec;
        settings.frame_rate = Time::new(30, 1).unwrap();
        settings.width = 640;
        settings.height = 360;
        let output = dir.0.join(format!("{codec:?}.mp4"));
        let mut observed = false;
        let result = video_export::render(&renderer, &doc, &inputs, &output, &settings, |p| {
            observed |= p.backend == "NVIDIA NVENC";
            true
        })
        .unwrap();
        assert!(!result.cancelled && result.hardware && observed);
        assert!(!result.host_staging, "{:?}", result.interop_fallback);
        assert_eq!(result.direct_gpu_transfers, 10);
        assert_eq!(result.staged_frames, 0);
        assert!(result.interop_fallback.is_none());
        assert_eq!(result.frames, 10);
        assert_eq!(result.backend, "NVIDIA NVENC");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let info = &info["streams"][0];
        assert_eq!(info["width"], 640);
        assert_eq!(info["height"], 360);
        assert_eq!(info["nb_frames"], "10");
        assert_eq!(info["avg_frame_rate"], "30/1");
        assert_eq!(info["color_transfer"], "bt709");
        assert_eq!(info["color_range"], "tv");
        assert_eq!(
            info["pix_fmt"],
            if codec == Codec::H264 {
                "yuv420p"
            } else {
                "yuv420p10le"
            }
        );
        let decode = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&output)
            .args([
                "-frames:v",
                "1",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(decode.status.success());
        assert!(decode.stdout.iter().all(|v| *v >= 250));
        let original = std::fs::read(&output).unwrap();
        assert!(
            video_export::render(&renderer, &doc, &inputs, &output, &settings, |_| true).is_err()
        );
        assert_eq!(std::fs::read(&output).unwrap(), original);
    }
    let settings = Settings::for_document(&doc);
    let output = dir.0.join("cancelled.mp4");
    let before = std::fs::read_dir(&dir.0).unwrap().count();
    let cancelled = video_export::render(&renderer, &doc, &inputs, &output, &settings, |p| {
        p.completed < 2
    })
    .unwrap();
    assert!(cancelled.cancelled);
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), before);
    let cancelled = video_export::render(&renderer, &doc, &inputs, &output, &settings, |p| {
        p.phase != "Finalizing MP4"
    })
    .unwrap();
    assert!(cancelled.cancelled);
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), before);
}

#[test]
#[ignore = "requires Vulkan, CUDA external memory and NVIDIA NVENC"]
fn direct_cuda_encoder_planes_match_staged_planes_exactly() {
    let dir = Scratch::new();
    let renderer = Renderer::new().unwrap();
    let doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let inputs = ImageInputs::default();
    let plan = Plan::compile(&doc).unwrap();
    let gpu = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    for (width, height) in [(322, 182), (3840, 2160)] {
        for ten_bit in [false, true] {
            let path = dir.0.join(format!("native-{width}-{ten_bit}.mp4"));
            let mut encoder = veylvik_decode::encode::Encoder::open(
                &path,
                veylvik_decode::encode::Settings {
                    width,
                    height,
                    fps: [30000, 1001],
                    ten_bit,
                    quality: 20,
                    fast: false,
                },
            )
            .unwrap();
            assert!(encoder.send(encoder.frame().unwrap()).is_err());
            for n in 0..8 {
                let matte = [n as f32 / 8., 0.18, 0.75];
                let mut frame = encoder.frame().unwrap();
                assert!(frame.stage().is_err());
                assert!(frame.fill(|_| Ok(())).is_err());
                frame
                    .fill(|destination| Ok(renderer.encode_video_cuda(&gpu, matte, destination)?))
                    .unwrap();
                let diagnostic = frame.stage().unwrap();
                let staged = renderer
                    .encode_video_planes(&gpu, width, height, ten_bit, matte)
                    .unwrap();
                assert_eq!(
                    diagnostic, staged,
                    "{width}x{height} ten_bit={ten_bit} frame={n}"
                );
                encoder.send(frame).unwrap();
            }
            encoder.finish().unwrap();
            let probe = Command::new("ffprobe")
                .args(["-v", "error", "-show_streams", "-of", "json"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(probe.status.success());
            let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            assert_eq!(info["streams"][0]["nb_frames"], "8");
            assert_eq!(info["streams"][0]["avg_frame_rate"], "30000/1001");
            assert_eq!(info["streams"][0]["color_transfer"], "bt709");
            assert_eq!(
                info["streams"][0]["pix_fmt"],
                if ten_bit { "yuv420p10le" } else { "yuv420p" }
            );
        }
    }
}

#[test]
#[ignore = "requires Vulkan and NVIDIA NVENC"]
fn direct_bridge_budget_fallback_is_explicit_and_renderer_recovers() {
    let dir = Scratch::new();
    let mut renderer = Renderer::new().unwrap();
    let mut doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    doc.frame_count = 1;
    let inputs = ImageInputs::default();
    let plan = Plan::compile(&doc).unwrap();
    let gpu = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let original_budget = renderer.budget_bytes;
    renderer.budget_bytes =
        renderer.resource_stats().unwrap().held_bytes + 320 * 180 * 3 + 48 + 1024;
    let settings = Settings::for_document(&doc);
    let fallback = video_export::render(
        &renderer,
        &doc,
        &inputs,
        &dir.0.join("staged.mp4"),
        &settings,
        |_| true,
    )
    .unwrap();
    assert!(fallback.host_staging && fallback.hardware);
    assert_eq!(fallback.staged_frames, 1);
    assert_eq!(fallback.direct_gpu_transfers, 0);
    assert!(fallback.interop_fallback.unwrap().contains("budget"));
    renderer.budget_bytes = original_budget;
    let direct = video_export::render(
        &renderer,
        &doc,
        &inputs,
        &dir.0.join("direct.mp4"),
        &settings,
        |_| true,
    )
    .unwrap();
    assert!(!direct.host_staging);
    assert_eq!(direct.direct_gpu_transfers, 1);
    drop(gpu);
}
