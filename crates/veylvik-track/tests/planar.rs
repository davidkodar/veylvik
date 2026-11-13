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
        refine: false,
        grid: 8,
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
                        motion: None,
                    },
                    Sample {
                        time: Time::new(1, 1).unwrap(),
                        corners: reference.map(|[x, y]| [x + 0.125, y]),
                        features: vec![],
                        rms_pixels: 0.,
                        motion: None,
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
        refine: false,
        grid: 8,
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

#[test]
#[ignore = "requires Vulkan"]
fn area_alignment_reaches_sub_tenth_pixel_through_brightness_change_and_occluder() {
    let renderer = Renderer::new().unwrap();
    let tracker = PlanarTracker::new(&renderer);
    let (width, height) = (240_u32, 320_u32);
    let surface = [[0.15, 0.15], [0.85, 0.15], [0.85, 0.85], [0.15, 0.85]];
    let truth = Homography::between(
        surface,
        [[0.17, 0.13], [0.86, 0.17], [0.84, 0.86], [0.13, 0.82]],
    )
    .unwrap();
    let texture = |x: f64, y: f64| -> f32 {
        (0.5 + 0.13 * (x * 0.31 + y * 0.07).sin()
            + 0.12 * (y * 0.43 - x * 0.11).cos()
            + 0.1 * (x * 0.19 + y * 0.23).sin()
            + 0.08 * (x * 0.09 - y * 0.17).cos()) as f32
    };
    let mut doc = veylvik_document::blank(width, height, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        });
    let plan = Plan::compile(&doc).unwrap();
    let make = |motion: Homography, gain: f32, occlude: bool| {
        let inverse = motion.inverse().unwrap();
        let pixels = (0..width * height)
            .map(|i| {
                let (x, y) = (f64::from(i % width), f64::from(i / width));
                let p = inverse
                    .project([(x + 0.5) / f64::from(width), (y + 0.5) / f64::from(height)])
                    .unwrap();
                let v = if occlude && x > 90. && x < 140. && y > 120. && y < 180. {
                    0.9
                } else {
                    gain * texture(
                        p[0] * f64::from(width) - 0.5,
                        p[1] * f64::from(height) - 0.5,
                    )
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
    let error = |h: Homography| {
        surface
            .iter()
            .map(|&p| {
                let (a, b) = (h.project(p).unwrap(), truth.project(p).unwrap());
                (((a[0] - b[0]) * f64::from(width)).powi(2)
                    + ((a[1] - b[1]) * f64::from(height)).powi(2))
                .sqrt()
            })
            .fold(0., f64::max)
    };
    let reference = make(Homography::IDENTITY, 1., false);
    // Start 1.5 pixels away from the truth, as a slightly wrong feature solution would.
    let nudge = Homography {
        matrix: [
            1.,
            0.,
            1.5 / f64::from(width),
            0.,
            1.,
            -1.0 / f64::from(height),
            0.,
            0.,
            1.,
        ],
    };
    let start = nudge.compose(truth);
    assert!(error(start) > 1.2);
    for (gain, occlude) in [(1.0, false), (1.3, false), (1.0, true)] {
        let current = make(truth, gain, occlude);
        let refined = tracker
            .align_area(&renderer, &reference, &current, None, surface, start, true)
            .unwrap()
            .unwrap_or_else(|| panic!("alignment rejected (gain {gain}, occluder {occlude})"));
        assert!(
            error(refined) < 0.1,
            "gain {gain} occluder {occlude}: {:.3} px",
            error(refined)
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn planar_warp_hides_the_part_of_a_layer_behind_the_camera() {
    use veylvik_document::{
        Node,
        planar::{Homography, Sample, Track, WarpMode},
    };
    let renderer = Renderer::new().unwrap();
    // Output pixel (0.5, 0.8) sees the layer point (0.5, 0.2) through a negative homogeneous
    // coordinate: on one side of this motion's horizon that point is behind the camera.
    let motion = Homography {
        matrix: [-1.5, 2.5, 1.0, 0.0, 1.0, 1.0, 0.0, 2.5, 1.0],
    };
    let render = |area_centre: [f64; 2]| {
        let mut doc = veylvik_document::blank(64, 64, Time::new(24, 1).unwrap()).unwrap();
        let layer = doc.output;
        doc.nodes.get_mut(&layer).unwrap().operation =
            Operation::Solid(veylvik_document::nodes::Solid {
                color: [1., 1., 1., 1.],
            });
        let [cx, cy] = area_centre;
        let reference = [
            [cx - 0.1, cy - 0.1],
            [cx + 0.1, cy - 0.1],
            [cx + 0.1, cy + 0.1],
            [cx - 0.1, cy + 0.1],
        ];
        let corners = reference.map(|p| {
            motion
                .project(p)
                .map_or([0., 0.], |q| q.map(|v| v.clamp(-1e6, 1e6)))
        });
        let sample = Sample {
            time: Time::new(0, 1).unwrap(),
            corners,
            features: vec![],
            rms_pixels: 0.,
            motion: Some(motion),
        };
        let data = Track {
            reference,
            reference_time: Time::new(0, 1).unwrap(),
            region: None,
            samples: vec![sample],
        };
        data.validate().unwrap();
        let track = Node::new(
            "Track",
            Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack {
                data: Box::new(data),
            }),
            &[("image", layer)],
        );
        let track_id = track.id;
        doc.nodes.insert(track.id, track);
        let warp = Node::new(
            "Warp",
            Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp {
                mode: WarpMode::Follow,
            }),
            &[("image", layer), ("track", track_id)],
        );
        doc.output = warp.id;
        doc.nodes.insert(warp.id, warp);
        let plan = Plan::compile(&doc).unwrap();
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap()
    };
    let ghost = (0.8 * 64.) as usize * 64 + 32;
    // Tracked area on the far side of the horizon: that pixel would be a mirrored ghost.
    assert_eq!(render([-1., -1.]).pixels[ghost][3], 0.);
    // Tracked area on that pixel's side: the layer is really there and stays visible.
    assert!(render([0.5, 0.2]).pixels[ghost][3] > 0.99);
}
