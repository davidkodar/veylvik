use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

/// Kernels compile on first use, so this is what proves every one of them builds.
#[test]
#[ignore = "requires Vulkan"]
fn every_kernel_compiles() {
    let renderer = Renderer::new().unwrap();
    assert!(renderer.compile_kernels().unwrap() >= 82);
    // A second call finds them all compiled.
    assert!(renderer.compile_kernels().unwrap() >= 82);
}

#[test]
#[ignore = "requires Vulkan"]
fn compiled_programs_are_kept_in_a_file_and_a_damaged_file_is_ignored() {
    use veylvik_document::Time;
    let folder = std::env::temp_dir().join(format!(
        "veylvik-pipelines-test-{}",
        veylvik_document::NodeId::new_v4()
    ));
    let plan = Plan::compile(&veylvik_document::motion_demo()).unwrap();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let file = || {
        std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|e| e == "veylvik"))
    };
    // A first renderer compiles into the cache and writes it.
    let first = Renderer::new().unwrap();
    if !first.use_pipeline_cache(&folder).unwrap() {
        eprintln!("this device cannot keep compiled programs; nothing to test");
        return;
    }
    assert!(first.compile_for(&plan).unwrap() > 0);
    let kept = std::fs::read(file().expect("no cache file written")).unwrap();
    assert!(kept.len() > 24);
    let picture = first
        .render_frame(&plan, request, &ImageInputs::default())
        .unwrap()
        .pixels;
    drop(first);
    // A second starts from it and renders the same picture.
    let second = Renderer::new().unwrap();
    assert!(second.use_pipeline_cache(&folder).unwrap());
    second.compile_for(&plan).unwrap();
    assert!(
        second
            .render_frame(&plan, request, &ImageInputs::default())
            .unwrap()
            .pixels
            == picture
    );
    drop(second);
    // A damaged file (cut short, then bytes changed) is not handed to the driver: the
    // renderer starts an empty cache, compiles, and renders the same picture.
    let path = file().unwrap();
    for damaged in [kept[..kept.len() / 2].to_vec(), {
        let mut changed = kept.clone();
        let middle = changed.len() / 2;
        changed[middle] ^= 0xff;
        changed
    }] {
        std::fs::write(&path, &damaged).unwrap();
        let third = Renderer::new().unwrap();
        assert!(third.use_pipeline_cache(&folder).unwrap());
        third.compile_for(&plan).unwrap();
        assert!(
            third
                .render_frame(&plan, request, &ImageInputs::default())
                .unwrap()
                .pixels
                == picture
        );
    }
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
#[ignore = "requires Vulkan"]
fn a_render_that_compiles_programs_is_told_apart_from_one_that_does_not() {
    use veylvik_document::Time;
    let plan = Plan::compile(&veylvik_document::motion_demo()).unwrap();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let renderer = Renderer::new().unwrap();
    let before = renderer.compiled_programs();
    renderer
        .render_frame(&plan, request, &ImageInputs::default())
        .unwrap();
    let first = renderer.compiled_programs();
    assert!(first > before, "the first render compiles its kernels");
    renderer
        .render_frame(&plan, request, &ImageInputs::default())
        .unwrap();
    assert_eq!(
        renderer.compiled_programs(),
        first,
        "the second finds them compiled"
    );
}
