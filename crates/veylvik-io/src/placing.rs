//! Standing a 3D object on the ground of a shot: from a point in the picture, through the camera that films the scene, to the shadow catcher there, so nobody has to do the ray geometry by hand.
use crate::Result;
use serde_json::{Value, json};
use veylvik_document::{Command, Composition, NodeId, Operation, Time};
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
/// The commands that stand the node on the ground, and a report.
pub fn stand(doc: &Composition, args: &Value) -> Result<(Vec<Command>, Value)> {
    let (render, target) = (node_id(args, "render")?, node_id(args, "node")?);
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
    let pixel = [x * doc.width as f32, y * doc.height as f32];
    let (eye, direction) = pick::ray(&camera, pixel, doc.width, doc.height);
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
