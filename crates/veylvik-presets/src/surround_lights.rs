//! Four broad softboxes around a scene, preserving existing lights and image branches.
use crate::{Scene, c};
use serde_json::json;
use veylvik_document::{Command, Composition, Error, NodeId, Operation, Result, nodes::*};
/// Add a neutral or warm surround rig to an existing Render scene as one undo step.
pub fn surround_lights(
    doc: &Composition,
    render: NodeId,
    warm: bool,
) -> Result<crate::authoring::Authored> {
    let node = doc
        .nodes
        .get(&render)
        .ok_or_else(|| Error("Render scene not found".into()))?;
    if !matches!(node.operation, Operation::RenderScene(_)) {
        return Err(Error(
            "Select a Render scene node to add surround lighting".into(),
        ));
    }
    let root = *node
        .inputs
        .get("scene")
        .ok_or_else(|| Error("Render scene has no scene input".into()))?;
    let s = doc.width as f32 / 1920.;
    let mut scene = Scene::below(doc);
    let mut parts = vec![root];
    // Broad asymmetric cards give metals readable bands rather than tiny white dots.
    // The four sides also light the backs of extruded titles during a complete orbit.
    for (i, (p, rotation, power, width)) in [
        ([0., -450., 1600.], [15., 0., -12.], 1.8, 1100.),
        ([1600., -180., 0.], [0., 90., 10.], 1.1, 700.),
        ([0., -350., -1600.], [-12., 180., -8.], 1.5, 1000.),
        ([-1600., -100., 0.], [0., -90., 12.], 0.9, 650.),
    ]
    .into_iter()
    .enumerate()
    {
        parts.push(scene.add(
            &format!("Surround softbox {}", i + 1),
            &LIGHT_3D,
            json!({
                "kind":"area", "shadows":true,
                "placement":{"position":p.map(|v| v*s),"rotation":rotation,"scale":[1.,1.,1.]},
                "colour":if warm { [1.,0.86,0.69] } else { [1.,1.,1.] },
                "intensity":c(power),"width":c(width*s),"height":c(650.*s),"softness":c(0.6)
            }),
            &[],
        )?);
    }
    let merged = scene.merge("Surround lighting", parts)?;
    let built = scene.built(
        merged,
        None,
        vec![Command::Connect {
            id: render,
            input: "scene".into(),
            source: Some(merged),
        }],
    );
    Ok(crate::authoring::Authored {
        commands: built.commands,
        result: json!({"render":render,"rig":merged,"hint":"Four editable softboxes; existing lighting is preserved. Adjust their intensity to balance the scene."}),
    })
}
