//! A composition rendered for a host finds the host's media through the host it was given, with no editor in the process, and two hosts do not see each other's.
use std::sync::Arc;
use veylvik_document::*;
use veylvik_editor::{
    ClipSource,
    host::{Host, Pick, Picked},
};
use veylvik_render::*;

const W: u32 = 8;
const H: u32 = 4;

/// Media that is one flat colour, held as a host holds frames.
struct Flat {
    device: wgpu::Device,
    queue: wgpu::Queue,
    red: f32,
}
impl HostMedia for Flat {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            width: W,
            height: H,
            duration: None,
            frame_rate: None,
            frames: None,
            description: "a host's still".into(),
        }
    }
    fn identity(&self, _: Time) -> u64 {
        1
    }
    fn frame(&self, _: Time, _: Option<[u32; 2]>) -> veylvik_render::Result<HostFrame> {
        let bytes: Vec<u8> = (0..W * H)
            .flat_map(|_| [self.red, 0.0, 0.0, 1.0])
            .flat_map(f32::to_le_bytes)
            .collect();
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("a host's frame"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, &bytes);
        Ok(HostFrame {
            buffer,
            width: W,
            height: H,
            pixels: HostPixels::rec709(true),
        })
    }
}
/// A host that knows one key.
struct Pool(Arc<dyn FrameSource>);
impl Host for Pool {
    fn pick(&self, _: &Pick) -> std::result::Result<Option<Picked>, String> {
        Ok(None)
    }
    fn media(&self, key: &str) -> Option<Arc<dyn FrameSource>> {
        (key == "pool:1").then(|| self.0.clone())
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn each_clip_source_asks_the_host_it_was_given() {
    let renderer = Arc::new(Renderer::new().unwrap());
    let rate = Time { ticks: 24, rate: 1 };
    let mut composition = blank(W, H, rate).unwrap();
    let footage = Node::new(
        "clip",
        Operation::Footage(nodes::Footage {
            asset: "pool:1".into(),
            timing: Default::default(),
            placement: Default::default(),
            colour: Default::default(),
            layer: None,
            fallback: String::new(),
        }),
        &[],
    );
    composition.output = footage.id;
    composition.nodes.insert(footage.id, footage);
    let host = |red: f32| -> Arc<dyn Host> {
        Arc::new(Pool(Arc::new(HostSource::new(
            Flat {
                device: renderer.device().clone(),
                queue: renderer.queue().clone(),
                red,
            },
            1 << 20,
        ))))
    };
    let folder = std::env::temp_dir();
    // No host, and no editor ever made in this process: the key means nothing.
    assert!(ClipSource::new(renderer.clone(), composition.clone(), folder.clone(), None).is_err());
    let target = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("the host's target"),
        size: u64::from(W * H) * 16,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for red in [0.25_f32, 0.75] {
        let source = ClipSource::new(
            renderer.clone(),
            composition.clone(),
            folder.clone(),
            Some(host(red)),
        )
        .unwrap();
        let pixels = HostPixels::rec709(false);
        source
            .render(Time::at_frame(0, rate).unwrap(), [W, H], &target, &pixels)
            .unwrap();
        let back = renderer
            .read_frame(&renderer.frame_from_host(&target, W, H, &pixels).unwrap())
            .unwrap()
            .pixels[0];
        assert!(
            (back[0] - red).abs() < 1e-5 && back[3] == 1.0,
            "{red}: {back:?}"
        );
    }
}

/// Media with a different value in every pixel, straight alpha that is not one everywhere.
struct Ramp {
    device: wgpu::Device,
    queue: wgpu::Queue,
}
fn ramp() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as f32, (i / W) as f32);
            [
                x / 8.0,
                y / 4.0,
                1.5 - x / 16.0,
                if i % 3 == 0 { 0.5 } else { 1.0 },
            ]
        })
        .collect()
}
impl HostMedia for Ramp {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            width: W,
            height: H,
            duration: None,
            frame_rate: Some(Time { ticks: 24, rate: 1 }),
            frames: Some(48),
            description: "a host's clip".into(),
        }
    }
    fn identity(&self, _: Time) -> u64 {
        7
    }
    fn frame(&self, _: Time, _: Option<[u32; 2]>) -> veylvik_render::Result<HostFrame> {
        let bytes: Vec<u8> = ramp()
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("a host's frame"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, &bytes);
        Ok(HostFrame {
            buffer,
            width: W,
            height: H,
            pixels: HostPixels::rec709(true),
        })
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn a_clips_first_composition_is_the_clip_unchanged() {
    let renderer = Arc::new(Renderer::new().unwrap());
    let rate = Time { ticks: 24, rate: 1 };
    let composition = Composition::from_host_footage(
        "pool:1",
        "Beach take 3",
        Some(std::path::Path::new("/media/beach-take-3.mov")),
        [W, H],
        rate,
        48,
    )
    .unwrap();
    // It is an ordinary composition: written and read back, it is the same one.
    let written = serde_json::to_string(&composition).unwrap();
    let read: Composition = serde_json::from_str(&written).unwrap();
    assert_eq!(serde_json::to_string(&read).unwrap(), written);
    assert_eq!((read.width, read.height, read.frame_count), (W, H, 48));
    let node = &read.nodes[&read.output];
    assert_eq!(node.label, "Beach take 3");
    let Operation::Footage(footage) = &node.operation else {
        panic!("the output is not the clip");
    };
    assert_eq!(
        (footage.asset.as_str(), footage.fallback.as_str()),
        ("pool:1", "/media/beach-take-3.mov")
    );
    // Rendered for the host at full size, it is the host's frame as the host gave it.
    let host: Arc<dyn Host> = Arc::new(Pool(Arc::new(HostSource::new(
        Ramp {
            device: renderer.device().clone(),
            queue: renderer.queue().clone(),
        },
        1 << 20,
    ))));
    let source = ClipSource::new(renderer.clone(), read, std::env::temp_dir(), Some(host)).unwrap();
    let target = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("the host's target"),
        size: u64::from(W * H) * 16,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let straight = HostPixels::rec709(true);
    let mut worst = 0.0_f32;
    for frame in [0, 20, 47] {
        source
            .render(
                Time::at_frame(frame, rate).unwrap(),
                [W, H],
                &target,
                &straight,
            )
            .unwrap();
        // Read the target as premultiplied pixels to see its numbers as they stand.
        let back = renderer
            .read_frame(
                &renderer
                    .frame_from_host(&target, W, H, &HostPixels::rec709(false))
                    .unwrap(),
            )
            .unwrap()
            .pixels;
        for (got, gave) in back.iter().zip(ramp()) {
            for c in 0..4 {
                worst = worst.max((got[c] - gave[c]).abs());
            }
        }
    }
    println!("largest difference from the host's own frame: {worst:e}");
    assert!(worst <= 2e-7, "the clip came back changed by {worst:e}");
}
