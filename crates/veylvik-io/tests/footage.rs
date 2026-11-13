use std::path::PathBuf;
use veylvik_document::{Operation, Time};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "veylvik-sequence-test-{}",
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
fn source() -> (Scratch, PathBuf, veylvik_io::footage::Media) {
    let dir = Scratch::new();
    for (i, color) in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]]
        .into_iter()
        .enumerate()
    {
        veylvik_io::save_png(&dir.0.join(format!("shot{i:04}.png")), 1, 1, &[color]).unwrap();
    }
    let (path, media) = veylvik_io::footage::import_numbered(
        &dir.0.join("shot0000.png"),
        Time::new(24, 1).unwrap(),
        32,
    )
    .unwrap();
    (dir, path, media)
}
#[test]
fn numbered_import_is_reusable_and_protects_every_frame() {
    let (dir, path, _) = source();
    assert!(
        veylvik_io::footage::import_numbered(
            &dir.0.join("shot0000.png"),
            Time::new(24, 1).unwrap(),
            32
        )
        .is_ok()
    );
    let before = std::fs::read(&path).unwrap();
    assert!(
        veylvik_io::footage::import_numbered(
            &dir.0.join("shot0000.png"),
            Time::new(30, 1).unwrap(),
            32
        )
        .is_err()
    );
    assert_eq!(before, std::fs::read(&path).unwrap());
    let mut doc = veylvik_document::blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: path.to_str().unwrap().into(),
        });
    for n in 0..2 {
        assert!(
            veylvik_io::protect_inputs(&doc, &dir.0, &dir.0.join(format!("shot{n:04}.png")))
                .is_err()
        );
    }
    assert!(veylvik_io::save(&doc, &dir.0, &dir.0.join("saved.veylvik")).is_ok());
}
#[test]
#[ignore = "requires Vulkan"]
fn sequence_frames_invalidate_gpu_cache_at_exact_frame_boundaries() {
    let (_dir, path, media) = source();
    let mut doc = veylvik_document::blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
    let key = path.to_str().unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image { asset: key.into() });
    let mut inputs = ImageInputs::default();
    media.insert(&mut inputs, key);
    let plan = Plan::compile(&doc).unwrap();
    let renderer = Renderer::new().unwrap();
    let at = |time| {
        renderer
            .render_gpu_cached(&plan, RenderRequest::full(time), &inputs)
            .unwrap()
    };
    let first = at(Time::new(0, 1).unwrap());
    let same = at(Time::new(1, 48).unwrap());
    assert!(std::sync::Arc::ptr_eq(&first, &same));
    let second = at(Time::new(1, 24).unwrap());
    assert_eq!(
        renderer.read_frame(&first).unwrap().pixels[0],
        [1.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(
        renderer.read_frame(&second).unwrap().pixels[0],
        [0.0, 1.0, 0.0, 1.0]
    );
    let held = at(Time::new(5, 1).unwrap());
    assert!(std::sync::Arc::ptr_eq(&second, &held));
}
