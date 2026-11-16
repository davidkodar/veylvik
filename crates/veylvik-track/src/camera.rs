//! Footage to a 3D camera: follow features, solve the camera, and express it as scene nodes (an animated camera, a shadow-catching ground and a 3D render merged over the footage).
use crate::planar::PlanarTracker;
use crate::{Result, error, features};
use serde_json::{Value, json};
use veylvik_document::nodes::{self, MergeScenes, ShapeKind};
use veylvik_document::{Command, Composition, Interpolation, Node, NodeId, Operation, Time};
use veylvik_render::Renderer;
use veylvik_solve::linalg::*;
use veylvik_solve::{Pose, Solved, Track};

#[derive(Debug, Clone, Copy)]
pub struct Request {
    /// The footage node to solve.
    pub source: NodeId,
    pub frames: [i64; 2],
    /// The lens in millimetres when known; None finds it.
    pub focal_length: Option<f32>,
    pub sensor_width: f32,
}
impl Request {
    /// Arguments as the MCP tools give them: source, start, end, focal_length, sensor_width.
    pub fn from_json(args: &Value, frame_count: u32) -> Result<Self> {
        let source = args["source"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| error("source must be a node id"))?;
        let last = i64::from(frame_count.saturating_sub(1));
        let frame = |key: &str, default: i64| args[key].as_i64().unwrap_or(default);
        let frames = [frame("start", 0), frame("end", last)];
        if frames[0] < 0 || frames[1] > last || frames[1] - frames[0] < 2 {
            return Err(error(format!(
                "start and end must be at least two frames apart within 0..={last}"
            )));
        }
        if frames[1] - frames[0] >= 4096 {
            return Err(error("a camera solve is limited to 4096 frames"));
        }
        let mm = |key: &str| args[key].as_f64().map(|v| v as f32);
        let focal_length = mm("focal_length");
        let sensor_width = mm("sensor_width").unwrap_or(36.0);
        if focal_length.is_some_and(|f| !(1.0..=2000.0).contains(&f))
            || !(1.0..=100.0).contains(&sensor_width)
        {
            return Err(error(
                "focal_length must be 1..2000 mm and sensor_width 1..100 mm",
            ));
        }
        Ok(Self {
            source,
            frames,
            focal_length,
            sensor_width,
        })
    }
}
/// The solved shot and how many tracks fed it.
pub struct Outcome {
    pub solved: Solved,
    pub tracks: usize,
    pub width: u32,
    pub height: u32,
}
/// Follow features through the footage and solve its camera.
pub fn analyse(
    renderer: &Renderer,
    plan: &veylvik_render::Plan,
    inputs: &veylvik_render::ImageInputs,
    request: &Request,
    rate: Time,
    progress: &mut features::Progress<'_>,
) -> Result<Outcome> {
    let tracker = PlanarTracker::new(renderer);
    let found = features::track(
        &tracker,
        renderer,
        plan,
        inputs,
        request.frames,
        rate,
        features::Settings::default(),
        progress,
    )?;
    let tracks: Vec<Track> = found
        .tracks
        .iter()
        .map(|t| Track { points: t.clone() })
        .collect();
    let focal = request
        .focal_length
        .map(|mm| f64::from(mm) * f64::from(found.width) / f64::from(request.sensor_width));
    let solved = veylvik_solve::solve(
        &tracks,
        found.frames,
        &veylvik_solve::Settings {
            width: found.width,
            height: found.height,
            focal,
        },
    )
    .map_err(|e| error(e.to_string()))?;
    Ok(Outcome {
        solved,
        tracks: tracks.len(),
        width: found.width,
        height: found.height,
    })
}
/// The dominant plane below the first camera: (normal towards the cameras, a point
/// on it, the points on it), when enough points lie on one.
fn ground(solved: &Solved, first: &Pose) -> Option<(V3, V3, Vec<V3>)> {
    let points: Vec<V3> = solved.points.iter().flatten().copied().collect();
    if points.len() < 30 || solved.nodal {
        return None;
    }
    let eye = first.centre();
    let up = mat_vec(&transpose(&first.r), [0.0, -1.0, 0.0]);
    let mut depths: Vec<f64> = points.iter().map(|p| first.apply(*p)[2]).collect();
    depths.sort_by(f64::total_cmp);
    let tolerance = depths[depths.len() / 2] * 0.01;
    let mut rng = Rng::new(0x9e0);
    let mut best: Option<(usize, V3, V3)> = None;
    for _ in 0..400 {
        let [a, b, c] = [0; 3].map(|_| points[rng.below(points.len())]);
        let n = cross(sub(b, a), sub(c, a));
        if norm(n) < 1e-12 {
            continue;
        }
        let mut n = normalize(n);
        if dot(n, sub(eye, a)) < 0.0 {
            n = scale(n, -1.0);
        }
        // A ground faces up, within 50 degrees, and lies below the camera.
        if dot(n, up) < 50f64.to_radians().cos() || dot(n, sub(eye, a)) <= 0.0 {
            continue;
        }
        let count = points
            .iter()
            .filter(|p| dot(n, sub(**p, a)).abs() < tolerance)
            .count();
        if best.as_ref().is_none_or(|(k, ..)| count > *k) {
            best = Some((count, n, a));
        }
    }
    let (count, n, a) = best?;
    if count < 20 || count * 100 < points.len() * 10 {
        return None;
    }
    let on: Vec<V3> = points
        .iter()
        .copied()
        .filter(|p| dot(n, sub(*p, a)).abs() < tolerance)
        .collect();
    // Refit the plane through its points: centroid and the smallest spread direction.
    let centre = scale(
        on.iter().fold([0.0; 3], |s, p| add(s, *p)),
        1.0 / on.len() as f64,
    );
    let mut cov = vec![0.0; 9];
    for p in &on {
        let d = sub(*p, centre);
        for i in 0..3 {
            for j in 0..3 {
                cov[i * 3 + j] += d[i] * d[j];
            }
        }
    }
    let (_, vectors) = symmetric_eigen(cov, 3);
    let mut normal = normalize([vectors[0], vectors[3], vectors[6]]);
    if dot(normal, n) < 0.0 {
        normal = scale(normal, -1.0);
    }
    Some((normal, centre, on))
}
/// Degrees (x, y, z) such that Rz·Ry·Rx = `r`, nearest to `near` turn for turn.
fn euler(r: &M3, near: [f64; 3]) -> [f64; 3] {
    let y = (-r[2][0]).clamp(-1.0, 1.0).asin();
    let x = r[2][1].atan2(r[2][2]);
    let z = r[1][0].atan2(r[0][0]);
    let mut out = [x, y, z].map(f64::to_degrees);
    for k in 0..3 {
        out[k] += ((near[k] - out[k]) / 360.0).round() * 360.0;
    }
    out
}
/// A node type's registered default operation.
fn default_of(kind: &veylvik_document::registry::NodeType) -> Result<Operation> {
    serde_json::from_value(kind.default_operation()).map_err(|e| error(e.to_string()))
}
/// Mirror z: the solver's world is right-handed with z forward, Veylvik's has z towards the viewer.
const MIRROR: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
/// What [`scene`] builds.
pub struct Built {
    pub commands: Vec<Command>,
    pub report: Value,
    /// The Merge scenes node that new 3D objects go into.
    pub scene: NodeId,
    /// Each track's point in the composition's 3D space, for guides.
    pub points: Vec<Option<[f32; 3]>>,
}
/// Scene nodes for a solve of `request` on `doc`: the animated camera, a ground that
/// catches shadows, a Render scene of them and a Merge of that render over the
/// footage. The output is left alone.
pub fn scene(doc: &Composition, request: &Request, outcome: &Outcome) -> Result<Built> {
    let solved = &outcome.solved;
    let (w, h) = (f64::from(doc.width), f64::from(doc.height));
    let first_frame = solved
        .poses
        .iter()
        .position(Option::is_some)
        .ok_or_else(|| error("No frame was solved"))?;
    let first = solved.poses[first_frame].ok_or_else(|| error("No frame was solved"))?;
    // The solve's pixels are the footage's; the composition may differ in size.
    let focal_mm = solved.focal * f64::from(request.sensor_width) / f64::from(outcome.width);
    let distance = f64::from(veylvik_scene::home_distance(
        doc.width,
        focal_mm as f32,
        request.sensor_width,
    ));
    let home = [w * 0.5, h * 0.5, distance];
    // World alignment: level with the ground (when found), facing the first camera's way.
    let floor = ground(solved, &first);
    let align: M3 = match &floor {
        Some((normal, ..)) => {
            let down = scale(*normal, -1.0);
            let forward = mat_vec(&transpose(&first.r), [0.0, 0.0, 1.0]);
            let level = normalize(sub(forward, scale(down, dot(forward, down))));
            [cross(down, level), down, level]
        }
        None => first.r,
    };
    // Scale: the median depth seen from the first camera lands at the home distance.
    let mut depths: Vec<f64> = solved
        .points
        .iter()
        .flatten()
        .map(|p| first.apply(*p)[2])
        .filter(|d| *d > 0.0)
        .collect();
    depths.sort_by(f64::total_cmp);
    let size = depths
        .get(depths.len() / 2)
        .copied()
        .unwrap_or(1.0)
        .max(1e-9);
    let s = distance / size;
    let eye = first.centre();
    let place = |x: V3| {
        add(
            mat_vec(&MIRROR, scale(mat_vec(&align, sub(x, eye)), s)),
            home,
        )
    };
    // The camera, keyed on every solved frame.
    let Operation::Camera3d(mut camera) = default_of(&nodes::CAMERA_3D)? else {
        return Err(error("camera default"));
    };
    camera.focal_length = (focal_mm as f32).into();
    camera.sensor_width = request.sensor_width.into();
    let mut turn = [0.0; 3];
    let mut keyed = 0;
    for (k, pose) in solved.poses.iter().enumerate() {
        let Some(pose) = pose else { continue };
        let at = Time::at_frame(request.frames[0] + k as i64, doc.frame_rate)
            .map_err(|e| error(e.to_string()))?;
        let rotation = mat_mul(
            &mat_mul(&MIRROR, &mat_mul(&align, &transpose(&pose.r))),
            &MIRROR,
        );
        turn = euler(&rotation, turn);
        let position = sub(place(pose.centre()), home);
        let f32s = |v: [f64; 3]| v.map(|x| x as f32);
        camera
            .placement
            .position
            .set_key_at(at, f32s(position))
            .map_err(|e| error(e.to_string()))?;
        camera
            .placement
            .rotation
            .set_key_at(at, f32s(turn))
            .map_err(|e| error(e.to_string()))?;
        keyed += 1;
    }
    camera
        .placement
        .position
        .set_interpolation(Interpolation::Linear);
    camera
        .placement
        .rotation
        .set_interpolation(Interpolation::Linear);
    let camera = Node::new("Solved camera", Operation::Camera3d(camera), &[]);
    let mut commands = Vec::new();
    let mut parts = vec![("scene", camera.id)];
    let mut ground_report = Value::Null;
    if let Some((_, centre, on)) = &floor {
        let placed: Vec<V3> = on.iter().map(|p| place(*p)).collect();
        let c = place(*centre);
        let spread = placed
            .iter()
            .map(|p| (p[0] - c[0]).hypot(p[2] - c[2]))
            .fold(0.0, f64::max);
        let extent = (spread * 3.0).max(w * 4.0) as f32;
        let Operation::Shape3d(mut plane) = default_of(&nodes::SHAPE_3D)? else {
            return Err(error("shape default"));
        };
        plane.kind = ShapeKind::Plane;
        plane.size_x = extent.into();
        plane.size_y = extent.into();
        plane.shadow_catcher = true;
        plane.placement.position = [
            (c[0] - w * 0.5) as f32,
            (c[1] - h * 0.5) as f32,
            c[2] as f32,
        ]
        .into();
        plane.placement.rotation = [90.0, 0.0, 0.0].into();
        let plane = Node::new("Ground (shadow catcher)", Operation::Shape3d(plane), &[]);
        parts.push(("scene_2", plane.id));
        ground_report = json!({"node": plane.id, "points": on.len(), "height": c[1]});
        commands.push(Command::Put {
            node: Box::new(plane),
        });
    }
    let merged = Node::new(
        "Solved scene",
        Operation::MergeScenes(MergeScenes {}),
        &parts,
    );
    let Operation::RenderScene(mut render) = default_of(&nodes::RENDER_SCENE)? else {
        return Err(error("render default"));
    };
    render.camera = 1.0.into();
    let render = Node::new(
        "Render solved scene",
        Operation::RenderScene(render),
        &[("scene", merged.id)],
    );
    let over = Node::new(
        "3D over footage",
        Operation::Merge(nodes::Merge {}),
        &[("background", request.source), ("foreground", render.id)],
    );
    let worst = solved
        .frame_error
        .iter()
        .flatten()
        .fold(0.0_f64, |a, b| a.max(*b));
    let report = json!({
        "frames": solved.poses.len(), "solved_frames": keyed, "tracks": outcome.tracks,
        "points": solved.points.iter().flatten().count(), "rms_pixels": solved.rms, "worst_frame_pixels": worst,
        "focal_length": focal_mm, "sensor_width": request.sensor_width, "tripod": solved.nodal, "lens_measured": solved.focal_measured,
        "camera": camera.id, "ground": ground_report, "scene": merged.id, "render": render.id, "composite": over.id,
        "next": "Add 3D objects to the Solved scene node and view 3D over footage",
    });
    let scene = merged.id;
    commands.push(Command::Put {
        node: Box::new(camera),
    });
    commands.push(Command::Put {
        node: Box::new(merged),
    });
    commands.push(Command::Put {
        node: Box::new(render),
    });
    commands.push(Command::Put {
        node: Box::new(over),
    });
    let points = solved
        .points
        .iter()
        .map(|p| p.map(|p| place(p).map(|v| v as f32)))
        .collect();
    Ok(Built {
        commands,
        report,
        scene,
        points,
    })
}
