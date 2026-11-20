use std::process::Command;
use veylvik_document::{Node, Operation, Time, nodes};

#[test]
#[ignore = "requires FFmpeg with libx264 and aac"]
fn preview_mix_places_footage_sound_on_the_timeline() {
    let dir = std::env::temp_dir().join(format!(
        "veylvik-audio-{}",
        veylvik_document::NodeId::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    let clip = dir.join("tone.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x32:rate=24:duration=3"
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=3"
            ])
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest"
            ])
            .arg(&clip)
            .status()
            .unwrap()
            .success()
    );
    let mut doc = veylvik_document::blank(64, 32, Time::new(24, 1).unwrap()).unwrap();
    let footage = Node::new(
        "Tone",
        Operation::Footage(nodes::Footage {
            asset: clip.to_str().unwrap().into(),
            timing: veylvik_document::media::Timing {
                start: Time::new(1, 2).unwrap(),
                source_in: Time::new(1, 5).unwrap(),
                source_out: None,
            },
            placement: Default::default(),
            colour: Default::default(),
            layer: None,
            fallback: String::new(),
        }),
        &[],
    );
    doc.output = footage.id;
    doc.nodes.insert(footage.id, footage);
    let pcm = veylvik_audio::mix(&doc, &dir, [0.0, 2.0], 44_100, || true)
        .unwrap()
        .expect("the clip has sound");
    assert_eq!(pcm.rate, 44_100);
    assert_eq!(
        pcm.frames(),
        88_200,
        "exactly the window, at the device rate"
    );
    let peak = |from: f64, to: f64| {
        pcm.samples[(from * 44_100.) as usize * 2..(to * 44_100.) as usize * 2]
            .iter()
            .fold(0f32, |m, s| m.max(s.abs()))
    };
    assert!(peak(0.0, 0.45) < 1e-4, "silent before the clip starts");
    assert!(peak(0.6, 2.0) > 0.05, "the tone plays from 0.5 s");
    // Nothing audible in the window: no mix at all.
    assert!(
        veylvik_audio::mix(&doc, &dir, [10.0, 12.0], 44_100, || true)
            .unwrap()
            .is_none()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
