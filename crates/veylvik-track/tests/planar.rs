use std::sync::Arc;
use veylvik_document::{Operation, Time, planar::Homography};
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};
use veylvik_track::planar::{PlanarTracker, Settings};

#[test]
#[ignore = "requires Vulkan"]
fn gpu_planar_detection_perspective_occlusion_and_flat_rejection() {
    let renderer = Renderer::new().unwrap();
    let tracker = PlanarTracker::new(&renderer);
    let settings = Settings {
        patch_radius: 5,
        search_radius: 12,
        tolerance_pixels: 1.5,
        reanchor_below: 0,
        screen_area: false,
        motion: veylvik_track::estimate::Motion::Perspective,
    };
    let (width, height) = (192_u32, 144_u32);
    let surface = [[0.12, 0.15], [0.85, 0.15], [0.85, 0.85], [0.12, 0.85]];
    let h = Homography::between(
        surface,
        [[0.16, 0.13], [0.86, 0.18], [0.83, 0.88], [0.11, 0.81]],
    )
    .unwrap();
    // Continuous multi-frequency texture lets us generate a known perspective
    // warp without reusing the renderer's projective sampling implementation.
    let texture = |x: f64, y: f64| -> f32 {
        (0.5 + 0.13 * (x * 0.72 + y * 0.13).sin()
            + 0.12 * (y * 0.91 - x * 0.19).cos()
            + 0.12 * (x * 0.37 + y * 0.51).sin()
            + 0.1 * (x * 0.17 - y * 0.33).cos()) as f32
    };
    let mut doc = veylvik_document::blank(width, height, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        });
    let plan = Plan::compile(&doc).unwrap();
    let make = |motion: Homography, occlude: bool, flat: bool| {
        let inverse = motion.inverse().unwrap();
        let pixels = (0..width * height)
            .map(|i| {
                let x = f64::from(i % width);
                let y = f64::from(i / width);
                let p = inverse
                    .project([x / f64::from(width), y / f64::from(height)])
                    .unwrap();
                let v = if flat {
                    0.4
                } else if occlude && x > 75. && x < 110. && y > 45. && y < 105. {
                    texture(x + 33., y - 19.)
                } else {
                    texture(p[0] * f64::from(width), p[1] * f64::from(height))
                };
                [v, v, v, 1.]
            })
            .collect();
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "source",
            Arc::new(Image::new(width, height, pixels).unwrap()),
        );
        renderer
            .render_gpu(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
    };
    let reference = make(Homography::IDENTITY, false, false);
    let points = tracker
        .detect(&renderer, &reference, surface, settings)
        .unwrap();
    assert!(points.len() >= 16, "only {} features", points.len());
    let current = make(h, true, false);
    let found = tracker
        .track(&renderer, &reference, &current, &points, h, settings)
        .unwrap();
    assert!(
        found.features.iter().any(|f| !f.accepted),
        "occluder must reject matches"
    );
    assert!(found.features.iter().filter(|f| f.accepted).count() >= 12);
    for p in surface {
        let expected = h.project(p).unwrap();
        let actual = found.consensus.motion.project(p).unwrap();
        assert!(
            ((actual[0] - expected[0]) * f64::from(width)).abs() < 0.8,
            "{actual:?} != {expected:?}"
        );
        assert!(
            ((actual[1] - expected[1]) * f64::from(height)).abs() < 0.8,
            "{actual:?} != {expected:?}"
        );
    }
    // Initial solve must also work without already knowing the motion.
    let initial = tracker
        .track(
            &renderer,
            &reference,
            &current,
            &points,
            Homography::IDENTITY,
            settings,
        )
        .unwrap();
    assert!(initial.features.iter().filter(|f| f.accepted).count() >= 12);
    let refined = tracker
        .track(
            &renderer,
            &reference,
            &current,
            &points,
            initial.consensus.motion,
            settings,
        )
        .unwrap();
    assert!(refined.consensus.rms_pixels < 0.5);
    // An occlusion matte over the foreground object removes its matches before the
    // fit, and a matte on the reference keeps detection off the covered area.
    let matte = |inside: &dyn Fn(f64, f64) -> bool| {
        let pixels = (0..width * height)
            .map(|i| {
                let (x, y) = (f64::from(i % width), f64::from(i / width));
                if inside(x, y) {
                    [0., 0., 0., 1.]
                } else {
                    [0.; 4]
                }
            })
            .collect();
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "source",
            Arc::new(Image::new(width, height, pixels).unwrap()),
        );
        renderer
            .render_gpu(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
    };
    let (ox, oy) = ((73., 112.), (43., 107.));
    let occluder = matte(&|x, y| x > ox.0 && x < ox.1 && y > oy.0 && y < oy.1);
    let masked = tracker
        .track_masked(
            &renderer,
            &reference,
            &current,
            &points,
            h,
            settings,
            Some(&occluder),
        )
        .unwrap();
    let under = |f: &veylvik_document::planar::Feature| {
        let (x, y) = (
            f.position[0] * f64::from(width),
            f.position[1] * f64::from(height),
        );
        x > ox.0 - 4. && x < ox.1 + 4. && y > oy.0 - 4. && y < oy.1 + 4.
    };
    assert!(
        masked
            .features
            .iter()
            .filter(|f| under(f))
            .all(|f| !f.accepted)
    );
    assert!(masked.features.iter().filter(|f| f.accepted).count() >= 12);
    for p in surface {
        let (expected, actual) = (
            h.project(p).unwrap(),
            masked.consensus.motion.project(p).unwrap(),
        );
        assert!(((actual[0] - expected[0]) * f64::from(width)).abs() < 0.8);
        assert!(((actual[1] - expected[1]) * f64::from(height)).abs() < 0.8);
    }
    let left = matte(&|x, _| x < 96.);
    let right_only = tracker
        .detect_masked(&renderer, &reference, surface, settings, Some(&left))
        .unwrap();
    assert!(right_only.len() >= 8);
    assert!(
        right_only
            .iter()
            .all(|p| p[0] * f64::from(width) >= 96. - 0.5)
    );
    let flat = make(Homography::IDENTITY, false, true);
    assert!(tracker.detect(&renderer, &flat, surface, settings).is_err());
    assert!(
        tracker
            .track(&renderer, &reference, &flat, &points, h, settings)
            .is_err()
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn planar_warp_follows_stabilizes_and_invalidates_cached_motion() {
    use veylvik_document::{
        Node,
        planar::{Sample, Track, WarpMode},
    };
    let renderer = Renderer::new().unwrap();
    let mut doc = veylvik_document::blank(64, 48, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        });
    let reference = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let track = Node::new(
        "Track",
        Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack {
            data: Box::new(Track {
                reference,
                reference_time: Time::new(0, 1).unwrap(),
                region: None,
                samples: vec![
                    Sample {
                        time: Time::new(0, 1).unwrap(),
                        corners: reference,
                        features: vec![],
                        rms_pixels: 0.,
                    },
                    Sample {
                        time: Time::new(1, 1).unwrap(),
                        corners: reference.map(|[x, y]| [x + 0.125, y]),
                        features: vec![],
                        rms_pixels: 0.,
                    },
                ],
            }),
        }),
        &[("image", source)],
    );
    let track_id = track.id;
    doc.nodes.insert(track.id, track);
    let warp = Node::new(
        "Warp",
        Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp {
            mode: WarpMode::Follow,
        }),
        &[("image", source), ("track", track_id)],
    );
    doc.output = warp.id;
    doc.nodes.insert(warp.id, warp);
    let mut pixels = vec![[0.; 4]; 64 * 48];
    pixels[24 * 64 + 24] = [0.5, 0.2, 0.1, 0.5];
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "source",
        Arc::new(Image::new(64, 48, pixels.clone()).unwrap()),
    );
    let plan = Plan::compile(&doc).unwrap();
    let at = |plan: &Plan, ticks| {
        renderer
            .render_frame(
                plan,
                RenderRequest::full(Time::new(ticks, 1).unwrap()),
                &inputs,
            )
            .unwrap()
    };
    let first = at(&plan, 0);
    assert_eq!(first.pixels, pixels);
    let next = at(&plan, 1);
    assert_eq!(next.pixels[24 * 64 + 32], pixels[24 * 64 + 24]);
    assert_eq!(next.pixels[24 * 64 + 24], [0.; 4]);
    let a = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let b = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp {
            mode: WarpMode::Stabilize,
        });
    let stabilized = at(&Plan::compile(&doc).unwrap(), 1);
    assert_eq!(stabilized.pixels[24 * 64 + 16], pixels[24 * 64 + 24]);
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp {
            mode: WarpMode::Fit,
        });
    let fitted = at(&Plan::compile(&doc).unwrap(), 1);
    assert_eq!(fitted.pixels[24 * 64 + 32], pixels[24 * 64 + 24]);
    let Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack { data }) =
        &mut doc.nodes.get_mut(&track_id).unwrap().operation
    else {
        unreachable!()
    };
    data.samples[1].corners = reference.map(|[x, y]| [x + 0.25, y]);
    let edited_plan = Plan::compile(&doc).unwrap();
    let c = renderer
        .render_gpu_cached(
            &edited_plan,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(!Arc::ptr_eq(&b, &c));
    assert_eq!(
        at(&edited_plan, 1).pixels[24 * 64 + 40],
        pixels[24 * 64 + 24]
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn pyramid_recovers_large_motion_rotation_scale_and_odd_dimensions() {
    let renderer = Renderer::new().unwrap();
    let tracker = PlanarTracker::new(&renderer);
    let settings = Settings {
        patch_radius: 5,
        search_radius: 12,
        tolerance_pixels: 1.5,
        reanchor_below: 0,
        screen_area: false,
        motion: veylvik_track::estimate::Motion::Perspective,
    };
    let (width, height) = (513, 385);
    let surface = [[0.18, 0.22], [0.68, 0.22], [0.68, 0.78], [0.18, 0.78]];
    let angle = 0.055_f64;
    let scale = 1.045;
    let motion = Homography {
        matrix: [
            scale * angle.cos(),
            -scale * angle.sin(),
            0.145,
            scale * angle.sin(),
            scale * angle.cos(),
            -0.045,
            0.,
            0.,
            1.,
        ],
    };
    let texture = |x: f64, y: f64| {
        let mut value = 0.5;
        for i in 1..=24 {
            let k = f64::from(i);
            let frequency = 0.025 + 0.004 * k * k / 3.;
            value += 0.016
                * (x * frequency * (k * 2.13).cos() + y * frequency * (k * 2.13).sin() + k * k)
                    .sin();
        }
        value as f32
    };
    let mut doc = veylvik_document::blank(width, height, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        });
    let plan = Plan::compile(&doc).unwrap();
    let make = |h: Homography| {
        let inverse = h.inverse().unwrap();
        let pixels = (0..width * height)
            .map(|i| {
                let p = inverse
                    .project([
                        (f64::from(i % width) + 0.5) / f64::from(width),
                        (f64::from(i / width) + 0.5) / f64::from(height),
                    ])
                    .unwrap();
                let v = texture(p[0] * f64::from(width), p[1] * f64::from(height));
                [v, v, v, 1.]
            })
            .collect();
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "source",
            Arc::new(Image::new(width, height, pixels).unwrap()),
        );
        renderer
            .render_gpu(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
    };
    let reference = make(Homography::IDENTITY);
    let current = make(motion);
    let points = tracker
        .detect(&renderer, &reference, surface, settings)
        .unwrap();
    let a = tracker.pyramid(&renderer, &reference).unwrap();
    let b = tracker.pyramid(&renderer, &current).unwrap();
    assert_eq!(a.len(), 3);
    let found = tracker
        .track_multiscale(
            &renderer,
            &reference,
            &current,
            &a,
            &b,
            &points,
            Homography::IDENTITY,
            settings,
        )
        .unwrap();
    assert!(found.features.iter().filter(|f| f.accepted).count() >= 12);
    for p in surface {
        let actual = found.consensus.motion.project(p).unwrap();
        let expected = motion.project(p).unwrap();
        assert!(
            ((actual[0] - expected[0]) * f64::from(width)).abs() < 0.8,
            "{actual:?} != {expected:?}"
        );
        assert!(
            ((actual[1] - expected[1]) * f64::from(height)).abs() < 0.8,
            "{actual:?} != {expected:?}"
        );
    }
}
