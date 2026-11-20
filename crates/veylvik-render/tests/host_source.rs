//! A host's media as footage: read from the host's own buffers, kept by the host's name for each frame, and made again when the host says the frame is another.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 16;
const H: u32 = 9;

/// The frame of a 24 frames a second clip at `time`.
fn number(time: Time) -> u64 {
    (time.ticks * 24 / time.rate as i64) as u64
}
/// A host with one clip: every frame one flat straight colour, half transparent, whose
/// red is the frame number and whose green is the media's version.
struct Clip {
    device: wgpu::Device,
    queue: wgpu::Queue,
    version: AtomicU64,
    asked: AtomicU64,
}
impl HostMedia for Clip {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            width: W,
            height: H,
            duration: None,
            frame_rate: Some(Time { ticks: 24, rate: 1 }),
            frames: None,
            description: "the host's clip".into(),
        }
    }
    fn identity(&self, time: Time) -> u64 {
        (self.version.load(Ordering::SeqCst) << 32) | number(time)
    }
    fn frame(&self, time: Time, _: Option<[u32; 2]>) -> veylvik_render::Result<HostFrame> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let frame = number(time) as f32;
        let colour = [
            frame / 10.0,
            self.version.load(Ordering::SeqCst) as f32 / 10.0,
            0.25,
            0.5,
        ];
        let bytes: Vec<u8> = (0..W * H)
            .flat_map(|_| colour)
            .flat_map(f32::to_le_bytes)
            .collect();
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("the host's frame"),
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
fn a_hosts_media_is_read_once_kept_by_its_name_and_read_again_when_it_changes() {
    let renderer = Renderer::new().unwrap();
    let rate = Time { ticks: 24, rate: 1 };
    let mut doc = blank(W, H, rate).unwrap();
    doc.frame_count = 48;
    let footage = Node::new(
        "clip",
        Operation::Footage(nodes::Footage {
            asset: "host:clip".into(),
            timing: Default::default(),
            placement: Default::default(),
            colour: Default::default(),
            layer: None,
            fallback: String::new(),
        }),
        &[],
    );
    let grade = Node::new(
        "grade",
        serde_json::from_value(
            registry::TYPES
                .iter()
                .find(|kind| kind.type_id == "exposure")
                .unwrap()
                .default_operation(),
        )
        .unwrap(),
        &[("image", footage.id)],
    );
    doc.output = grade.id;
    doc.nodes.insert(footage.id, footage);
    doc.nodes.insert(grade.id, grade);
    let plan = Plan::compile(&doc).unwrap();
    let source = Arc::new(HostSource::new(
        Clip {
            device: renderer.device().clone(),
            queue: renderer.queue().clone(),
            version: AtomicU64::new(1),
            asked: AtomicU64::new(0),
        },
        // Room for two frames.
        2 * u64::from(W * H) * 16,
    ));
    let mut inputs = ImageInputs::default();
    inputs.insert_source("host:clip", source.clone());
    let at = |frame| RenderRequest::full(Time::at_frame(frame, rate).unwrap());
    let asked = || source.media().asked.load(Ordering::SeqCst);
    // The way an editor or a host asks: through the renderer's kept pictures.
    let look = |frame| {
        let made = renderer
            .render_gpu_cached(&plan, at(frame), &inputs)
            .unwrap();
        renderer.read_frame(&made).unwrap().pixels[0]
    };

    // The host's straight colour arrives multiplied by its alpha.
    let first = look(3);
    for (got, want) in first.iter().zip([0.3 * 0.5, 0.1 * 0.5, 0.25 * 0.5, 0.5]) {
        assert!((got - want).abs() < 1e-5, "{first:?}");
    }
    assert_eq!(asked(), 1);
    // The same frame again: the host is not asked, and the picture made of it is found
    // again, because the frame it is made of is the same one.
    let (identity, hits) = (
        renderer.identity(&plan, at(3), &inputs).unwrap(),
        renderer.cache_stats().unwrap().hits,
    );
    assert_eq!(look(3), first);
    assert_eq!(
        asked(),
        1,
        "a frame the host already gave was asked for again"
    );
    assert_eq!(renderer.identity(&plan, at(3), &inputs).unwrap(), identity);
    assert!(
        renderer.cache_stats().unwrap().hits > hits,
        "the picture was made again"
    );
    // Another moment is another frame.
    assert!((look(5)[0] - 0.25).abs() < 1e-5);
    assert_eq!(asked(), 2);
    // The host's media changes under the same key: an old frame must not be shown.
    source.media().version.store(2, Ordering::SeqCst);
    let changed = look(3);
    assert!(
        (changed[1] - 0.2 * 0.5).abs() < 1e-5,
        "the old frame was shown: {changed:?}"
    );
    assert_eq!(asked(), 3);
    assert_ne!(renderer.identity(&plan, at(3), &inputs).unwrap(), identity);
    // Never more than the budget, and nothing after forgetting.
    look(7);
    look(9);
    assert_eq!(source.held(), 2 * u64::from(W * H) * 16);
    source.forget();
    assert_eq!(source.held(), 0);
}
