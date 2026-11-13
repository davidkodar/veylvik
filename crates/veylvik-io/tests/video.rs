use std::{path::PathBuf, process::Command, sync::Arc};
use veylvik_document::Time;
use veylvik_render::FrameSource;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "veylvik-video-{}",
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
#[ignore = "requires FFmpeg with libx264 and zscale"]
fn video_seeking_matches_sequential_decode_with_b_frames_and_nonzero_start() {
    let dir = Scratch::new();
    let path = dir.0.join("shot.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x32:rate=24:duration=1",
                "-c:v",
                "libx264",
                "-bf",
                "3",
                "-g",
                "12",
                "-pix_fmt",
                "yuv420p",
                "-output_ts_offset",
                "2",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-colorspace",
                "bt709"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let video = veylvik_io::video::Video::open(&path, 1024 * 1024).unwrap();
    let info = video.info().unwrap();
    assert_eq!(info.frames, Some(24));
    assert_eq!(info.width, 64);
    let frames: Vec<_> = (0..24)
        .map(|n| video.frame(Time::new(n, 24).unwrap()).unwrap())
        .collect();
    for n in [0, 23, 1, 5, 12, 7, 22, 3] {
        let sought = video.frame(Time::new(n, 24).unwrap()).unwrap();
        assert_eq!(sought.pixels(), frames[n as usize].pixels(), "frame {n}");
    }
    let last = video.frame(Time::new(23, 24).unwrap()).unwrap();
    let held = video.frame(Time::new(99, 1).unwrap()).unwrap();
    assert!(Arc::ptr_eq(&last, &held));
    std::fs::write(&path, b"changed").unwrap();
    assert!(video.frame(Time::new(0, 1).unwrap()).is_err());
}
#[test]
#[ignore = "requires FFmpeg with FFV1 and zscale"]
fn variable_rate_uses_presentation_timestamps_not_average_frame_rate() {
    let dir = Scratch::new();
    let path = dir.0.join("variable.mkv");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x32:rate=25:duration=0.48",
                "-vf",
                "settb=1/1000,setpts=N*40+gte(N\\,5)*20",
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
                "1/1000",
                "-c:v",
                "ffv1"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let video = veylvik_io::video::Video::open(&path, 1024 * 1024).unwrap();
    let a = video.frame(Time::new(160, 1000).unwrap()).unwrap();
    let held = video.frame(Time::new(200, 1000).unwrap()).unwrap();
    assert!(Arc::ptr_eq(&a, &held));
    let b = video.frame(Time::new(220, 1000).unwrap()).unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    let seq: Vec<_> = (0..12)
        .map(|n| {
            video
                .frame(Time::new(n * 40 + if n >= 5 { 20 } else { 0 }, 1000).unwrap())
                .unwrap()
        })
        .collect();
    for n in [10, 0, 6, 3, 11] {
        let time = Time::new(n * 40 + if n >= 5 { 20 } else { 0 }, 1000).unwrap();
        assert_eq!(
            video.frame(time).unwrap().pixels(),
            seq[n as usize].pixels()
        );
    }
}

#[test]
#[ignore = "requires FFmpeg and Vulkan"]
fn footage_timing_shares_one_source_without_cross_contaminating_clips() {
    use veylvik_document::{Node, Operation, media::Timing};
    use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
    let dir = Scratch::new();
    let path = dir.0.join("shot.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x32:rate=24:duration=1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let mut doc = veylvik_document::blank(64, 32, Time::new(24, 1).unwrap()).unwrap();
    let first = Node::new(
        "First",
        Operation::Footage(veylvik_document::nodes::Footage {
            placement: Default::default(),
            colour: Default::default(),
            asset: path.to_str().unwrap().into(),
            timing: Timing {
                start: Time::new(1, 1).unwrap(),
                source_in: Time::new(1, 2).unwrap(),
                source_out: Some(Time::new(3, 4).unwrap()),
            },
        }),
        &[],
    );
    let mut second = Node::new("Second", first.operation.clone(), &[]);
    if let Operation::Footage(veylvik_document::nodes::Footage { timing, .. }) =
        &mut second.operation
    {
        timing.start = Time::new(0, 1).unwrap();
        timing.source_in = Time::new(0, 1).unwrap();
    }
    doc.output = first.id;
    doc.nodes.insert(first.id, first.clone());
    doc.nodes.insert(second.id, second.clone());
    let merge = Node::new(
        "Both clips",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", first.id), ("foreground", second.id)],
    );
    let merged = merge.id;
    doc.nodes.insert(merged, merge);
    let renderer = Renderer::new().unwrap();
    let mut inputs = ImageInputs::default();
    veylvik_io::footage::load(&path, 1024 * 1024)
        .unwrap()
        .insert(&mut inputs, path.to_str().unwrap());
    let render = |id, time| {
        renderer
            .render_gpu_cached(
                &Plan::compile_output(&doc, id).unwrap(),
                RenderRequest::full(time),
                &inputs,
            )
            .unwrap()
    };
    let before = render(first.id, Time::new(0, 1).unwrap());
    assert!(
        renderer
            .read_frame(&before)
            .unwrap()
            .pixels
            .iter()
            .all(|p| p[3] == 0.0)
    );
    let first_frame = render(first.id, Time::new(1, 1).unwrap());
    let equivalent = render(second.id, Time::new(1, 2).unwrap());
    assert_eq!(
        renderer.read_frame(&first_frame).unwrap().pixels,
        renderer.read_frame(&equivalent).unwrap().pixels
    );
    assert_eq!(
        renderer
            .read_frame(&render(merged, Time::new(1, 1).unwrap()))
            .unwrap()
            .pixels,
        renderer.read_frame(&first_frame).unwrap().pixels
    );
    assert_eq!(
        renderer
            .read_frame(&render(merged, Time::new(0, 1).unwrap()))
            .unwrap()
            .pixels,
        renderer
            .read_frame(&render(second.id, Time::new(0, 1).unwrap()))
            .unwrap()
            .pixels
    );
    let after = render(first.id, Time::new(5, 4).unwrap());
    assert!(
        renderer
            .read_frame(&after)
            .unwrap()
            .pixels
            .iter()
            .all(|p| p[3] == 0.0)
    );
    let saved = dir.0.join("saved.veylvik");
    veylvik_io::save(&doc, &dir.0, &saved).unwrap();
    let restored = veylvik_document::Composition::load(&saved).unwrap();
    assert!(matches!(
        restored.nodes[&first.id].operation,
        Operation::Footage(veylvik_document::nodes::Footage { .. })
    ));
    assert!(veylvik_io::protect_inputs(&doc, &dir.0, &path).is_err());
    let recovery = veylvik_io::collect::collect(&doc, &dir.0, &dir.0.join("collected")).unwrap();
    let collected = veylvik_document::Composition::load(&recovery.project).unwrap();
    assert_eq!(
        collected.nodes[&first.id].operation.asset(),
        collected.nodes[&second.id].operation.asset()
    );
    let plan = Plan::compile(&collected).unwrap();
    let loaded =
        veylvik_io::load_inputs(&plan, recovery.project.parent().unwrap(), 1024 * 1024).unwrap();
    let frame = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &loaded,
        )
        .unwrap();
    assert_eq!(
        frame.pixels,
        renderer.read_frame(&first_frame).unwrap().pixels
    );
}

#[test]
#[ignore = "requires FFmpeg with FFV1 and zscale"]
fn video_colour_is_linear_and_alpha_is_premultiplied() {
    let dir = Scratch::new();
    let png = dir.0.join("source.png");
    let path = dir.0.join("alpha.mkv");
    let pixel = [0.10702057, 0.10702057, 0.10702057, 0.5];
    veylvik_io::save_png(&png, 16, 16, &vec![pixel; 256]).unwrap();
    assert!(
        Command::new("ffmpeg")
            .args(["-v", "error", "-loop", "1", "-i"])
            .arg(&png)
            .args([
                "-frames:v",
                "2",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "bgra",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "iec61966-2-1",
                "-colorspace",
                "rgb",
                "-color_range",
                "pc"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let video = veylvik_io::video::Video::open(&path, 1024 * 1024).unwrap();
    let frame = video.frame(Time::new(0, 1).unwrap()).unwrap();
    let p = frame.pixels()[0];
    assert!((p[3] - 0.5).abs() < 0.01, "alpha {:?}", p);
    assert!((p[0] - 0.107).abs() < 0.01, "linear premultiplied {:?}", p);
}

#[test]
#[ignore = "requires NVIDIA NVDEC, Vulkan and FFmpeg libx264/libx265"]
fn hardware_frames_stay_linear_on_gpu_and_seek_in_8_and_10_bit() {
    use veylvik_io::video::{DecodePolicy, Video};
    use veylvik_render::Renderer;
    let dir = Scratch::new();
    let renderer = Renderer::new().unwrap();
    for (codec, pix, extension) in [
        ("libx264", "yuv420p", "mp4"),
        ("libx265", "yuv420p10le", "mp4"),
    ] {
        let path = dir.0.join(format!("{codec}.{extension}"));
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24:duration=1",
            "-c:v",
            codec,
            "-pix_fmt",
            pix,
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-threads",
            "2",
        ]);
        if codec == "libx265" {
            cmd.args(["-x265-params", "log-level=error:pools=2:frame-threads=2"]);
        }
        assert!(cmd.arg(&path).status().unwrap().success());
        let hardware =
            Video::open_with_policy(&path, 8 * 1024 * 1024, DecodePolicy::HardwareRequired)
                .unwrap();
        let initial = hardware.diagnostics().unwrap();
        assert!(initial.hardware && initial.in_process && initial.host_staging);
        let device_id = initial.device_context_id.unwrap();
        assert_eq!(hardware.diagnostics().unwrap().cpu_realizations, 0);
        let hardware = Arc::new(hardware);
        let mut inputs = veylvik_render::ImageInputs::default();
        inputs.insert_source("clip", hardware.clone());
        let mut doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            veylvik_document::Operation::Footage(veylvik_document::nodes::Footage {
                asset: "clip".into(),
                timing: Default::default(),
                placement: Default::default(),
                colour: Default::default(),
            });
        renderer
            .render_gpu(
                &veylvik_render::Plan::compile(&doc).unwrap(),
                veylvik_render::RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        let first_transfer = hardware.diagnostics().unwrap();
        assert_eq!(first_transfer.cpu_realizations, 0);
        assert_eq!(first_transfer.staged_frames, 0);
        assert_eq!(first_transfer.direct_gpu_transfers, 1, "{first_transfer:?}");
        assert!(!first_transfer.host_staging);
        let software =
            Video::open_with_policy(&path, 8 * 1024 * 1024, DecodePolicy::Software).unwrap();
        let mut sequential = Vec::new();
        for n in 0..24 {
            let time = Time::new(n, 24).unwrap();
            let gpu = hardware.gpu_frame(time, &renderer).unwrap().unwrap();
            let pixels = renderer.read_frame(&gpu).unwrap().pixels;
            let reference = hardware.frame(time).unwrap();
            let difference = pixels
                .iter()
                .zip(reference.pixels())
                .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
                .fold(0.0f32, f32::max);
            assert!(
                difference < 0.001,
                "GPU versus CPU realization: {difference}"
            );
            // Independent FFmpeg software decode checks plane ordering/range/matrix.
            // Chroma reconstruction differs at edges; compare mean error.
            let sw = software.frame(time).unwrap();
            let mean = pixels
                .iter()
                .zip(sw.pixels())
                .flat_map(|(a, b)| a[..3].iter().zip(&b[..3]).map(|(a, b)| (a - b).abs()))
                .sum::<f32>()
                / (pixels.len() * 3) as f32;
            assert!(
                mean < 0.025,
                "hardware vs software mean colour error {mean}"
            );
            sequential.push(pixels);
        }
        let before = hardware.diagnostics().unwrap().cpu_realizations;
        for n in [0, 23, 1, 5, 12, 7, 22, 3] {
            let time = Time::new(n, 24).unwrap();
            let gpu = hardware.gpu_frame(time, &renderer).unwrap().unwrap();
            assert_eq!(
                renderer.read_frame(&gpu).unwrap().pixels,
                sequential[n as usize]
            );
            assert!(Arc::ptr_eq(
                &gpu,
                &hardware.gpu_frame(time, &renderer).unwrap().unwrap()
            ));
        }
        let after = hardware.diagnostics().unwrap();
        assert_eq!(after.cpu_realizations, before);
        assert_eq!(after.device_context_id, Some(device_id));
        assert!(
            after.seeks >= 4,
            "Native seeking should retain its decoder/device"
        );
        let other = Renderer::new().unwrap();
        let gpu = hardware
            .gpu_frame(Time::new(0, 1).unwrap(), &other)
            .unwrap()
            .unwrap();
        assert!(other.owns_frame(&gpu));
        assert!(!renderer.owns_frame(&gpu));
    }
}

#[test]
#[ignore = "requires NVDEC and Vulkan; UHD throughput diagnostic"]
fn uhd_decode_reports_real_backend_and_conversion_cost() {
    use veylvik_io::video::{DecodePolicy, Video};
    let dir = Scratch::new();
    let path = dir.0.join("uhd.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=3840x2160:rate=24:duration=1",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-threads",
                "2"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let renderer = veylvik_render::Renderer::new().unwrap();
    for policy in [DecodePolicy::HardwareRequired, DecodePolicy::Software] {
        let video = Video::open_with_policy(&path, 128 * 1024 * 1024, policy).unwrap();
        let started = std::time::Instant::now();
        for n in 0..24 {
            video
                .gpu_frame(Time::new(n, 24).unwrap(), &renderer)
                .unwrap();
        }
        let elapsed = started.elapsed();
        let diagnostics = video.diagnostics().unwrap();
        assert_eq!(diagnostics.cpu_realizations, 0);
        assert_eq!(diagnostics.gpu_conversions, 24);
        if policy == DecodePolicy::HardwareRequired {
            assert_eq!(diagnostics.direct_gpu_transfers, 24, "{diagnostics:?}");
            assert_eq!(diagnostics.staged_frames, 0);
            assert!(!diagnostics.host_staging);
        }
        eprintln!(
            "UHD {policy:?}: {elapsed:?}, {:.1} frames/s, {diagnostics:?}",
            24.0 / elapsed.as_secs_f64()
        );
    }
}

#[test]
#[ignore = "requires NVIDIA NVDEC, Vulkan and FFmpeg libx264"]
fn bridge_budget_failure_stages_hardware_and_recovers_on_later_frames() {
    use veylvik_io::video::{DecodePolicy, Video};
    let dir = Scratch::new();
    let path = dir.0.join("fallback.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=24:duration=0.2",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let source =
        Video::open_with_policy(&path, 8 * 1024 * 1024, DecodePolicy::HardwareRequired).unwrap();
    let mut renderer = veylvik_render::Renderer::new().unwrap();
    let budget = renderer.budget_bytes;
    // Fits the compact staged plane buffer but not the external allocation's
    // conservative 64 KiB reservation. Decoder must stay hardware-backed.
    renderer.budget_bytes = 320 * 180 * 16 + 320 * 180 * 3 / 2 + 262144 + 112;
    source
        .gpu_frame(Time::new(0, 24).unwrap(), &renderer)
        .unwrap()
        .unwrap();
    let fallback = source.diagnostics().unwrap();
    assert!(fallback.hardware && fallback.host_staging);
    assert_eq!(fallback.staged_frames, 1);
    assert_eq!(fallback.direct_gpu_transfers, 0);
    assert!(fallback.interop_fallback.contains("budget"), "{fallback:?}");
    renderer.budget_bytes = budget;
    source
        .gpu_frame(Time::new(1, 24).unwrap(), &renderer)
        .unwrap()
        .unwrap();
    let recovered = source.diagnostics().unwrap();
    assert!(recovered.hardware && !recovered.host_staging);
    assert_eq!(recovered.staged_frames, 1);
    assert_eq!(recovered.direct_gpu_transfers, 1);
    assert!(recovered.interop_fallback.is_empty());
}
