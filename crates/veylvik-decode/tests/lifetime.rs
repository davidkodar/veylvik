use std::{path::PathBuf, process::Command};
use veylvik_decode::{Decoder, DeviceKind, PlaneFormat};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (Scratch, PathBuf) {
    let dir = Scratch(std::env::temp_dir().join(format!(
            "veylvik-native-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&dir.0).unwrap();
    let path = dir.0.join("shot.mkv");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=25:duration=1",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-g",
                "10",
                "-bf",
                "3",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    (dir, path)
}
fn open(path: &std::path::Path) -> Decoder {
    Decoder::open(
        path,
        DeviceKind::Cuda,
        Some("h264_cuvid"),
        [320, 180],
        PlaneFormat::Nv12,
    )
    .unwrap()
}
#[test]
fn invalid_dimensions_are_rejected_before_opening_a_device() {
    for size in [[0, 180], [320, 181], [8194, 180]] {
        assert!(
            Decoder::open(
                std::path::Path::new("missing"),
                DeviceKind::Cuda,
                Some("h264_cuvid"),
                size,
                PlaneFormat::Nv12
            )
            .is_err()
        );
    }
}
#[test]
#[ignore = "requires NVIDIA NVDEC and FFmpeg libx264"]
fn frames_outlive_decoders_and_parallel_clips_share_one_device() {
    let (_dir, path) = fixture();
    let mut anchor = open(&path);
    let id = anchor.device_id();
    let held = anchor.frame_at(0).unwrap();
    let first = held.stage().unwrap();
    assert_eq!(first.len(), 320 * 180 * 3 / 2);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let first = first.clone();
            std::thread::spawn(move || {
                let mut decoder = open(&path);
                assert_eq!(decoder.device_id(), id);
                assert_eq!(decoder.frame_at(0).unwrap().stage().unwrap(), first);
                decoder.seek(800).unwrap();
                let late = decoder.frame_at(800).unwrap();
                assert_ne!(late.stage().unwrap(), first);
                decoder.seek(0).unwrap();
                assert_eq!(decoder.frame_at(0).unwrap().stage().unwrap(), first);
                assert_eq!(decoder.device_id(), id);
                assert_eq!(decoder.seeks, 2);
                drop(decoder);
                late.stage().unwrap()
            })
        })
        .collect();
    let late: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(late.windows(2).all(|w| w[0] == w[1]));
    // Held AVFrame owns both hardware pool and device after its decoder closes.
    drop(anchor);
    assert_eq!(held.stage().unwrap(), first);
    let mut reopened = open(&path);
    assert_eq!(reopened.device_id(), id);
    assert_eq!(reopened.frame_at(0).unwrap().stage().unwrap(), first);
    // A missing timestamp must error, rather than display a neighbouring frame.
    reopened.seek(1).unwrap();
    assert!(reopened.frame_at(1).is_err());
}

#[test]
#[ignore = "requires NVIDIA NVDEC and FFmpeg libx264"]
fn variable_rate_and_nonzero_start_seek_to_exact_presentation_times() {
    let (dir, _) = fixture();
    let path = dir.0.join("vfr.mkv");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=25:duration=0.48",
                "-vf",
                "settb=1/1000,setpts=N*40+gte(N\\,5)*20",
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
                "1/1000",
                "-output_ts_offset",
                "2",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-bf",
                "3",
                "-g",
                "6",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let timestamps: Vec<_> = (0..12)
        .map(|n| 2000 + n * 40 + if n >= 5 { 20 } else { 0 })
        .collect();
    let mut decoder = open(&path);
    let frames: Vec<_> = timestamps
        .iter()
        .map(|&pts| decoder.frame_at(pts).unwrap().stage().unwrap())
        .collect();
    for n in [11, 0, 5, 2, 9, 4] {
        decoder.seek(timestamps[n]).unwrap();
        assert_eq!(
            decoder.frame_at(timestamps[n]).unwrap().stage().unwrap(),
            frames[n],
            "frame {n}"
        );
    }
}

#[test]
#[ignore = "requires NVIDIA NVDEC, Vulkan and FFmpeg libx264/libx265"]
fn direct_handoff_preserves_padded_planes_and_survives_decoder_drop() {
    use std::sync::Arc;
    use veylvik_render::{Renderer, VideoEncoding, VideoFormat};
    let (dir, _) = fixture();
    let mut renderer = Renderer::new().unwrap();
    for (codec, decoder, pixel, format, encoding_format) in [
        (
            "libx264",
            "h264_cuvid",
            "yuv420p",
            PlaneFormat::Nv12,
            VideoFormat::Nv12,
        ),
        (
            "libx265",
            "hevc_cuvid",
            "yuv420p10le",
            PlaneFormat::P010,
            VideoFormat::P010,
        ),
    ] {
        let path = dir.0.join(format!("padded-{codec}.mkv"));
        let mut command = Command::new("ffmpeg");
        command.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=322x182:rate=25:duration=0.2",
            "-c:v",
            codec,
            "-threads",
            "2",
            "-pix_fmt",
            pixel,
        ]);
        if codec == "libx265" {
            command.args(["-x265-params", "log-level=error:pools=2:frame-threads=2"]);
        }
        assert!(command.arg(&path).status().unwrap().success());
        let mut decoder =
            Decoder::open(&path, DeviceKind::Cuda, Some(decoder), [322, 182], format).unwrap();
        let held = Arc::new(decoder.frame_at(0).unwrap());
        let cuda = held.cuda().unwrap().unwrap();
        let staged = held.stage().unwrap();
        drop(decoder);
        drop(held);
        let lut: Vec<_> = (0..65536).map(|n| n as f32 / 65535.0).collect();
        let encoding = VideoEncoding {
            width: 322,
            height: 182,
            format: encoding_format,
            rotation: 0,
            kr: 0.2126,
            kb: 0.0722,
            full_range: false,
            chroma_location: [0., 0.5],
            primaries: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        };
        let expected = renderer.import_video(&staged, encoding, &lut).unwrap();
        let expected = renderer.read_frame(&expected).unwrap().pixels;
        // Repeated allocations/imports exercise FD, context and ownership cleanup.
        for _ in 0..32 {
            let frame = renderer.import_cuda_video(&cuda, encoding, &lut).unwrap();
            assert_eq!(renderer.read_frame(&frame).unwrap().pixels, expected);
        }
        assert!(
            cuda.into_vulkan(renderer.device(), renderer.queue(), 1)
                .is_err()
        );
        let budget = renderer.budget_bytes;
        renderer.budget_bytes = 1;
        assert!(renderer.import_cuda_video(&cuda, encoding, &lut).is_err());
        renderer.budget_bytes = budget;
        assert!(renderer.import_cuda_video(&cuda, encoding, &lut).is_ok());
        let mut wrong = encoding;
        wrong.width = 320;
        assert!(renderer.import_cuda_video(&cuda, wrong, &lut).is_err());
    }
}
