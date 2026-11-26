//! Authoring calls shared by the MCP servers: each turns plain arguments into document commands and a result, so the headless and the live server build exactly the same nodes.
use crate::particles::LOOKS;
use serde_json::{Value, json};
use veylvik_document::{
    Command, Composition, Error, Node, NodeId, NodePosition, Operation, Result, registry::TYPES,
};

/// What a call adds: the commands for one undo step and the reply's own fields.
pub struct Authored {
    pub commands: Vec<Command>,
    pub result: Value,
}
/// The tools as `(name, description, properties, required)`; servers add their own
/// `revision` (and `session`) and `dry_run`.
pub fn tools() -> Vec<(&'static str, &'static str, Value, &'static [&'static str])> {
    let id = json!({"type":"string","format":"uuid"});
    let kinds: Vec<&str> = TYPES.iter().map(|kind| kind.type_id).collect();
    let looks: Vec<&str> = LOOKS.iter().map(|look| look.name).collect();
    vec![
        (
            "surround_lights",
            "Add four broad softboxes around an existing Render scene for full-orbit reflections, as editable nodes. Existing lights and compositing are preserved. warm false gives neutral studio light; true gives warm product lighting. One undo step.",
            json!({"render":id,"warm":{"type":"boolean"}}),
            &["render"],
        ),
        (
            "add_node",
            "Add one node of a type from nodes and return its id. settings are written over the type's default settings (only name what differs, also inside a setting made of parts such as placement, timing or a Text's typography; scalars are {\"kind\":\"constant\",\"value\":N} or keyed curves as in nodes), inputs maps port names to existing node ids, and output true also makes the node the composition's output. One undo step.",
            json!({"type":{"enum":kinds},"label":{"type":"string","maxLength":256},"settings":{"type":"object"},"inputs":{"type":"object","additionalProperties":id},"position":veylvik_document::schema::position(),"output":{"type":"boolean"}}),
            &["type"],
        ),
        (
            "gold_title",
            "Add the Gold title scene, sized to the composition: brushed gold 3D lettering under studio strip lights on a dark wall, as ordinary nodes. It becomes the output and the display transform becomes Filmic, unless as_output is false: then it is only added, for a larger composition to place it (its look is made for Filmic; the display command sets that). The letters cast a shadow on the wall unless shadow is false (afterwards: the node Shadow on the wall, its Shadow strength or In the scene). Returns the output node, the Title node whose content holds the words, and the ids of the 3D letters and the camera: a Merge scenes of those two into a Render scene has alpha exactly on the letters, so it can be a Particles target for dust that gathers into the title. One undo step.",
            json!({"text":{"type":"string","minLength":1,"maxLength":256},"subtitle":{"type":"string","maxLength":256},"shadow":{"type":"boolean","default":true},"as_output":{"type":"boolean","default":true}}),
            &["text"],
        ),
        (
            "particle_look",
            "Give a Particles node a ready-made look, scaled to the composition: Sparks, Embers, Snow, Rain, Magic dust, Whirlpool, Confetti, Fireworks, Network and Murmuration need nothing; Disintegrate needs the emitter input and Assemble the target input connected. Replaces the node's settings except its seed, position and wiring. One undo step.",
            json!({"node":id,"look":{"enum":looks}}),
            &["node", "look"],
        ),
        // Carried out by the hosts themselves: it needs the 3D scene, which this
        // crate does not know.
        (
            "stand_on_ground",
            "Stand a 3D object on the ground of the shot at a point in the picture. render is the Render scene node that films it (its scene and camera are used); node is the object to move: a shape_3d, text_3d, model_3d or image_card, or a transform_3d holding several. x and y are the point in 0..1 of the frame (0, 0 is the top left) at frame (default 0). The ray through that point meets the nearest shadow catcher (the Ground a camera solve adds), and the object is moved so the middle of its base rests there. height, in pixels of the picture, also scales it to stand that tall at that distance. The node must sit directly in the scene, with no 3D transform above it, and its position and scale must not be animated. One undo step; returns the point and its distance.",
            json!({"render":id,"node":id,"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1},"frame":{"type":"integer","minimum":0},"height":{"type":"number","minimum":1,"maximum":100000}}),
            &["render", "node", "x", "y"],
        ),
        (
            "shine_here",
            "Move a light so its reflection shows on the surface at a point in the picture, as Studio's Shine here does: the way to light glossy or metal surfaces (gold, chrome, car paint) where you want them to gleam. render is the Render scene node that films it (its scene and camera are used); light is the Light node to move (an area light gives a soft band, a point light a sharp highlight); x and y are the point in 0..1 of the frame (0, 0 is the top left) at frame (default 0). The ray through that point meets the nearest surface; the light moves to where that surface mirrors the camera's view, distance pixels from it (default: as far as the light is now), and turns to face the spot. An Aim at on the light is cleared, since it would turn it away again. The light must not sit inside a turned or scaled 3D transform. A keyed position or rotation gets a key at that frame. One undo step; returns the surface hit and the light's new position and rotation.",
            json!({"render":id,"light":id,"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1},"frame":{"type":"integer","minimum":0},"distance":{"type":"number","minimum":50,"maximum":50000}}),
            &["render", "light", "x", "y"],
        ),
    ]
}
fn text<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args[name]
        .as_str()
        .ok_or_else(|| Error(format!("{name} must be a string")))
}
fn node_id(value: &Value, what: &str) -> Result<NodeId> {
    value
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| Error(format!("{what} must be a node id")))
}
/// The commands for `tool`, or `None` when it is not one of these tools.
pub fn build(composition: &Composition, tool: &str, args: &Value) -> Result<Option<Authored>> {
    Ok(Some(match tool {
        "add_node" => add_node(args)?,
        "surround_lights" => crate::surround_lights(
            composition,
            node_id(&args["render"], "render")?,
            args.get("warm").and_then(Value::as_bool).unwrap_or(false),
        )?,
        "gold_title" => {
            let subtitle = args.get("subtitle").and_then(Value::as_str).unwrap_or("");
            let shadow = args.get("shadow").and_then(Value::as_bool).unwrap_or(true);
            let as_output = args
                .get("as_output")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let mut built =
                crate::gold_title_with(composition, text(args, "text")?, subtitle, shadow)?;
            if !as_output {
                built
                    .commands
                    .retain(|c| !matches!(c, Command::Output { .. } | Command::Display { .. }));
            }
            // The 3D letters and the camera, so particles can be made to gather on the
            // letters: a Render scene of those two alone has alpha exactly where they show.
            let find = |kind: fn(&Operation) -> bool| {
                built.commands.iter().find_map(|c| match c {
                    Command::Put { node } if kind(&node.operation) => Some(node.id),
                    _ => None,
                })
            };
            let letters = find(|o| matches!(o, Operation::Text3d(_)));
            let camera = find(|o| matches!(o, Operation::Camera3d(_)));
            Authored {
                result: json!({"output":built.output,"title":built.edit,"letters":letters,"camera":camera,"added_nodes":built.commands.iter().filter(|c|matches!(c,Command::Put{..})).count()}),
                commands: built.commands,
            }
        }
        "particle_look" => {
            let id = node_id(&args["node"], "node")?;
            let name = text(args, "look")?;
            let look = LOOKS
                .iter()
                .find(|look| look.name == name)
                .ok_or_else(|| Error(format!("unknown look {name}")))?;
            let mut node = composition
                .nodes
                .get(&id)
                .ok_or_else(|| Error("node does not exist".into()))?
                .clone();
            if !matches!(node.operation, Operation::Particles(_)) {
                return Err(Error("node is not a Particles node".into()));
            }
            let emitter = node.inputs.contains_key("emitter");
            let Operation::Particles(particles) = &mut node.operation else {
                unreachable!("checked above")
            };
            look.apply_wired(particles, composition.width, composition.height, emitter);
            Authored {
                commands: vec![Command::Put {
                    node: Box::new(node),
                }],
                result: json!({"node":id,"look":look.name,"hint":look.hint}),
            }
        }
        _ => return Ok(None),
    }))
}
/// Writes `given` over `default`. A setting made of named parts (a placement, a
/// timing, a Text's animation) takes only the parts given and keeps the rest;
/// an animatable value (`kind` and its keys) and everything else is replaced whole.
fn merge(default: &mut Value, given: &Value) {
    match (default.as_object_mut(), given.as_object()) {
        (Some(parts), Some(new)) if !parts.contains_key("kind") && !new.contains_key("kind") => {
            for (key, value) in new {
                match parts.get_mut(key) {
                    Some(part) => merge(part, value),
                    None => {
                        parts.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        _ => *default = given.clone(),
    }
}
/// A shader's control value by its name (p1 to p16).
fn operation_scalar<'a>(
    shader: &'a mut veylvik_document::nodes::Shader,
    name: &str,
) -> Option<&'a mut veylvik_document::Scalar> {
    use veylvik_document::nodes::NodeOp;
    shader.scalar_mut(name)
}
fn add_node(args: &Value) -> Result<Authored> {
    let kind = text(args, "type")?;
    let kind = TYPES
        .iter()
        .find(|t| t.type_id == kind)
        .ok_or_else(|| Error(format!("unknown node type {kind}")))?;
    let mut operation = kind.default_operation();
    if let Some(settings) = args.get("settings") {
        let settings = settings
            .as_object()
            .ok_or_else(|| Error("settings must be an object".into()))?;
        let target = operation.as_object_mut().expect("defaults are objects");
        for (key, value) in settings {
            if key == "type" || !target.contains_key(key) {
                return Err(Error(format!("{} has no setting {key}", kind.type_id)));
            }
            merge(target.get_mut(key).expect("checked above"), value);
        }
    }
    let mut operation: Operation =
        serde_json::from_value(operation).map_err(|e| Error(format!("settings: {e}")))?;
    // A shader given its code starts each control the caller left alone at the value the
    // code declares, as Studio's shader editor does.
    if let Operation::Shader(shader) = &mut operation
        && let Ok(controls) = veylvik_shader::check(&shader.code)
    {
        let given = args.get("settings").and_then(Value::as_object);
        for (k, value) in veylvik_shader::defaults(&controls).into_iter().enumerate() {
            let name = format!("p{}", k + 1);
            if given.is_some_and(|g| g.contains_key(&name)) {
                continue;
            }
            if let Some(scalar) = operation_scalar(shader, &name) {
                *scalar = value.into();
            }
        }
    }
    let mut inputs = Vec::new();
    if let Some(given) = args.get("inputs") {
        for (port, source) in given
            .as_object()
            .ok_or_else(|| Error("inputs must be an object".into()))?
        {
            inputs.push((port.as_str(), node_id(source, "an input")?));
        }
    }
    let label = args
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or(kind.label);
    let mut node = Node::new(label, operation, &inputs);
    node.position = match args.get("position") {
        None | Some(Value::Null) => None,
        Some(position) => Some(
            serde_json::from_value::<NodePosition>(position.clone())
                .map_err(|e| Error(format!("position: {e}")))?,
        ),
    };
    let id = node.id;
    let mut commands = vec![Command::Put {
        node: Box::new(node),
    }];
    let output = args.get("output").and_then(Value::as_bool).unwrap_or(false);
    if output {
        commands.push(Command::Output { id });
    }
    Ok(Authored {
        commands,
        result: json!({"id":id,"type":kind.type_id,"output":output}),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Document, Time};
    fn document() -> Document {
        Document::new(veylvik_document::blank(1920, 1080, Time::new(30, 1).unwrap()).unwrap())
            .unwrap()
    }
    fn run(document: &mut Document, tool: &str, args: Value) -> Result<Value> {
        let authored = build(document.snapshot(), tool, &args)?.expect("a known tool");
        document.apply(document.revision(), &authored.commands)?;
        Ok(authored.result)
    }
    #[test]
    fn surround_rig_preserves_existing_scene_and_output() {
        let mut doc = document();
        run(&mut doc, "gold_title", json!({"text":"ORBIT"})).unwrap();
        let output = doc.snapshot().output;
        let render = doc
            .snapshot()
            .nodes
            .values()
            .find(|n| matches!(n.operation, Operation::RenderScene(_)))
            .unwrap()
            .id;
        let before = doc.snapshot().nodes[&render].inputs["scene"];
        let count = doc.snapshot().nodes.len();
        let reply = run(&mut doc, "surround_lights", json!({"render":render})).unwrap();
        let rig: NodeId = reply["rig"].as_str().unwrap().parse().unwrap();
        assert_eq!(doc.snapshot().output, output);
        assert_eq!(doc.snapshot().nodes[&render].inputs["scene"], rig);
        assert_eq!(doc.snapshot().nodes[&rig].inputs["scene"], before);
        assert_eq!(doc.snapshot().nodes.len(), count + 5);
        assert!(build(doc.snapshot(), "surround_lights", &json!({"render":output})).is_err());
    }
    #[test]
    fn a_node_is_added_from_its_defaults_with_the_named_settings_and_wiring() {
        let mut doc = document();
        let back = doc.snapshot().output;
        let blur = run(
            &mut doc,
            "add_node",
            json!({"type":"blur","settings":{"radius":{"kind":"constant","value":12.0}},"inputs":{"image":back},"output":true}),
        )
        .unwrap();
        let id: NodeId = blur["id"].as_str().unwrap().parse().unwrap();
        let after = doc.snapshot();
        assert_eq!(after.output, id);
        assert_eq!(after.nodes[&id].inputs["image"], back);
        assert_eq!(after.nodes[&id].label, "Blur");
        let wrong = build(
            after,
            "add_node",
            &json!({"type":"blur","settings":{"radiuss":1}}),
        );
        assert!(
            wrong
                .err()
                .is_some_and(|e| e.0.contains("no setting radiuss"))
        );
        assert!(build(after, "save", &json!({})).unwrap().is_none());
        // A setting made of parts takes the parts given and keeps the others.
        let text = build(
            after,
            "add_node",
            &json!({"type":"text","settings":{"typography":{"motion":"rise","start":2.0}}}),
        )
        .unwrap()
        .unwrap();
        let Command::Put { node } = &text.commands[0] else {
            panic!()
        };
        let stored = serde_json::to_value(&node.operation).unwrap();
        assert_eq!(stored["typography"]["motion"], "rise");
        assert!((stored["typography"]["stagger"].as_f64().unwrap() - 0.04).abs() < 1e-6);
        let moved = build(
            after,
            "add_node",
            &json!({"type":"shape_3d","settings":{"placement":{"position":[1.0, 2.0, 3.0]}}}),
        )
        .unwrap()
        .unwrap();
        let Command::Put { node } = &moved.commands[0] else {
            panic!()
        };
        let stored = serde_json::to_value(&node.operation).unwrap();
        assert_eq!(stored["placement"]["position"], json!([1.0, 2.0, 3.0]));
        assert_eq!(stored["placement"]["scale"], json!([1.0, 1.0, 1.0]));
    }
    #[test]
    fn a_look_restyles_particles_and_the_gold_title_becomes_the_output() {
        let mut doc = document();
        let node = run(&mut doc, "add_node", json!({"type":"particles"})).unwrap();
        let reply = run(
            &mut doc,
            "particle_look",
            json!({"node":node["id"],"look":"Sparks"}),
        )
        .unwrap();
        assert_eq!(reply["look"], "Sparks");
        let id: NodeId = node["id"].as_str().unwrap().parse().unwrap();
        let Operation::Particles(p) = &doc.snapshot().nodes[&id].operation else {
            panic!("particles")
        };
        assert_eq!(
            serde_json::to_value(&p.rate).unwrap(),
            json!({"kind":"constant","value":1500.0})
        );
        let title = run(&mut doc, "gold_title", json!({"text":"VEYLVIK"})).unwrap();
        let output: NodeId = title["output"].as_str().unwrap().parse().unwrap();
        assert_eq!(doc.snapshot().output, output);
        assert!(names().contains(&"gold_title"));
    }
    #[test]
    fn a_gold_title_added_into_a_larger_film_leaves_its_output_and_display_alone() {
        let mut doc = document();
        let before = doc.snapshot();
        let (output, display) = (before.output, before.display);
        let count = before.nodes.len();
        let title = run(
            &mut doc,
            "gold_title",
            json!({"text":"VEYLVIK","as_output":false}),
        )
        .unwrap();
        let after = doc.snapshot();
        assert_eq!((after.output, after.display), (output, display));
        let added: NodeId = title["output"].as_str().unwrap().parse().unwrap();
        assert!(after.nodes.contains_key(&added) && after.nodes.len() > count);
        let id = |key: &str| -> NodeId { title[key].as_str().unwrap().parse().unwrap() };
        assert!(matches!(
            after.nodes[&id("letters")].operation,
            Operation::Text3d(_)
        ));
        assert!(matches!(
            after.nodes[&id("camera")].operation,
            Operation::Camera3d(_)
        ));
    }
    #[test]
    fn a_shader_starts_at_its_codes_defaults_and_a_look_fits_the_wiring() {
        let mut doc = document();
        let code = "// slider amount 0 1 0.7\n// slider other 0 4 2.5\nfn shade(at: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(u.amount, u.other, 0.0, 1.0);\n}\n";
        let added = run(
            &mut doc,
            "add_node",
            json!({"type":"shader","settings":{"code":code,"p2":{"kind":"constant","value":1.0}}}),
        )
        .unwrap();
        let id: NodeId = added["id"].as_str().unwrap().parse().unwrap();
        let Operation::Shader(s) = &doc.snapshot().nodes[&id].operation else {
            panic!("a shader")
        };
        let at = |v: &veylvik_document::Scalar| {
            v.at(veylvik_document::Time::new(0, 1).unwrap()).unwrap()
        };
        assert_eq!(
            (at(&s.p1), at(&s.p2)),
            (0.7, 1.0),
            "declared default, then the caller's own value"
        );
        // A flock with an emitter given the Murmuration look is born anywhere on the picture.
        let word = run(&mut doc, "add_node", json!({"type":"text"})).unwrap();
        let flock = run(
            &mut doc,
            "add_node",
            json!({"type":"particles","inputs":{"emitter":word["id"]}}),
        )
        .unwrap();
        run(
            &mut doc,
            "particle_look",
            json!({"node":flock["id"],"look":"Murmuration"}),
        )
        .unwrap();
        let id: NodeId = flock["id"].as_str().unwrap().parse().unwrap();
        let Operation::Particles(p) = &doc.snapshot().nodes[&id].operation else {
            panic!("particles")
        };
        assert_eq!((at(&p.width), at(&p.height)), (0.0, 0.0));
    }
    fn names() -> Vec<&'static str> {
        tools().into_iter().map(|tool| tool.0).collect()
    }
}
