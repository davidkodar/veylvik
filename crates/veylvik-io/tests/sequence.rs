use std::{ops::Deref, path::PathBuf};
use veylvik_document::{NodeId, Time, sequence::FrameRange};
use veylvik_io::sequence::{SequenceRequest, render_sequence};
use veylvik_render::Renderer;
struct Scratch(PathBuf);
impl Deref for Scratch {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn sequence_cancellation_collision_and_frame_parity() {
    let root = Scratch(std::env::temp_dir().join(format!("veylvik-sequence-{}", NodeId::new_v4())));
    std::fs::create_dir(&root.0).unwrap();
    let renderer = Renderer::new().unwrap();
    let mut doc = veylvik_document::demo();
    doc.width = 13;
    doc.height = 7;
    let request = SequenceRequest {
        frames: FrameRange::new(-1, 1, Time::new(30000, 1001).unwrap()).unwrap(),
        output: doc.output,
        region: None,
    };
    let directory = root.join("cancelled");
    let result = render_sequence(&renderer, &doc, &root, &directory, request, |done, _| {
        done < 1
    })
    .unwrap();
    assert_eq!(result["manifest"]["status"], "cancelled");
    assert_eq!(result["manifest"]["completed"], 1);
    assert!(directory.join("frame-000000.png").is_file());
    assert!(!directory.join("frame-000001.png").exists());
    let manifest = std::fs::read(directory.join("sequence.json")).unwrap();
    assert!(render_sequence(&renderer, &doc, &root, &directory, request, |_, _| true).is_err());
    assert_eq!(
        std::fs::read(directory.join("sequence.json")).unwrap(),
        manifest
    );
    let directory = root.join("complete");
    let mut progress = Vec::new();
    let result = render_sequence(
        &renderer,
        &doc,
        &root,
        &directory,
        request,
        |done, total| {
            progress.push((done, total));
            true
        },
    )
    .unwrap();
    assert_eq!(progress, [(0, 3), (1, 3), (2, 3), (3, 3)]);
    assert_eq!(result["manifest"]["status"], "complete");
    assert_eq!(result["manifest"]["completed"], 3);
    assert_eq!(
        std::fs::read(directory.join("frame-000000.png")).unwrap(),
        std::fs::read(root.join("cancelled/frame-000000.png")).unwrap()
    );
    // Collision introduced after reservation must never replace the file.
    let directory = root.join("collision");
    assert!(
        render_sequence(&renderer, &doc, &root, &directory, request, |done, _| {
            if done == 0 {
                std::fs::write(directory.join("frame-000000.png"), b"original").unwrap();
            }
            true
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read(directory.join("frame-000000.png")).unwrap(),
        b"original"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("sequence.json")).unwrap()).unwrap();
    assert_eq!(manifest["status"], "rendering");
}
