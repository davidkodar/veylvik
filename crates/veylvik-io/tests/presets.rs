use veylvik_document::{NodeId, presets::builtin};
use veylvik_io::presets;
#[test]
fn library_round_trip_preserves_values_and_exports_never_overwrite() {
    let root = std::env::temp_dir().join(format!("veylvik-presets-{}", NodeId::new_v4()));
    assert!(presets::list(&root).unwrap().entries.is_empty());
    let preset = builtin::build("prismatic_glow").unwrap();
    let path = presets::add(&root, &preset).unwrap();
    assert_eq!(
        serde_json::to_value(presets::load(&path).unwrap()).unwrap(),
        serde_json::to_value(&preset).unwrap()
    );
    let before = std::fs::read(&path).unwrap();
    assert!(presets::save_new(&path, &preset).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let listing = presets::list(&root).unwrap();
    assert_eq!(listing.entries.len(), 1);
    assert!(listing.warnings.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn corrupt_oversized_and_symlinked_entries_do_not_hide_healthy_presets() {
    let root = std::env::temp_dir().join(format!("veylvik-presets-{}", NodeId::new_v4()));
    let preset = builtin::build("liquid_shimmer").unwrap();
    let path = presets::add(&root, &preset).unwrap();
    std::fs::write(root.join("bad.veyfx"), b"{}").unwrap();
    std::fs::File::create(root.join("huge.veyfx"))
        .unwrap()
        .set_len(presets::MAX_PRESET_BYTES as u64 + 1)
        .unwrap();
    std::os::unix::fs::symlink(&path, root.join("link.veyfx")).unwrap();
    assert!(presets::load(&root.join("huge.veyfx")).is_err());
    let listing = presets::list(&root).unwrap();
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.warnings.len(), 2);
    std::fs::remove_dir_all(root).unwrap();
}
