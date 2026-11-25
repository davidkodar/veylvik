//! A baked flock reads back the frames it was baked from, is really what is drawn, and is left aside once the flock's settings change.
use std::sync::Arc;
use veylvik_document::{Composition, Node, Operation, Time, blank, registry};
use veylvik_io::flock::{Bake, Header, write};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 160;
const H: u32 = 120;

fn flock() -> (Composition, veylvik_document::NodeId) {
    let mut value = registry::PARTICLES.default_operation();
    for (name, number) in [
        ("x", 80.0),
        ("y", 60.0),
        ("width", 120.0),
        ("height", 80.0),
        ("rate", 300.0),
        ("life", 10.0),
        ("start", -1.0),
        ("speed", 30.0),
        ("spread", 360.0),
        ("gravity", 0.0),
        ("size", 1.0),
        ("flock_radius", 25.0),
        ("cohesion", 60.0),
        ("separation", 40.0),
        ("alignment", 1.0),
        ("top_speed", 40.0),
    ] {
        value[name] = serde_json::json!({"kind": "constant", "value": number});
    }
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    doc.frame_count = 20;
    let node = Node::new("Birds", serde_json::from_value(value).unwrap(), &[]);
    let id = node.id;
    doc.output = id;
    doc.nodes.insert(id, node);
    (doc, id)
}
fn render(
    renderer: &Renderer,
    doc: &Composition,
    inputs: &ImageInputs,
    frame: i64,
) -> Vec<[f32; 4]> {
    renderer
        .render_frame(
            &Plan::compile(doc).unwrap(),
            RenderRequest::full(Time::at_frame(frame, doc.frame_rate).unwrap()),
            inputs,
        )
        .unwrap()
        .pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_baked_flock_is_read_back_exactly_and_set_aside_when_it_changes() {
    let renderer = Renderer::new().unwrap();
    let folder = std::env::temp_dir().join(format!("veylvik-bake-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let (mut doc, id) = flock();
    let plan = Plan::compile(&doc).unwrap();
    let header = Header {
        key: Renderer::flock_key(&doc, id).unwrap(),
        frame_rate: doc.frame_rate,
        first: 0,
        frames: doc.frame_count,
        particles: veylvik_render::flock_capacity(W, H),
    };
    let path = folder.join("birds.veyflock");
    write(&path, header, |k| {
        renderer
            .flock_states(
                &plan,
                id,
                Time::at_frame(i64::from(k), doc.frame_rate).unwrap(),
                &veylvik_render::ImageInputs::default(),
            )
            .map_err(|e| e.to_string())
    })
    .unwrap();
    // Simulated, then read from the bake in a renderer of its own: the same pictures.
    let simulated: Vec<_> = [3, 11, 19]
        .iter()
        .map(|f| render(&renderer, &doc, &ImageInputs::default(), *f))
        .collect();
    if let Operation::Particles(p) = &mut doc.nodes.get_mut(&id).unwrap().operation {
        p.bake = "birds.veyflock".into();
    }
    let mut inputs = ImageInputs::default();
    inputs.insert_flock("birds.veyflock", Arc::new(Bake::open(&path).unwrap()));
    let reader = Renderer::new().unwrap();
    for (k, f) in [3, 11, 19].iter().enumerate() {
        assert!(
            render(&reader, &doc, &inputs, *f) == simulated[k],
            "frame {f} read from the bake"
        );
    }
    // A bake of other places with the same key is what is drawn.
    let moved = folder.join("moved.veyflock");
    write(&moved, header, |k| {
        let mut states = renderer
            .flock_states(
                &plan,
                id,
                Time::at_frame(i64::from(k), doc.frame_rate).unwrap(),
                &veylvik_render::ImageInputs::default(),
            )
            .map_err(|e| e.to_string())?;
        for s in &mut states {
            s[0] += 20.0;
        }
        Ok(states)
    })
    .unwrap();
    let mut shifted = ImageInputs::default();
    shifted.insert_flock("birds.veyflock", Arc::new(Bake::open(&moved).unwrap()));
    assert!(
        render(&reader, &doc, &shifted, 11) != simulated[1],
        "the bake is what is drawn"
    );
    // Changed settings: the bake no longer fits, and it is simulated anew.
    if let Operation::Particles(p) = &mut doc.nodes.get_mut(&id).unwrap().operation {
        p.cohesion = 10.0.into();
    }
    let changed = render(&reader, &doc, &shifted, 11);
    let mut fresh = doc.clone();
    if let Operation::Particles(p) = &mut fresh.nodes.get_mut(&id).unwrap().operation {
        p.bake.clear();
    }
    assert!(
        changed
            == render(
                &Renderer::new().unwrap(),
                &fresh,
                &ImageInputs::default(),
                11
            ),
        "simulated anew"
    );
    let _ = std::fs::remove_dir_all(&folder);
}
