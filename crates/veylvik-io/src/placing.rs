//! Standing a 3D object on the ground of a shot: from a point in the picture, through the camera that films the scene, to the shadow catcher there, so nobody has to do the ray geometry by hand.
use crate::Result;
use serde_json::{Value, json};
use veylvik_document::{Command, Composition, NodeId, Operation, Time};
use veylvik_scene::Scene;
use veylvik_scene::pick;

fn node_id(args: &Value, name: &str) -> Result<NodeId> {
    args[name]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| format!("{name} must be a node id").into())
}
/// A node's placement field as three plain numbers; an error when it is animated.
fn vector(operation: &Value, field: &str) -> Result<[f32; 3]> {
    serde_json::from_value(operation["placement"][field].clone()).map_err(|_| {
        format!(
            "the object's {field} is animated or missing: stand_on_ground needs a plain {field}"
        )
        .into()
    })
}
/// What a Render scene node films at a frame, and the ray through a point of the picture.
struct Filmed {
    scene: Scene,
    camera: veylvik_scene::Camera,
    eye: [f32; 3],
    direction: [f32; 3],
    frame: i64,
    time: Time,
}
/// The scene a Render scene node films at `frame`, its camera, and the ray through the
/// point `x`, `y` (0..1 of the frame).
fn filmed(doc: &Composition, args: &Value) -> Result<Filmed> {
    let render = node_id(args, "render")?;
    let number = |name: &str| {
        args[name]
            .as_f64()
            .ok_or(format!("{name} must be a number"))
    };
    let (x, y) = (number("x")? as f32, number("y")? as f32);
    let frame = args.get("frame").and_then(Value::as_i64).unwrap_or(0);
    let time = Time::at_frame(frame, doc.frame_rate)?;
    let film = doc
        .nodes
        .get(&render)
        .ok_or("the Render scene node does not exist")?;
    let Operation::RenderScene(settings) = &film.operation else {
        return Err("render must be a Render scene node".into());
    };
    let root = *film
        .inputs
        .get("scene")
        .ok_or("the Render scene has no scene")?;
    let scene = veylvik_scene::evaluate(doc, root, time)?;
    let index = settings.camera.at(time)?.round().clamp(0.0, 64.0) as usize;
    let camera = scene.camera(index, doc.width, doc.height);
    let (eye, direction) = pick::ray(
        &camera,
        [x * doc.width as f32, y * doc.height as f32],
        doc.width,
        doc.height,
    );
    Ok(Filmed {
        scene,
        camera,
        eye,
        direction,
        frame,
        time,
    })
}
/// The commands that move a light so its reflection shows on the surface at a point in
/// the picture, and a report: what Studio's Shine here does, from a script.
pub fn shine(doc: &Composition, args: &Value) -> Result<(Vec<Command>, Value)> {
    let target = node_id(args, "light")?;
    let Filmed {
        scene,
        eye,
        direction,
        frame,
        time,
        ..
    } = filmed(doc, args)?;
    let hit = pick::hit(&scene, eye, direction)?
        .ok_or("there is no surface at that point to reflect the light")?;
    let distance = args
        .get("distance")
        .and_then(Value::as_f64)
        .map(|d| d as f32);
    let shine = pick::shine(&scene, target, eye, &hit, distance)
        .ok_or("that light is not in the scene this Render scene films")?;
    let mut node = doc
        .nodes
        .get(&target)
        .ok_or("the light does not exist")?
        .clone();
    let Operation::Light3d(light) = &mut node.operation else {
        return Err("light must be a Light node".into());
    };
    // Moved by the difference in the world: exact for a light not inside a turned or
    // scaled 3D transform. An Aim at would turn it away again, so it is let go.
    let local = light.placement.position.at(time)?;
    let position = [0, 1, 2].map(|k| local[k] + shine.place[k] - shine.now[k]);
    light.placement.position.set_at(time, position)?;
    light.placement.rotation.set_at(time, shine.rotation)?;
    let aimed = light.aim_at.take().is_some();
    let report = json!({"light":target,"surface":hit.node,"point":hit.point,"position":position,"rotation":shine.rotation,
        "distance":(0..3).map(|k| (shine.place[k] - hit.point[k]).powi(2)).sum::<f32>().sqrt(),"aim_cleared":aimed,"frame":frame});
    Ok((
        vec![Command::Put {
            node: Box::new(node),
        }],
        report,
    ))
}
/// The commands that stand the node on the ground, and a report.
pub fn stand(doc: &Composition, args: &Value) -> Result<(Vec<Command>, Value)> {
    let target = node_id(args, "node")?;
    let Filmed {
        scene,
        camera,
        eye,
        direction,
        frame,
        time,
    } = filmed(doc, args)?;
    let ground = pick::hit_ground(&scene, eye, direction)?.ok_or(
        "no ground under that point: the scene needs a shadow catcher there (a camera solve adds one)",
    )?;
    let mut node = doc
        .nodes
        .get(&target)
        .ok_or("the object does not exist")?
        .clone();
    let mut operation = serde_json::to_value(&node.operation)?;
    let (mut position, mut scale) = (
        vector(&operation, "position")?,
        vector(&operation, "scale")?,
    );
    // The object as it stands now, alone: its box in the world.
    let boxed = |node: &veylvik_document::Node| -> Result<([f32; 3], [f32; 3])> {
        let mut alone = doc.clone();
        alone.nodes.insert(node.id, node.clone());
        pick::bounds(&veylvik_scene::evaluate(&alone, node.id, time)?)
            .ok_or_else(|| "the object has nothing solid to stand".into())
    };
    let rebuilt =
        |operation: &Value| -> Result<Operation> { Ok(serde_json::from_value(operation.clone())?) };
    let mut grown = 1.0;
    if let Some(tall) = args.get("height").and_then(Value::as_f64) {
        let (low, high) = boxed(&node)?;
        let focal =
            veylvik_scene::home_distance(doc.width, camera.focal_length, camera.sensor_width);
        // Along the view, not along the ray: that is what the picture's scale follows.
        let view = veylvik_scene::math::rigid_inverse(&camera.world);
        let depth = -veylvik_scene::math::point(&view, ground.point)[2];
        grown = tall as f32 * depth / focal / (high[1] - low[1]).max(1e-3);
        scale = scale.map(|s| s * grown);
        operation["placement"]["scale"] = json!(scale);
        node.operation = rebuilt(&operation)?;
    }
    let (low, high) = boxed(&node)?;
    let base = [(low[0] + high[0]) * 0.5, high[1], (low[2] + high[2]) * 0.5];
    for k in 0..3 {
        position[k] += ground.point[k] - base[k];
    }
    operation["placement"]["position"] = json!(position);
    node.operation = rebuilt(&operation)?;
    let report = json!({"node":target,"point":ground.point,"ground":ground.node,"distance":ground.distance,"position":position,"scale":scale,"scaled_by":grown,"frame":frame});
    Ok((
        vec![Command::Put {
            node: Box::new(node),
        }],
        report,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, nodes, registry};
    fn op(kind: &registry::NodeType) -> Operation {
        serde_json::from_value(kind.default_operation()).unwrap()
    }
    /// A camera at home, a wall facing it at the frame's depth, an area light off to the
    /// side, filmed by a Render scene: the document and the light's and render's ids.
    fn studio() -> (Composition, NodeId, NodeId) {
        let mut doc = veylvik_document::blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
        let add = |doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]| {
            let node = Node::new("n", op, inputs);
            let id = node.id;
            doc.nodes.insert(id, node);
            id
        };
        let camera = add(&mut doc, op(&nodes::CAMERA_3D), &[]);
        let mut wall = op(&nodes::SHAPE_3D);
        if let Operation::Shape3d(w) = &mut wall {
            w.kind = nodes::ShapeKind::Plane;
            w.size_x = 3000.0.into();
            w.size_y = 3000.0.into();
        }
        let wall = add(&mut doc, wall, &[]);
        let mut light = op(&nodes::LIGHT_3D);
        if let Operation::Light3d(l) = &mut light {
            l.kind = nodes::LightKind::Area;
            l.placement.position = [900.0, -400.0, 700.0].into();
        }
        let light = add(&mut doc, light, &[]);
        let merged = add(
            &mut doc,
            op(&nodes::MERGE_SCENES),
            &[("scene", camera), ("scene_2", wall), ("scene_3", light)],
        );
        let render = add(&mut doc, op(&nodes::RENDER_SCENE), &[("scene", merged)]);
        doc.output = render;
        (doc, light, render)
    }
    #[test]
    fn a_light_moves_to_where_a_surface_mirrors_the_camera_at_the_point_asked() {
        let (doc, light, render) = studio();
        // The middle of the frame: the wall faces the camera there, so its reflection
        // is straight back along the view, and the light faces straight into the scene.
        let (commands, report) = shine(&doc, &json!({"render": render.to_string(), "light": light.to_string(), "x": 0.5, "y": 0.5, "distance": 800})).unwrap();
        let [Command::Put { node }] = &commands[..] else {
            panic!("one Put: {commands:?}")
        };
        let Operation::Light3d(l) = &node.operation else {
            panic!()
        };
        let p = l.placement.position.at(Time::new(0, 1).unwrap()).unwrap();
        let r = l.placement.rotation.at(Time::new(0, 1).unwrap()).unwrap();
        assert!(
            p[0].abs() < 1.0 && p[1].abs() < 1.0 && (p[2] - 800.0).abs() < 1.0,
            "on the view's axis, 800 out: {p:?}"
        );
        assert!(r.iter().all(|v| v.abs() < 0.1), "facing straight in: {r:?}");
        assert_eq!(report["distance"].as_f64().map(|d| d.round()), Some(800.0));
        // Up and to the left of the middle: the reflection there comes from up and left,
        // and with no distance given the light keeps its distance from the spot.
        let (commands, report) = shine(
            &doc,
            &json!({"render": render.to_string(), "light": light.to_string(), "x": 0.3, "y": 0.3}),
        )
        .unwrap();
        let [Command::Put { node }] = &commands[..] else {
            panic!()
        };
        let Operation::Light3d(l) = &node.operation else {
            panic!()
        };
        let p = l.placement.position.at(Time::new(0, 1).unwrap()).unwrap();
        assert!(
            p[0] < -100.0 && p[1] < -100.0 && p[2] > 0.0,
            "up and to the left, in front: {p:?}"
        );
        let spot: Vec<f32> = serde_json::from_value(report["point"].clone()).unwrap();
        let root = doc.nodes[&render].inputs["scene"];
        let before = veylvik_scene::evaluate(&doc, root, Time::new(0, 1).unwrap()).unwrap();
        let was = before
            .lights
            .iter()
            .find(|l| l.node == light)
            .unwrap()
            .position();
        let away = (0..3)
            .map(|k| (was[k] - spot[k]).powi(2))
            .sum::<f32>()
            .sqrt();
        let now = report["distance"].as_f64().unwrap() as f32;
        assert!(
            (now - away).abs() < 1.0,
            "as far from the spot as before: {now} against {away}"
        );
    }
}
