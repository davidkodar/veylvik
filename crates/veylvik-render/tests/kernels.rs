use veylvik_render::Renderer;

/// Kernels compile on first use, so this is what proves every one of them builds.
#[test]
#[ignore = "requires Vulkan"]
fn every_kernel_compiles() {
    let renderer = Renderer::new().unwrap();
    assert!(renderer.compile_kernels().unwrap() >= 82);
    // A second call finds them all compiled.
    assert!(renderer.compile_kernels().unwrap() >= 82);
}
