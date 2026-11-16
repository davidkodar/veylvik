use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{Image, ImageInputs, Plan, Renderer};
use veylvik_solve::linalg::*;
use veylvik_solve::{Pose, Settings, Track};
use veylvik_track::{features, planar::PlanarTracker};

const W: u32 = 640;
const H: u32 = 360;
const FRAMES: usize = 40;
fn add(doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new(op.descriptor().label, op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
/// Blocky value noise: crisp corners everywhere, like real texture.
fn texture(seed: u64) -> Image {
    let mut rng = Rng::new(seed);
    let coarse: Vec<f32> = (0..24 * 24)
        .map(|_| 0.1 + 0.8 * rng.unit() as f32)
        .collect();
    let fine: Vec<f32> = (0..64 * 64)
        .map(|_| (rng.unit() as f32 - 0.5) * 0.3)
        .collect();
    let size = 512;
    let pixels = (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            let v = (coarse[(y * 24 / size) * 24 + x * 24 / size]
                + fine[(y * 64 / size) * 64 + x * 64 / size])
                .clamp(0.02, 1.0);
            [v, v * 0.9, v * 0.8, 1.0]
        })
        .collect();
    Image::new(size as u32, size as u32, pixels).unwrap()
}
/// Rotation angle between solved and true orientations, each relative to frame 0.
fn rotation_error(solved: &[Option<Pose>], truth: &[Pose]) -> f64 {
    let (s0, t0) = (solved[0].unwrap().r, truth[0].r);
    solved
        .iter()
        .zip(truth)
        .filter_map(|(s, t)| {
            let rs = mat_mul(&s.as_ref()?.r, &transpose(&s0));
            let rt = mat_mul(&t.r, &transpose(&t0));
            Some(norm(log_so3(&mat_mul(&rs, &transpose(&rt)))).to_degrees())
        })
        .fold(0.0, f64::max)
}
/// Worst camera position error after the best similarity alignment, as a share of the path.
fn path_error(solved: &[Option<Pose>], truth: &[Pose]) -> f64 {
    let pairs: Vec<(V3, V3)> = solved
        .iter()
        .zip(truth)
        .filter_map(|(s, t)| Some((s.as_ref()?.centre(), t.centre())))
        .collect();
    let mean = |pick: fn(&(V3, V3)) -> V3| {
        scale(
            pairs.iter().map(pick).fold([0.0; 3], add_v),
            1.0 / pairs.len() as f64,
        )
    };
    let (ms, mt) = (mean(|p| p.0), mean(|p| p.1));
    let centred: Vec<(V3, V3)> = pairs
        .iter()
        .map(|(s, t)| (sub(*s, ms), sub(*t, mt)))
        .collect();
    let r = best_rotation(centred.iter().map(|(s, t)| (*s, *t, 1.0)));
    let k = centred
        .iter()
        .map(|(s, t)| dot(mat_vec(&r, *s), *t))
        .sum::<f64>()
        / centred.iter().map(|(s, _)| dot(*s, *s)).sum::<f64>();
    let length = norm(sub(truth[truth.len() - 1].centre(), truth[0].centre())).max(1e-9);
    centred
        .iter()
        .map(|(s, t)| norm(sub(scale(mat_vec(&r, *s), k), *t)))
        .fold(0.0, f64::max)
        / length
}
fn add_v(a: V3, b: V3) -> V3 {
    veylvik_solve::linalg::add(a, b)
}

#[test]
#[ignore = "requires Vulkan"]
fn tracks_rendered_footage_and_solves_its_camera() {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    let rate = Time::new(24, 1).unwrap();
    let mut doc = blank(W, H, rate).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert("a", Arc::new(texture(11)));
    inputs.insert("b", Arc::new(texture(23)));
    let image_a = add(
        &mut doc,
        Operation::Image(nodes::Image { asset: "a".into() }),
        &[],
    );
    let image_b = add(
        &mut doc,
        Operation::Image(nodes::Image { asset: "b".into() }),
        &[],
    );
    // A set of textured cards at many depths and angles: wall, floor, side panels, near cards.
    let cards = [
        (image_a, [0.0, 0.0, -600.0], [0.0, 0.0, 0.0], 3.0),
        (image_b, [0.0, 150.0, -150.0], [90.0, 0.0, 0.0], 3.0),
        (image_b, [-520.0, 0.0, -250.0], [0.0, 60.0, 0.0], 1.6),
        (image_a, [480.0, -20.0, -300.0], [0.0, -55.0, 0.0], 1.6),
        (image_b, [-140.0, -60.0, 250.0], [0.0, 20.0, 0.0], 0.3),
        (image_a, [170.0, 40.0, 320.0], [10.0, -25.0, 0.0], 0.25),
    ];
    let mut scene = Vec::new();
    for (image, position, rotation, size) in cards {
        let mut card = op(&IMAGE_CARD);
        if let Operation::ImageCard(c) = &mut card {
            c.placement.position = position.into();
            c.placement.rotation = rotation.into();
            c.placement.scale = [size, size, 1.0].into();
        }
        scene.push(add(&mut doc, card, &[("image", image)]));
    }
    let mut camera = op(&CAMERA_3D);
    if let Operation::Camera3d(c) = &mut camera {
        let (first, last) = (
            Time::at_frame(0, rate).unwrap(),
            Time::at_frame(FRAMES as i64 - 1, rate).unwrap(),
        );
        c.placement
            .position
            .set_key_at(first, [-150.0, -40.0, 0.0])
            .unwrap();
        c.placement
            .position
            .set_key_at(last, [150.0, 20.0, -150.0])
            .unwrap();
        c.placement
            .rotation
            .set_key_at(first, [-4.0, 8.0, 0.0])
            .unwrap();
        c.placement
            .rotation
            .set_key_at(last, [3.0, -8.0, 1.0])
            .unwrap();
        c.placement
            .position
            .set_interpolation(Interpolation::Linear);
        c.placement
            .rotation
            .set_interpolation(Interpolation::Linear);
    }
    scene.push(add(&mut doc, camera, &[]));
    let names: Vec<String> = (0..scene.len())
        .map(|i| {
            if i == 0 {
                "scene".into()
            } else {
                format!("scene_{}", i + 1)
            }
        })
        .collect();
    let ports: Vec<(&str, NodeId)> = names
        .iter()
        .map(String::as_str)
        .zip(scene.iter().copied())
        .collect();
    let merged = add(&mut doc, op(&MERGE_SCENES), &ports);
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.camera = 1.0.into();
    }
    doc.output = add(&mut doc, render, &[("scene", merged)]);
    let plan = Plan::compile(&doc).unwrap();
    let tracker = PlanarTracker::new(&renderer);
    let found = features::track(
        &tracker,
        &renderer,
        &plan,
        &inputs,
        [0, FRAMES as i64 - 1],
        rate,
        features::Settings::default(),
        &mut |_, _| true,
    )
    .unwrap();
    let lengths: Vec<usize> = found.tracks.iter().map(Vec::len).collect();
    eprintln!(
        "tracks {} mean length {:.1}",
        lengths.len(),
        lengths.iter().sum::<usize>() as f64 / lengths.len().max(1) as f64
    );
    assert!(found.tracks.len() >= 150, "{} tracks", found.tracks.len());
    // The true camera, in the solver's right-handed frame (Veylvik's z mirrored).
    let truth: Vec<Pose> = (0..FRAMES)
        .map(|f| {
            let at = Time::at_frame(f as i64, rate).unwrap();
            let s = veylvik_scene::evaluate(&doc, merged, at).unwrap();
            let m = s.cameras[0].world;
            let flip = |v: f64, k: usize| if k == 2 { -v } else { v };
            // Camera to world, rows i, columns j; mirrored on both sides.
            let mut r = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    r[j][i] = flip(flip(f64::from(m[i][j]), i), j);
                }
            }
            let c = [0, 1, 2].map(|k| flip(f64::from(m[k][3]), k));
            Pose {
                r,
                t: scale(mat_vec(&r, c), -1.0),
            }
        })
        .collect();
    let tracks: Vec<Track> = found
        .tracks
        .iter()
        .map(|t| Track { points: t.clone() })
        .collect();
    let focal = veylvik_scene::home_distance(W, 50.0, 36.0) as f64;
    for known in [Some(focal), None] {
        let solved = veylvik_solve::solve(
            &tracks,
            FRAMES,
            &Settings {
                width: W,
                height: H,
                focal: known,
            },
        )
        .unwrap();
        let (rot, path) = (
            rotation_error(&solved.poses, &truth),
            path_error(&solved.poses, &truth),
        );
        eprintln!(
            "focal {:?}: solved {:.1}, rms {:.3}, rotation {rot:.3} deg, path {path:.4}, nodal {}",
            known, solved.focal, solved.rms, solved.nodal
        );
        assert!(!solved.nodal);
        assert!(solved.poses.iter().all(Option::is_some));
        assert!(solved.rms < 0.3, "rms {}", solved.rms);
        assert!(rot < 0.2, "rotation {rot}");
        assert!(path < 0.005, "path {path}");
        assert!(
            (solved.focal - focal).abs() < focal * 0.02,
            "focal {}",
            solved.focal
        );
    }
    // The solve as scene nodes: the point cloud seen through the solved Camera node
    // must land on the tracked pixels, as the renderer would draw it.
    let request = veylvik_track::camera::Request {
        source: doc.output,
        frames: [0, FRAMES as i64 - 1],
        focal_length: None,
        sensor_width: 36.0,
    };
    let solved = veylvik_solve::solve(
        &tracks,
        FRAMES,
        &Settings {
            width: W,
            height: H,
            focal: None,
        },
    )
    .unwrap();
    let outcome = veylvik_track::camera::Outcome {
        grain: None,
        solved,
        tracks: tracks.len(),
        width: W,
        height: H,
    };
    let built = veylvik_track::camera::scene(&doc, &request, &outcome).unwrap();
    assert!(
        !built.report["ground"].is_null(),
        "the floor card is found as ground"
    );
    let mut project = Document::new(doc.clone()).unwrap();
    project.apply(project.revision(), &built.commands).unwrap();
    let (mut sum, mut n) = (0.0, 0usize);
    for f in (0..FRAMES).step_by(3) {
        let at = Time::at_frame(f as i64, rate).unwrap();
        let s = veylvik_scene::evaluate(project.snapshot(), built.scene, at).unwrap();
        let camera = s.cameras[0];
        for (track, point) in found.tracks.iter().zip(&built.points) {
            let (Some(point), Some((_, pixel))) = (point, track.iter().find(|(g, _)| *g == f))
            else {
                continue;
            };
            let Some((p, _)) = camera.project(*point, W, H) else {
                continue;
            };
            let e = (f64::from(p[0]) - pixel[0]).hypot(f64::from(p[1]) - pixel[1]);
            if e < 3.0 {
                sum += e * e;
                n += 1;
            }
        }
    }
    let rms = (sum / n.max(1) as f64).sqrt();
    eprintln!(
        "scene reprojection {rms:.3} px over {n} sightings; report {}",
        built.report
    );
    assert!(n > 500 && rms < 0.4, "scene reprojection {rms} over {n}");
}

#[test]
fn far_background_points_stay_out_of_the_point_cloud() {
    // A solve whose points include one at near infinity (distant sky, no parallax).
    let doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut points: Vec<Option<V3>> = (0..40)
        .map(|i| Some([(i % 8) as f64 - 4.0, 1.0, 6.0 + (i / 8) as f64]))
        .collect();
    points.push(Some([0.0, 0.0, 1.0e12]));
    let pose = |x: f64| Pose {
        r: IDENTITY,
        t: [-x, 0.0, 0.0],
    };
    let solved = veylvik_solve::Solved {
        poses: vec![Some(pose(0.0)), Some(pose(0.1)), Some(pose(0.2))],
        points,
        focal: 900.0,
        rms: 0.2,
        frame_error: vec![Some(0.2); 3],
        nodal: false,
        focal_measured: true,
        distortion: veylvik_solve::Distortion { k1: -0.1, k2: 0.0 },
        keyframes: vec![0, 1, 2],
    };
    let request = veylvik_track::camera::Request {
        source: doc.output,
        frames: [0, 2],
        focal_length: None,
        sensor_width: 36.0,
    };
    let outcome = veylvik_track::camera::Outcome {
        grain: None,
        solved,
        tracks: 41,
        width: W,
        height: H,
    };
    let built = veylvik_track::camera::scene(&doc, &request, &outcome).unwrap();
    let mut project = Document::new(doc).unwrap();
    project.apply(project.revision(), &built.commands).unwrap();
    let cloud = project
        .snapshot()
        .nodes
        .values()
        .find_map(|n| match &n.operation {
            Operation::PointCloud(c) => Some(c.points.len()),
            _ => None,
        })
        .unwrap();
    assert_eq!(cloud, 40);
    // The measured lens bends the CG before its grain goes on.
    let snapshot = project.snapshot();
    let lens = snapshot
        .nodes
        .values()
        .find(|n| matches!(&n.operation, Operation::LensDistortion(l) if l.redistort))
        .expect("a redistort node");
    let grain = snapshot
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::MatchGrain(_)))
        .unwrap();
    assert_eq!(grain.inputs.get("image"), Some(&lens.id));
}
