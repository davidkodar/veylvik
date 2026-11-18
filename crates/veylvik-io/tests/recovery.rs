use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use veylvik_document::*;
use veylvik_io::recovery::Store;
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn recovery_copies_preserve_assets_and_leave_originals_and_other_sessions_untouched() {
    let root = Scratch(std::env::temp_dir().join(format!("veylvik-recovery-{}", NodeId::new_v4())));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::new(root.0.join("recovery"));
    assert!(store.list().unwrap().is_empty());
    let id = NodeId::new_v4();
    let second = NodeId::new_v4();
    let node = Node::new(
        "image",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate.exr".into(),
        }),
        &[],
    );
    let mut doc = Composition {
        markers: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: 13,
        height: 9,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [(node.id, node)].into(),
    };
    let original = root.0.join("original.veylvik");
    doc.save(&original).unwrap();
    let original_bytes = std::fs::read(&original).unwrap();
    store.write(id, &doc, &root.0).unwrap();
    store.write(second, &doc, &root.0).unwrap();
    let recovered = store.read(id).unwrap();
    let Operation::Image(veylvik_document::nodes::Image { asset }) =
        &recovered.nodes[&recovered.output].operation
    else {
        unreachable!()
    };
    assert_eq!(std::path::Path::new(asset), root.0.join("plate.exr"));
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    assert_eq!(store.list().unwrap().len(), 2);
    let lease = store.claim(id).unwrap();
    assert!(!store.available(id).unwrap());
    assert!(store.claim(id).is_err());
    assert!(store.read(id).is_err());
    assert_eq!(store.list().unwrap().len(), 1);
    drop(lease);
    assert!(store.available(id).unwrap());
    assert_eq!(store.list().unwrap().len(), 2);
    let copy = store.directory().join(format!("{id}.veylvik"));
    assert_eq!(
        std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
        0o600
    );
    doc.width = 20;
    store.write(id, &doc, &root.0).unwrap();
    assert_eq!(store.read(id).unwrap().width, 20);
    assert_eq!(store.read(second).unwrap().width, 13);
    doc.width = 0;
    assert!(store.write(id, &doc, &root.0).is_err());
    assert_eq!(store.read(id).unwrap().width, 20);
    std::fs::write(store.directory().join("unrelated.txt"), b"keep").unwrap();
    let link_id = NodeId::new_v4();
    let link = store.directory().join(format!("{link_id}.veylvik"));
    std::os::unix::fs::symlink(&original, &link).unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    assert!(store.read(link_id).is_err());
    store.remove(id).unwrap();
    store.remove(id).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    assert!(store.directory().join("unrelated.txt").exists());
}
