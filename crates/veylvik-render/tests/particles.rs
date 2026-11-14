use veylvik_document::*;
use veylvik_render::*;

fn render(renderer: &Renderer, doc: &Composition) -> Vec<[f32; 4]> {
    renderer
        .render(&Plan::compile(doc).unwrap(), Time::new(0, 1).unwrap())
        .unwrap()
}

#[test]
#[ignore = "requires Vulkan"]
fn liquid_tracks_input_coverage_and_has_no_emitter_on_empty_columns() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(160, 120, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation = Operation::Ellipse(nodes::Ellipse {
        x: 80.0.into(),
        y: 32.0.into(),
        radius_x: 30.0.into(),
        radius_y: 12.0.into(),
        feather: 0.5.into(),
    });
    let original = render(&renderer, &doc);
    let drip = Node::new(
        "drips",
        Operation::Drip(nodes::Drip {
            seed: 47,
            evolution: 1.7.into(),
            spacing: 16.0.into(),
            length: 60.0.into(),
            radius: 3.0.into(),
        }),
        &[("image", source)],
    );
    doc.output = drip.id;
    doc.nodes.insert(drip.id, drip);
    let wet = render(&renderer, &doc);
    assert!(wet.iter().zip(&original).all(|(a, b)| a[3] >= b[3]));
    assert!(wet.iter().skip(48 * 160).any(|p| p[3] > 0.5));
    assert!(
        wet.iter()
            .enumerate()
            .filter(|(i, _)| i % 160 < 40 || i % 160 > 120)
            .all(|(_, p)| p[3] == 0.0)
    );
    assert!(
        wet.iter()
            .all(|p| p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    );
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("evolution")
        .unwrap() = 5.7.into();
    let looped = render(&renderer, &doc);
    assert!(
        wet.iter()
            .zip(looped)
            .all(|(a, b)| (a[3] - b[3]).abs() < 0.0001)
    );
    doc.nodes.get_mut(&source).unwrap().operation =
        Operation::Solid(nodes::Solid { color: [0.0; 4] });
    assert!(render(&renderer, &doc).iter().all(|p| *p == [0.0; 4]));
}

#[test]
#[ignore = "requires Vulkan"]
fn dust_is_sparse_non_emissive_periodic_and_scrubbable() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(480, 270, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Dust(nodes::Dust {
        seed: 47,
        evolution: 0.0.into(),
        density: 0.72.into(),
        size: 0.625.into(),
        drift: 1.0.into(),
    });
    let first = render(&renderer, &doc);
    assert!(first.iter().any(|p| p[3] > 0.05));
    assert!(first.iter().filter(|p| p[3] > 0.01).count() < first.len() / 10);
    assert!(first.iter().all(|p| p.iter().all(|v| v.is_finite())
        && p[..3].iter().all(|v| *v >= 0.0 && *v <= p[3])
        && (0.0..=1.0).contains(&p[3])));
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("evolution")
        .unwrap() = 1.0.into();
    assert_ne!(first, render(&renderer, &doc));
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("evolution")
        .unwrap() = 4.0.into();
    assert_eq!(first, render(&renderer, &doc));
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("density")
        .unwrap() = 0.0.into();
    assert!(render(&renderer, &doc).iter().all(|p| *p == [0.0; 4]));
}

#[test]
#[ignore = "requires Vulkan"]
fn revised_effect_examples_loop_and_move_after_resolution_adaptation() {
    let renderer = Renderer::new().unwrap();
    for id in [
        "condensation",
        "dust_motes",
        "embers",
        "fireflies",
        "liquid_shimmer",
        "wet_drips",
        "water_drips",
    ] {
        let mut scene = presets::showcase::build(id).unwrap();
        let from = [scene.composition.width, scene.composition.height];
        presets::resolution::map_graph(
            &mut scene.composition.nodes,
            &mut scene.composition.groups,
            from,
            [480, 270],
            presets::resolution::Sizing::Fit,
        )
        .unwrap();
        scene.composition.width = 480;
        scene.composition.height = 270;
        let plan = Plan::compile(&scene.composition).unwrap();
        let first = renderer.render(&plan, Time::new(0, 1).unwrap()).unwrap();
        let middle = renderer.render(&plan, Time::new(2, 1).unwrap()).unwrap();
        let last = renderer.render(&plan, Time::new(4, 1).unwrap()).unwrap();
        assert_eq!(first, last, "{id}: loop seam");
        assert_ne!(first, middle, "{id}: no animation");
        assert!(
            middle
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&p[3])),
            "{id}: invalid pixels"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn condensation_keeps_substrate_alpha_and_obeys_amount_and_upstream_lighting() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(160, 100, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation = Operation::Ellipse(nodes::Ellipse {
        x: 80.0.into(),
        y: 50.0.into(),
        radius_x: 60.0.into(),
        radius_y: 35.0.into(),
        feather: 2.0.into(),
    });
    let mut material: Operation =
        serde_json::from_value(registry::MATERIAL.default_operation()).unwrap();
    if let Operation::Material(m) = &mut material {
        m.kind = MaterialKind::Gold;
    }
    let material = Node::new("gold", material, &[("image", source)]);
    let material_id = material.id;
    doc.output = material_id;
    doc.nodes.insert(material_id, material);
    let dry = render(&renderer, &doc);
    let coating = Node::new(
        "condensation",
        Operation::Condensation(nodes::Condensation {
            dispersion: 0.012.into(),
            seed: 731,
            amount: 0.7.into(),
            size: 2.0.into(),
            density: 0.7.into(),
            haze: 0.2.into(),
            evolution: 0.0.into(),
            trails: 0.6.into(),
        }),
        &[("image", material_id)],
    );
    doc.output = coating.id;
    doc.nodes.insert(coating.id, coating);
    let wet = render(&renderer, &doc);
    assert_ne!(wet, dry);
    assert!(
        wet.iter()
            .zip(&dry)
            .all(|(wet, dry)| wet[3] == dry[3] && wet.iter().all(|v| v.is_finite()))
    );
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("amount")
        .unwrap() = 0.0.into();
    assert_eq!(dry, render(&renderer, &doc));
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("amount")
        .unwrap() = 0.7.into();
    if let Operation::Material(m) = &mut doc.nodes.get_mut(&material_id).unwrap().operation {
        m.studio.environment = 0.0;
        m.studio.ambient = 0.0;
        m.studio.lights.clear();
    }
    let unlit = render(&renderer, &doc);
    assert!(
        unlit
            .iter()
            .all(|p| p[..3].iter().all(|v| v.abs() < 0.00001)),
        "wet coating invented light on an unlit material"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn green_drip_template_really_changes_to_gold_after_the_mask_stage() {
    let renderer = Renderer::new().unwrap();
    let mut doc = presets::showcase::build("fireflies").unwrap().composition;
    let from = [doc.width, doc.height];
    presets::resolution::map_graph(
        &mut doc.nodes,
        &mut doc.groups,
        from,
        [480, 270],
        presets::resolution::Sizing::Fit,
    )
    .unwrap();
    doc.width = 480;
    doc.height = 270;
    let original = render(&renderer, &doc);
    let text = doc
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Text(_)))
        .unwrap()
        .id;
    let commands = presets::text_style::apply(&doc, text, Some("gold_text"), None).unwrap();
    let styled = Document::new(doc).unwrap().prepare(0, &commands).unwrap();
    let gold = render(&renderer, &styled);
    assert!(
        gold.iter()
            .zip(&original)
            .filter(|(a, b)| (a[0] - b[0]).abs() > 0.02)
            .count()
            > 500
    );
    assert_eq!(gold[0], original[0]);
    assert_eq!(gold[0][0], gold[0][1]);
    assert_eq!(gold[0][1], gold[0][2]);
}
