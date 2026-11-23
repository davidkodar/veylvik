//! One click from a solve to a projection: the footage held at the playhead, cast through the solved camera onto the solved ground or onto a backdrop card at the depth of the solved points, merged into the solved scene.
use crate::Studio;
use veylvik_document::{Command, Node, NodeId, Operation, nodes};

/// What the held frame is cast onto.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Onto {
    /// A plain copy of the solved ground (its shadow catcher stays as it is).
    Ground,
    /// A card facing the camera as it was at the playhead, at the solved points' depth,
    /// filling its view.
    Backdrop,
}
/// The three angles (degrees, as a placement takes them: z, then y, then x) of a
/// rotation matrix's upper left.
fn angles(m: &veylvik_scene::math::Mat4) -> [f32; 3] {
    let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
    let x = m[2][1].atan2(m[2][2]);
    let z = m[1][0].atan2(m[0][0]);
    [x, y, z].map(f32::to_degrees)
}
impl Studio {
    /// Adds the held frame, the surface and a Camera projection from the solve's report.
    pub(crate) fn project_solved_footage(
        &mut self,
        report: &serde_json::Value,
        source: NodeId,
        onto: Onto,
    ) -> Result<(), String> {
        let doc = self.project.document.snapshot();
        let id =
            |key: &str| -> Option<NodeId> { report[key].as_str().and_then(|s| s.parse().ok()) };
        let scene_id = id("scene").ok_or("The solved scene is missing")?;
        let camera_id = id("camera").ok_or("The solved camera is missing")?;
        let mut scene = doc
            .nodes
            .get(&scene_id)
            .ok_or("The solved scene is missing")?
            .clone();
        let time = self.project.time().map_err(|e| e.to_string())?;
        let seconds = time.seconds().map_err(|e| e.to_string())? as f32;
        let default = |kind: &veylvik_document::registry::NodeType| -> Result<Operation, String> {
            serde_json::from_value(kind.default_operation()).map_err(|e| e.to_string())
        };
        // The surface.
        let surface = match onto {
            Onto::Ground => {
                let ground = report["ground"]["node"]
                    .as_str()
                    .and_then(|s| s.parse::<NodeId>().ok())
                    .and_then(|id| doc.nodes.get(&id))
                    .ok_or("No ground was found: project onto a backdrop instead")?;
                let Operation::Shape3d(mut plane) = ground.operation.clone() else {
                    return Err("The solved ground is not a shape".into());
                };
                plane.shadow_catcher = false;
                Node::new("Projected ground", Operation::Shape3d(plane), &[])
            }
            Onto::Backdrop => {
                // The camera as it was at the playhead.
                let by_id: std::collections::BTreeMap<_, _> =
                    doc.nodes.iter().map(|(k, n)| (*k, n)).collect();
                let lookup = |id: NodeId| by_id.get(&id).copied();
                let graph = veylvik_scene::Graph {
                    width: doc.width,
                    height: doc.height,
                    node: &lookup,
                };
                let seen = veylvik_scene::evaluate_in(&graph, scene_id, time)
                    .map_err(|e| e.to_string())?;
                let camera = seen
                    .cameras
                    .iter()
                    .find(|c| c.node == Some(camera_id))
                    .ok_or("The solved camera is not in the solved scene")?;
                let eye = veylvik_scene::math::point(&camera.world, [0.0; 3]);
                // At the depth of the solved points in front of it (their middle), else far off.
                let ahead = veylvik_scene::math::direction(&camera.world, [0.0, 0.0, -1.0]);
                let mut depths: Vec<f32> = seen
                    .points
                    .iter()
                    .map(|(_, p)| (0..3).map(|k| (p[k] - eye[k]) * ahead[k]).sum::<f32>())
                    .filter(|d| *d > 1.0)
                    .collect();
                depths.sort_by(f32::total_cmp);
                let f = veylvik_scene::home_distance(
                    doc.width,
                    camera.focal_length,
                    camera.sensor_width,
                );
                let depth = depths.get(depths.len() / 2).copied().unwrap_or(f * 4.0);
                let centre = [0, 1, 2].map(|k| eye[k] + ahead[k] * depth);
                let Operation::Shape3d(mut card) = default(&nodes::SHAPE_3D)? else {
                    return Err("shape".into());
                };
                card.kind = nodes::ShapeKind::Plane;
                // A little larger than the camera's view at that depth.
                card.size_x = (doc.width as f32 * depth / f * 1.2).into();
                card.size_y = (doc.height as f32 * depth / f * 1.2).into();
                let middle = [doc.width as f32 * 0.5, doc.height as f32 * 0.5, 0.0];
                card.placement.position = [0, 1, 2].map(|k| centre[k] - middle[k]).into();
                card.placement.rotation = angles(&camera.world).into();
                Node::new("Projected backdrop", Operation::Shape3d(card), &[])
            }
        };
        // The footage held at the playhead.
        let Operation::TimeRemap(mut hold) = default(&nodes::TIME_REMAP)? else {
            return Err("time remap".into());
        };
        hold.rate = 0.0.into();
        hold.source_time = seconds.into();
        let held = Node::new(
            "Held frame",
            Operation::TimeRemap(hold),
            &[("image", source)],
        );
        let Operation::CameraProjection(mut cast) = default(&nodes::CAMERA_PROJECTION)? else {
            return Err("projection".into());
        };
        cast.hold = true;
        cast.hold_time = seconds.into();
        let projection = Node::new(
            "Camera projection",
            Operation::CameraProjection(cast),
            &[
                ("scene", surface.id),
                ("camera", camera_id),
                ("image", held.id),
            ],
        );
        let free = (1..)
            .map(|n: usize| {
                if n == 1 {
                    "scene".to_string()
                } else {
                    format!("scene_{n}")
                }
            })
            .find(|port| !scene.inputs.contains_key(port))
            .ok_or("no free scene input")?;
        if !nodes::MERGE_PORTS.contains(&free.as_str()) {
            return Err("The solved scene has no free input; add a Merge scenes".into());
        }
        scene.inputs.insert(free, projection.id);
        self.project
            .apply(&[
                Command::Put {
                    node: Box::new(surface),
                },
                Command::Put {
                    node: Box::new(held),
                },
                Command::Put {
                    node: Box::new(projection),
                },
                Command::Put {
                    node: Box::new(scene),
                },
            ])
            .map_err(|e| e.to_string())?;
        self.refresh_draft();
        self.request_preview();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn angles_turn_back_into_the_same_rotation() {
        for degrees in [
            [10.0, 20.0, 30.0],
            [-35.0, 60.0, 5.0],
            [80.0, -10.0, -120.0],
        ] {
            let m = veylvik_scene::math::rotate(degrees);
            let back = veylvik_scene::math::rotate(super::angles(&m));
            for r in 0..3 {
                for c in 0..3 {
                    assert!((m[r][c] - back[r][c]).abs() < 1e-5, "{degrees:?}");
                }
            }
        }
    }
}
