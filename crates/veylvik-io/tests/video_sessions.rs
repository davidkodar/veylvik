//! Own test binary: the open-decoder limit is process-wide, so no other test may race it.
use std::{path::PathBuf, process::Command};
use veylvik_document::Time;
use veylvik_io::video::{DecodePolicy, Video};
use veylvik_render::FrameSource;

#[test]
#[ignore = "requires FFmpeg with libx264"]
fn idle_decoders_close_beyond_the_limit_and_reopen_on_demand() {
    let dir = std::env::temp_dir().join(format!(
        "veylvik-sessions-{}",
        veylvik_document::NodeId::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path: PathBuf = dir.join("clip.mp4");
    assert!(
        Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i"])
            .args(["testsrc2=size=64x32:rate=24:duration=1", "-c:v", "libx264"])
            .args(["-pix_fmt", "yuv420p"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let clips: Vec<Video> = (0..16)
        .map(|_| Video::open_with_policy(&path, 1024 * 1024, DecodePolicy::Software).unwrap())
        .collect();
    assert_eq!(veylvik_io::video::open_decoders(), 12);
    // The first clip's decoder was closed while idle; it reopens and decodes exactly.
    let reopened = clips[0].frame(Time::new(10, 24).unwrap()).unwrap();
    let reference = clips[15].frame(Time::new(10, 24).unwrap()).unwrap();
    assert_eq!(reopened.pixels(), reference.pixels());
    assert_eq!(veylvik_io::video::open_decoders(), 12);
    drop(clips);
    assert_eq!(veylvik_io::video::open_decoders(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}
