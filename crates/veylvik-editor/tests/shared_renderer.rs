//! An editor given the host's renderer draws with that renderer and makes the same pictures as an editor with a renderer of its own.
use std::sync::Arc;
use veylvik_document::{Node, Operation, Time, registry};
use veylvik_editor::{Capabilities, Editor, HOST_INTERFACE, LiveControl, Opens, Setup};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

fn of(kind: &str) -> Operation {
    serde_json::from_value(
        registry::TYPES
            .iter()
            .find(|t| t.type_id == kind)
            .unwrap()
            .default_operation(),
    )
    .unwrap()
}
/// A 3D cube filmed and blurred: the 3D renderer and an ordinary 2D pass.
fn composition() -> veylvik_document::Composition {
    let mut composition = veylvik_document::blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
    let shape = Node::new("cube", of("shape_3d"), &[]);
    let film = Node::new("film", of("render_scene"), &[("scene", shape.id)]);
    let graded = Node::new("graded", of("blur"), &[("image", film.id)]);
    composition.output = graded.id;
    for node in [shape, film, graded] {
        composition.nodes.insert(node.id, node);
    }
    composition
}
fn editor(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    adapter: &wgpu::AdapterInfo,
    renderer: Option<Arc<Renderer>>,
) -> Editor {
    editor_in(
        device,
        queue,
        adapter,
        renderer,
        &eframe::egui::Context::default(),
    )
}
fn editor_in(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    adapter: &wgpu::AdapterInfo,
    renderer: Option<Arc<Renderer>>,
    context: &eframe::egui::Context,
) -> Editor {
    Editor::new(Setup {
        interface: HOST_INTERFACE,
        context: context.clone(),
        device: device.clone(),
        queue: queue.clone(),
        adapter: adapter.clone(),
        target_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        opens: Opens::Composition {
            composition: Box::new(composition()),
            assets: std::env::temp_dir(),
        },
        live_control: LiveControl::Off,
        capabilities: Capabilities::embedded(),
        host: None,
        renderer,
    })
    .unwrap()
}
/// A host that has one clip, "clip", whose pictures it keeps itself.
struct Theirs;
impl veylvik_render::FrameSource for Theirs {
    fn byte_budget(&self) -> u64 {
        0
    }
    fn frame(&self, _: Time) -> veylvik_render::Result<Arc<veylvik_render::Image>> {
        Err(veylvik_render::Error::Message(
            "pictures only on the GPU".into(),
        ))
    }
}
struct Host;
impl veylvik_editor::host::Host for Host {
    fn pick(
        &self,
        _: &veylvik_editor::host::Pick,
    ) -> Result<Option<veylvik_editor::host::Picked>, String> {
        Ok(None)
    }
    fn media(&self, key: &str) -> Option<Arc<dyn veylvik_render::FrameSource>> {
        (key == "clip").then(|| Arc::new(Theirs) as Arc<dyn veylvik_render::FrameSource>)
    }
}
fn picture(renderer: &Renderer) -> Vec<[f32; 4]> {
    let plan = Plan::compile(&composition()).unwrap();
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels
}

#[test]
#[ignore = "requires Vulkan"]
fn an_editor_given_the_hosts_renderer_draws_with_it_and_makes_the_same_pictures() {
    let (_instance, adapter, device, queue) =
        pollster::block_on(veylvik_render::create_gpu()).unwrap();
    let adapter = adapter.get_info();
    // The host's renderer, as Klipp makes it: no 3D renderer of its own.
    let theirs =
        Arc::new(Renderer::from_device(device.clone(), queue.clone(), adapter.clone()).unwrap());
    let budget = theirs.budget_bytes;
    let shared = editor(&device, &queue, &adapter, Some(theirs.clone()));
    assert!(
        Arc::ptr_eq(&shared.renderer(), &theirs),
        "the editor made a renderer of its own"
    );
    assert_eq!(theirs.budget_bytes, budget, "the host's budget was changed");
    // The editor added the 3D renderer to the host's, so a Render scene works there.
    let with_shared = picture(&theirs);
    assert!(
        with_shared.iter().any(|p| p[3] > 0.0),
        "the cube is not there"
    );
    // An editor that makes its own renderer, as Studio does, makes the same picture.
    let own = editor(&device, &queue, &adapter, None);
    assert!(!Arc::ptr_eq(&own.renderer(), &theirs));
    let with_own = picture(&own.renderer());
    let bits = |pixels: &[[f32; 4]]| -> Vec<u32> {
        pixels.iter().flatten().map(|v| v.to_bits()).collect()
    };
    assert_eq!(bits(&with_shared), bits(&with_own), "the pictures differ");
}

#[test]
#[ignore = "requires Vulkan"]
fn a_hosts_own_fonts_do_not_take_the_editors_away() {
    use eframe::egui;
    let (_instance, adapter, device, queue) =
        pollster::block_on(veylvik_render::create_gpu()).unwrap();
    let adapter = adapter.get_info();
    let context = egui::Context::default();
    let mut editor = editor_in(&device, &queue, &adapter, None, &context);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 720.0),
        )),
        ..Default::default()
    };
    let pass = |editor: &mut Editor| {
        let _ = context.run_ui(input(), |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| editor.ui(ui));
        });
    };
    // The host draws the editor for a few passes, then sets fonts of its own (a theme
    // change), which drops the families the editor added, and goes on drawing it.
    for _ in 0..4 {
        pass(&mut editor);
    }
    context.set_fonts(egui::FontDefinitions::default());
    for _ in 0..4 {
        pass(&mut editor);
    }
    assert!(
        context.fonts(|fonts| fonts
            .definitions()
            .families
            .contains_key(&egui::FontFamily::Name("veylvik".into()))),
        "the editor's fonts were not put back"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn the_composition_an_editor_opens_with_finds_the_hosts_media() {
    let (_instance, adapter, device, queue) =
        pollster::block_on(veylvik_render::create_gpu()).unwrap();
    let composition = veylvik_document::Composition::from_host_footage(
        "clip",
        "Handheld",
        None,
        [64, 36],
        Time::new(24, 1).unwrap(),
        48,
    )
    .unwrap();
    let editor = Editor::new(Setup {
        interface: HOST_INTERFACE,
        context: eframe::egui::Context::default(),
        device,
        queue,
        adapter: adapter.get_info(),
        target_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        opens: Opens::Composition {
            composition: Box::new(composition),
            assets: std::env::temp_dir(),
        },
        live_control: LiveControl::Off,
        capabilities: Capabilities::embedded(),
        host: Some(Arc::new(Host)),
        renderer: None,
    })
    .unwrap();
    assert!(
        editor.unavailable_media().is_empty(),
        "{:?}",
        editor.unavailable_media()
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn a_host_carries_its_place_in_and_hears_where_the_artist_moved_it() {
    use eframe::egui;
    let (_instance, adapter, device, queue) =
        pollster::block_on(veylvik_render::create_gpu()).unwrap();
    let rate = Time::new(24, 1).unwrap();
    let mut long = composition();
    long.frame_count = 48;
    let context = egui::Context::default();
    let mut editor = Editor::new(Setup {
        interface: HOST_INTERFACE,
        context: context.clone(),
        device,
        queue,
        adapter: adapter.get_info(),
        target_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        opens: Opens::Composition {
            composition: Box::new(long),
            assets: std::env::temp_dir(),
        },
        live_control: LiveControl::Off,
        capabilities: Capabilities::embedded(),
        host: None,
        renderer: None,
    })
    .unwrap();
    let at = |frame: i64| Time::at_frame(frame, rate).unwrap();
    // The frame a moment is (times have no ==: 1/24 and 2/48 are one moment).
    let frame = |time: Time| (time.ticks as f64 * 24.0 / f64::from(time.rate)).round() as i64;
    let frames = |range: Option<(Time, Time)>| range.map(|(a, b)| (frame(a), frame(b)));
    // The host's place comes in, and is not reported back.
    assert!(editor.set_time(Time::new(1, 1).unwrap()));
    assert_eq!(frame(editor.time()), 24);
    assert!(editor.take_time_change().is_none());
    // Beyond the composition: its last frame.
    editor.set_time(Time::new(10, 1).unwrap());
    assert_eq!(frame(editor.time()), 47);
    editor.take_time_change();
    // A clip's used part, half a second to a second and a half.
    let used = (Time::new(1, 2).unwrap(), Time::new(3, 2).unwrap());
    editor.set_range(Some(used));
    assert_eq!(frames(editor.range()), Some((12, 36)));
    editor.set_range(None);
    assert!(editor.range().is_none());
    // The artist steps a frame on (the Right arrow): the host hears of it once.
    editor.set_time(at(20));
    let input = |events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 720.0),
        )),
        events,
        ..Default::default()
    };
    for events in [
        vec![],
        vec![],
        vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ] {
        let _ = context.run_ui(input(events), |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| editor.ui(ui));
        });
    }
    assert_eq!(editor.take_time_change().map(frame), Some(21));
    assert!(editor.take_time_change().is_none());
}
