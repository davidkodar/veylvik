use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn a_hosts_pixels_become_a_frame_and_go_back_unchanged() {
    let renderer = Renderer::new().unwrap();
    let (width, height) = (5u32, 3u32);
    // Straight colour in ACEScg: an ordinary pixel, a half-transparent one, light brighter
    // than white, and a clear one.
    let pixels: Vec<[f32; 4]> = (0..width * height)
        .map(|i| match i % 4 {
            0 => [0.18, 0.5, 0.9, 1.0],
            1 => [0.6, 0.2, 0.1, 0.5],
            2 => [4.0, 2.5, 0.3, 0.25],
            _ => [0.0, 0.0, 0.0, 0.0],
        })
        .collect();
    let bytes: Vec<u8> = pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let usage =
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
    let buffer = |label| {
        renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage,
            mapped_at_creation: false,
        })
    };
    let (theirs, back) = (buffer("the host's frame"), buffer("the frame handed back"));
    renderer.queue().write_buffer(&theirs, 0, &bytes);
    let host = HostPixels::acescg(true);
    let frame = renderer
        .frame_from_host(&theirs, width, height, &host)
        .unwrap();
    // Ours: the host's colour through its matrix, multiplied by its alpha.
    let ours = renderer.read_frame(&frame).unwrap().pixels;
    for (got, from) in ours.iter().zip(&pixels) {
        for row in 0..3 {
            let want = (0..3)
                .map(|c| host.to_rec709[row][c] * from[c])
                .sum::<f32>()
                * from[3];
            assert!((got[row] - want).abs() < 1e-5, "{got:?} from {from:?}");
        }
        assert_eq!(got[3], from[3]);
    }
    // And back to the host as it stores pixels: what it gave us.
    renderer.frame_to_host(&frame, &back, &host).unwrap();
    let returned = renderer
        .read_frame(
            &renderer
                .frame_from_host(&back, width, height, &HostPixels::rec709(false))
                .unwrap(),
        )
        .unwrap()
        .pixels;
    for (got, from) in returned.iter().zip(&pixels) {
        for c in 0..4 {
            assert!(
                (got[c] - from[c]).abs() < 2e-5,
                "{got:?} back from {from:?}"
            );
        }
    }
    // A buffer too small, or one that shaders cannot read, is refused.
    assert!(
        renderer
            .frame_from_host(&theirs, width + 1, height, &host)
            .is_err()
    );
    let plain = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("not a storage buffer"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    assert!(
        renderer
            .frame_from_host(&plain, width, height, &host)
            .is_err()
    );
}

#[test]
#[ignore = "requires Vulkan; prints timings"]
fn how_long_the_exchange_takes() {
    let mut renderer = Renderer::new().unwrap();
    // Room for 4K frames of 133 MB each.
    renderer.budget_bytes = 4 << 30;
    let host = HostPixels::acescg(true);
    for (width, height) in [(1920u32, 1080u32), (3840, 2160)] {
        let bytes = u64::from(width) * u64::from(height) * 16;
        let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("the host's frame"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let (mut inwards, mut outwards) = (Vec::new(), Vec::new());
        for _ in 0..20 {
            let begun = std::time::Instant::now();
            let frame = renderer
                .frame_from_host(&buffer, width, height, &host)
                .unwrap();
            inwards.push(begun.elapsed().as_secs_f64() * 1000.0);
            let begun = std::time::Instant::now();
            renderer.frame_to_host(&frame, &buffer, &host).unwrap();
            outwards.push(begun.elapsed().as_secs_f64() * 1000.0);
        }
        inwards.sort_by(f64::total_cmp);
        outwards.sort_by(f64::total_cmp);
        println!(
            "{width}x{height}: from the host {:.2} ms, to the host {:.2} ms (medians of 20, each waiting for the GPU)",
            inwards[10], outwards[10]
        );
    }
}
