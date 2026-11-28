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
    for format in [veylvik_io::FrameFormat::Png, veylvik_io::FrameFormat::Exr] {
        check_sequence(format);
    }
}
fn check_sequence(format: veylvik_io::FrameFormat) {
    let first = format!("frame-000000.{}", format.extension());
    let second = format!("frame-000001.{}", format.extension());
    let root = Scratch(std::env::temp_dir().join(format!("veylvik-sequence-{}", NodeId::new_v4())));
    std::fs::create_dir(&root.0).unwrap();
    let renderer = Renderer::new().unwrap();
    let mut doc = veylvik_document::demo();
    doc.width = 13;
    doc.height = 7;
    let request = SequenceRequest {
        format,
        frames: FrameRange::new(-1, 1, Time::new(30000, 1001).unwrap()).unwrap(),
        output: doc.output,
        region: None,
        layers: &[],
    };
    let directory = root.join("cancelled");
    let result = render_sequence(&renderer, &doc, &root, &directory, request, |done, _| {
        done < 1
    })
    .unwrap();
    assert_eq!(result["manifest"]["format"], format.extension());
    assert_eq!(
        result["manifest"]["alpha"],
        if format == veylvik_io::FrameFormat::Png {
            "straight"
        } else {
            "premultiplied"
        }
    );
    assert_eq!(result["manifest"]["status"], "cancelled");
    assert_eq!(result["manifest"]["completed"], 1);
    assert!(directory.join(&first).is_file());
    assert!(!directory.join(&second).exists());
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
        std::fs::read(directory.join(&first)).unwrap(),
        std::fs::read(root.join("cancelled").join(&first)).unwrap()
    );
    // Collision introduced after reservation must never replace the file.
    let directory = root.join("collision");
    assert!(
        render_sequence(&renderer, &doc, &root, &directory, request, |done, _| {
            if done == 0 {
                std::fs::write(directory.join(&first), b"original").unwrap();
            }
            true
        })
        .is_err()
    );
    assert_eq!(std::fs::read(directory.join(&first)).unwrap(), b"original");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("sequence.json")).unwrap()).unwrap();
    assert_eq!(manifest["status"], "rendering");
    // A GUI export uses its captured pixels even if no source file exists.
    let node = veylvik_document::Node::new(
        "snapshot",
        veylvik_document::Operation::Image(veylvik_document::nodes::Image {
            asset: "absent.png".into(),
        }),
        &[],
    );
    doc.output = node.id;
    doc.nodes = [(node.id, node)].into_iter().collect();
    let request = SequenceRequest {
        format: veylvik_io::FrameFormat::Png,
        frames: FrameRange::new(0, 0, doc.frame_rate).unwrap(),
        output: doc.output,
        region: None,
        layers: &[],
    };
    let mut inputs = veylvik_render::ImageInputs::default();
    let missing = root.join("missing");
    assert!(
        veylvik_io::sequence::render_sequence_with_inputs(
            &renderer,
            &doc,
            &inputs,
            &missing,
            request,
            |_, _| true
        )
        .is_err()
    );
    assert!(!missing.exists());
    inputs.insert(
        "absent.png",
        std::sync::Arc::new(veylvik_render::Image::new(1, 1, vec![[1.0, 0.0, 0.0, 1.0]]).unwrap()),
    );
    let result = veylvik_io::sequence::render_sequence_with_inputs(
        &renderer,
        &doc,
        &inputs,
        &root.join("snapshot"),
        request,
        |_, _| true,
    )
    .unwrap();
    assert_eq!(result["manifest"]["status"], "complete");
    let image =
        veylvik_io::load_png(&root.join("snapshot/frame-000000.png"), 128 * 1024 * 1024).unwrap();
    assert_eq!(image.pixels()[0], [1.0, 0.0, 0.0, 1.0]);
}
