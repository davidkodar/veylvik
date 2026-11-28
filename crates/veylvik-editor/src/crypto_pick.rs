//! Picking Cryptomatte objects in the viewer: the objects pass under the click is rendered and read back on the file worker, and the object covering most of that pixel goes into the matte (or out, when it is in already), named by its node's label when the composition has it.
use crate::Studio;
use veylvik_document::{Composition, NodeId, Operation, Time};
use veylvik_render::{ImageInputs, Plan, Region, RenderRequest, Renderer};

/// The id of the object covering most of the pixel at (`x`, `y`) of `plan`'s objects
/// pass, if any object covers it.
pub fn object_at(
    renderer: &Renderer,
    plan: &Plan,
    inputs: &ImageInputs,
    time: Time,
    [x, y]: [u32; 2],
) -> Result<Option<u32>, String> {
    let frame = renderer
        .render_frame(
            plan,
            RenderRequest {
                time,
                region: Some(Region {
                    x,
                    y,
                    width: 1,
                    height: 1,
                }),
                scene_view: None,
                scene_reflections: false,
            },
            inputs,
        )
        .map_err(|e| e.to_string())?;
    let [id, share, ..] = frame.pixels.first().copied().unwrap_or_default();
    Ok((share > 0.0).then(|| id.to_bits()))
}
/// How the matte names the object with `id`: the label of a node of the composition
/// whose name gives that id, else `#` and the id.
pub fn name_of(document: &Composition, id: u32) -> String {
    document
        .nodes
        .values()
        .map(|n| &n.label)
        .find(|label| veylvik_document::nodes::cryptomatte_id(label) == id)
        .cloned()
        .unwrap_or_else(|| format!("#{id:08x}"))
}
/// Adds `entry` to `objects`, or takes it out if it (or another entry for the same id)
/// is there: true when it went in.
pub fn toggle(objects: &mut Vec<String>, entry: String) -> bool {
    let id = veylvik_document::nodes::object_id(&entry);
    let before = objects.len();
    objects.retain(|o| veylvik_document::nodes::object_id(o) != id);
    if objects.len() == before {
        objects.push(entry);
        true
    } else {
        false
    }
}
impl Studio {
    /// Picks the object under `region`'s middle for the Cryptomatte `node` (on the file worker).
    pub(crate) fn pick_object(&mut self, node: NodeId, region: Region, time: Time) {
        let document = self.project.document.snapshot().clone();
        let Some(source) = document
            .nodes
            .get(&node)
            .and_then(|n| n.inputs.get("objects").copied())
        else {
            self.fail("Connect a Render scene with Pass objects to Objects first.");
            return;
        };
        let (renderer, inputs, session) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            self.project.session,
        );
        let at = [region.x + region.width / 2, region.y + region.height / 2];
        self.status = "Looking for the object there…".into();
        let result = self.files.start(move || {
            let plan = Plan::compile_output(&document, source).map_err(|e| e.to_string())?;
            let id = object_at(&renderer, &plan, &inputs, time, at)?;
            let name = id.map(|id| name_of(&document, id));
            Ok(crate::files::Completed::then(move |s, _| {
                s.object_picked(session, node, name)
            }))
        });
        self.file_started(result);
    }
    pub(crate) fn object_picked(&mut self, session: NodeId, node: NodeId, name: Option<String>) {
        if session != self.project.session || node != self.draft.id {
            self.fail("The picked object no longer matches the selected node; pick again.");
            return;
        }
        let Operation::Cryptomatte(matte) = &mut self.draft.operation else {
            return;
        };
        let Some(name) = name else {
            self.tell("No object there");
            return;
        };
        let added = toggle(&mut matte.objects, name.clone());
        self.draft_dirty = true;
        self.tell(if added {
            format!("{name} is in the matte · Apply to keep")
        } else {
            format!("{name} is out of the matte · Apply to keep")
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picking_an_object_again_takes_it_out_whatever_it_was_called() {
        let mut objects = Vec::new();
        assert!(toggle(&mut objects, "Red ball".into()));
        let id = veylvik_document::nodes::cryptomatte_id("Red ball");
        assert!(!toggle(&mut objects, format!("#{id:08x}")));
        assert!(objects.is_empty());
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_click_finds_the_object_covering_that_pixel_by_its_label() {
        use veylvik_document::{Node, nodes::*, registry};
        let op = |kind: &registry::NodeType| -> Operation {
            serde_json::from_value(kind.default_operation()).unwrap()
        };
        let mut doc = veylvik_document::blank(160, 90, Time::new(24, 1).unwrap()).unwrap();
        let mut ball = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut ball {
            s.kind = ShapeKind::Sphere;
            s.placement.position = [-40.0, 0.0, 0.0].into();
            for size in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
                *size = 50.0.into();
            }
        }
        let ball = Node::new("Left ball", ball, &[]);
        let mut render = op(&RENDER_SCENE);
        if let Operation::RenderScene(r) = &mut render {
            r.camera = 0.0.into();
            r.pass = ScenePass::Objects;
        }
        let render = Node::new("Objects", render, &[("scene", ball.id)]);
        let output = render.id;
        doc.nodes.insert(ball.id, ball);
        doc.nodes.insert(render.id, render);
        let renderer = Renderer::new().unwrap();
        renderer.set_scene_renderer(std::sync::Arc::new(veylvik_render3d::Engine::new(
            renderer.device(),
        )));
        let plan = Plan::compile_output(&doc, output).unwrap();
        let time = Time::new(0, 1).unwrap();
        let inputs = ImageInputs::default();
        let id = object_at(&renderer, &plan, &inputs, time, [40, 45])
            .unwrap()
            .unwrap();
        assert_eq!(name_of(&doc, id), "Left ball");
        assert_eq!(
            object_at(&renderer, &plan, &inputs, time, [150, 5]).unwrap(),
            None
        );
        assert_eq!(name_of(&doc, 0x1234_5678), "#12345678");
    }
}
