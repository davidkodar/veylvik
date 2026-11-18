//! Lights from the footage: the headless server's Match the shot, which reads where the light comes from in a frame of the shot and sets a 3D sun to match, as Studio's button does.
use super::{Result, Session, tool};
use serde_json::{Value, json};
use veylvik_document::{Command, Node, NodeId, Operation, Time, nodes, optical::ShotLight};
use veylvik_render::{Plan, RenderRequest};

pub(super) fn descriptor(revision: &Value) -> Value {
    let id = json!({"type":"string","format":"uuid"});
    tool(
        "match_shot",
        "Light a 3D scene like the footage it goes into. Reads a frame of shot (the footage node, or any node showing the plate) for where its light comes from: a bright source seen in the frame is a sun beyond the subject, otherwise the bright side of the frame. It then sets light (a Light node) to a shadow-casting sun with that direction, colour and strength, as seen by the camera of render (the Render scene node). Without light, a new Shot light is added to the scene (its root must be a Merge scenes node with a free input). Returns the light, whether it is a backlight, and environment: the strength an Environment in the scene should have to match the shot's brightness. One undo step.",
        json!({"revision":revision,"render":id,"shot":id,"frame":{"type":"integer","minimum":0},"light":id}),
        &["revision", "render", "shot"],
    )
}
fn node_id(args: &Value, name: &str) -> Result<NodeId> {
    args[name]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| format!("{name} must be a node id").into())
}
impl Session {
    pub(super) fn match_shot(&mut self, args: &Value) -> Result<Value> {
        let doc = self.document.snapshot().clone();
        let (render, shot) = (node_id(args, "render")?, node_id(args, "shot")?);
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
        let scene = veylvik_scene::evaluate(&doc, root, time)?;
        let index = settings.camera.at(time)?.round().clamp(0.0, 64.0) as usize;
        let camera = scene.camera(index, doc.width, doc.height);
        // The shot's picture at that frame.
        let plan = Plan::compile_output(&doc, shot)?;
        if self.renderer.is_none() {
            self.renderer = Some(crate::assets::runtime::renderer()?);
        }
        let renderer = self.renderer.as_ref().unwrap();
        let inputs = crate::assets::load_inputs(&plan, &self.asset_base, renderer.budget_bytes)?;
        let picture = renderer.render_frame(&plan, RenderRequest::full(time), &inputs)?;
        let source = veylvik_scene::source_in_frame(&picture.pixels, doc.width, doc.height);
        let read = ShotLight::analyse(&picture.pixels, doc.width, doc.height)
            .ok_or("the shot is black at that frame: choose another")?;
        let matched = veylvik_scene::match_shot(&read, source, &camera, doc.width, doc.height);
        let mut commands = Vec::new();
        let mut light = match args.get("light") {
            Some(_) => doc
                .nodes
                .get(&node_id(args, "light")?)
                .filter(|n| matches!(n.operation, Operation::Light3d(_)))
                .cloned()
                .ok_or("light must be a Light node")?,
            None => {
                let light = Node::new(
                    "Shot light",
                    serde_json::from_value(nodes::LIGHT_3D.default_operation())?,
                    &[],
                );
                let host = doc
                    .nodes
                    .get(&root)
                    .filter(|n| matches!(n.operation, Operation::MergeScenes(_)))
                    .ok_or("the scene's root is not a Merge scenes node: add the light yourself and pass it as light")?;
                let port = nodes::MERGE_PORTS
                    .iter()
                    .find(|p| !host.inputs.contains_key(**p))
                    .ok_or("the scene is full (eight inputs): merge some of it first")?;
                commands.push(Command::Connect {
                    id: root,
                    input: (*port).into(),
                    source: Some(light.id),
                });
                light
            }
        };
        let Operation::Light3d(l) = &mut light.operation else {
            unreachable!("checked above")
        };
        l.kind = nodes::LightKind::Sun;
        l.shadows = true;
        l.aim_at = None;
        l.placement.rotation = matched.rotation.into();
        l.colour = matched.colour.into();
        l.intensity = matched.intensity.into();
        let id = light.id;
        commands.insert(
            0,
            Command::Put {
                node: Box::new(light),
            },
        );
        let revision = self.document.apply(self.document.revision(), &commands)?;
        Ok(
            json!({"revision":revision,"light":id,"rotation":matched.rotation,"colour":matched.colour,"intensity":matched.intensity,"backlight":matched.backlight,"directional":matched.directional,"source_in_frame":source,"environment":matched.environment,"frame":frame}),
        )
    }
}
