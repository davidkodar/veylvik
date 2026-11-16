//! Evaluates a 3D scene graph at one frame into flat, bounded lists of objects, cameras and lights with world matrices.
//!
//! The frame is the composition's: x right and y down in pixels, z towards the viewer.
//! "Home" is the frame centre at depth 0; positions are offsets from where each node
//! naturally sits. The home camera sees exactly the 2D frame at depth 0.
pub mod math;
mod shot;
mod view;
use math::{Mat4, mul, rotate, scale, translate};
pub use shot::{ShotMatch, match_shot, shining, source_in_frame};
use veylvik_document::{
    Composition, Error, NodeId, Operation, Time,
    nodes::{LightKind, Placement, ShapeKind},
    vector::PathMask,
};
pub use veylvik_mesh::{Mesh, Model, Part, Texture};
pub use view::{FreeView, Pose, smooth_walk};

pub type Result<T> = std::result::Result<T, Error>;
/// Bound on what one scene may hold.
pub const MAX_OBJECTS: usize = 4096;
/// Flattening tolerance for extruded outlines, in pixels.
pub const OUTLINE_TOLERANCE: f32 = 0.2;

#[derive(Debug, Clone)]
pub enum Geometry {
    Shape {
        kind: ShapeKind,
        size: [f32; 3],
        segments: u32,
    },
    /// An outline in composition pixels, extruded `depth` deep about z = 0.
    Text {
        outline: PathMask,
        depth: f32,
        bevel: f32,
        round: bool,
    },
    /// A card the size of the frame showing the image of node `image`.
    Card { image: NodeId, size: [f32; 2] },
    /// An imported model (its file path), scaled so its largest side is `size`
    /// pixels; `extent` is its normalised box. The renderer splits it into its
    /// parts, each with `part` set.
    Model {
        path: String,
        size: f32,
        extent: [f32; 3],
        part: Option<usize>,
    },
}
#[derive(Debug, Clone)]
pub struct Surface {
    pub colour: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    /// Shows its own colours without lighting (image cards).
    pub unlit: bool,
    /// A library material that shades the surface instead of the values above.
    pub look: Option<Box<veylvik_document::nodes::Material>>,
    /// Shows only the shadows falling on it.
    pub catcher: bool,
}
#[derive(Debug, Clone)]
pub struct Object {
    pub node: NodeId,
    pub geometry: Geometry,
    pub world: Mat4,
    pub surface: Surface,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// None for the home view.
    pub node: Option<NodeId>,
    /// Camera to world; the camera looks along its -z with y down.
    pub world: Mat4,
    pub focal_length: f32,
    pub sensor_width: f32,
    /// Lens f-number for depth of field; 0 keeps everything sharp.
    pub f_stop: f32,
    /// Distance in focus (scene units); 0 focuses on the frame centre.
    pub focus_distance: f32,
    /// An object kept in focus, overriding the distance.
    pub focus_on: Option<NodeId>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub node: NodeId,
    pub kind: LightKind,
    pub shadows: bool,
    pub world: Mat4,
    pub colour: [f32; 3],
    pub intensity: f32,
    pub cone: f32,
    pub softness: f32,
}
impl Light {
    pub fn position(&self) -> [f32; 3] {
        math::point(&self.world, [0.0; 3])
    }
    /// Where the light points (its -z), normalised.
    pub fn direction(&self) -> [f32; 3] {
        let d = math::direction(&self.world, [0.0, 0.0, -1.0]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
        d.map(|v| v / l)
    }
}
/// What surrounds the scene: an image panorama (by its source node) or the studio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Environment {
    pub node: NodeId,
    pub image: Option<NodeId>,
    pub intensity: f32,
    /// Degrees about the vertical.
    pub rotation: f32,
    pub background: bool,
    pub blur: f32,
}
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub objects: Vec<Object>,
    pub cameras: Vec<Camera>,
    pub lights: Vec<Light>,
    /// The first Environment in the scene.
    pub environment: Option<Environment>,
    /// Guide points (Point cloud nodes) in the world, by node.
    pub points: Vec<(NodeId, [f32; 3])>,
}
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}
fn placed(p: &Placement, time: Time, origin: [f32; 3]) -> Result<Mat4> {
    let position = p.position.at(time)?;
    Ok(mul(
        &translate([
            origin[0] + position[0],
            origin[1] + position[1],
            origin[2] + position[2],
        ]),
        &mul(&rotate(p.rotation.at(time)?), &scale(p.scale.at(time)?)),
    ))
}
/// Pixels from the home camera to the frame at depth 0 (its focal length in pixels).
pub fn home_distance(width: u32, focal_length: f32, sensor_width: f32) -> f32 {
    width as f32 * focal_length / sensor_width.max(1e-6)
}
/// The camera that sees the 2D frame exactly.
pub fn home_camera(width: u32, height: u32) -> Camera {
    let (focal_length, sensor_width) = (50.0, 36.0);
    Camera {
        node: None,
        world: translate([
            width as f32 * 0.5,
            height as f32 * 0.5,
            home_distance(width, focal_length, sensor_width),
        ]),
        focal_length,
        sensor_width,
        f_stop: 0.0,
        focus_distance: 0.0,
        focus_on: None,
    }
}
impl Object {
    /// The corners of the box around its geometry, in its own frame.
    pub fn local_bounds(&self) -> ([f32; 3], [f32; 3]) {
        local_bounds(&self.geometry)
    }
    /// Centre and radius of a sphere around it, in the world.
    pub fn sphere(&self) -> ([f32; 3], f32) {
        let (a, b) = self.local_bounds();
        let centre = math::point(&self.world, [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5));
        let corner = math::point(&self.world, b);
        let d = [0, 1, 2].map(|k| corner[k] - centre[k]);
        (centre, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
    }
}
/// Local bounding box of an object's geometry.
pub fn local_bounds(geometry: &Geometry) -> ([f32; 3], [f32; 3]) {
    match geometry {
        Geometry::Shape { size, .. } => (size.map(|v| -v * 0.5), size.map(|v| v * 0.5)),
        Geometry::Text { outline, depth, .. } => {
            let [x0, y0, x1, y1] = veylvik_text::bounds(outline);
            ([x0, y0, -depth * 0.5], [x1, y1, depth * 0.5])
        }
        Geometry::Card { size, .. } => (
            [-size[0] * 0.5, -size[1] * 0.5, 0.0],
            [size[0] * 0.5, size[1] * 0.5, 0.0],
        ),
        Geometry::Model { size, extent, .. } => (
            extent.map(|v| -v * size * 0.5),
            extent.map(|v| v * size * 0.5),
        ),
    }
}
impl Camera {
    /// The distance in focus: the Focus on object's centre along the view when it
    /// is in the scene, else Focus distance (0 = the frame centre).
    pub fn focus(&self, scene: &Scene) -> f32 {
        self.focus_on
            .and_then(|id| scene.objects.iter().find(|o| o.node == id))
            .map(|o| {
                let v = math::point(&math::rigid_inverse(&self.world), o.sphere().0);
                (-v[2]).max(1.0)
            })
            .unwrap_or(self.focus_distance)
    }
    /// Where a world point lands in the frame (pixels) and its distance in front of
    /// the camera; None behind it.
    pub fn project(&self, p: [f32; 3], width: u32, height: u32) -> Option<([f32; 2], f32)> {
        let v = math::point(&math::rigid_inverse(&self.world), p);
        let depth = -v[2];
        if depth <= 1e-4 {
            return None;
        }
        let f = home_distance(width, self.focal_length, self.sensor_width);
        Some((
            [
                width as f32 * 0.5 + f * v[0] / depth,
                height as f32 * 0.5 + f * v[1] / depth,
            ],
            depth,
        ))
    }
}
impl Scene {
    /// Camera `index` (1-based, in node order); 0, or a missing camera, is the home view.
    pub fn camera(&self, index: usize, width: u32, height: u32) -> Camera {
        index
            .checked_sub(1)
            .and_then(|i| self.cameras.get(i))
            .copied()
            .unwrap_or_else(|| home_camera(width, height))
    }
    fn transformed(mut self, m: &Mat4) -> Self {
        for o in &mut self.objects {
            o.world = mul(m, &o.world);
        }
        for c in &mut self.cameras {
            c.world = mul(m, &c.world);
        }
        for l in &mut self.lights {
            l.world = mul(m, &l.world);
        }
        for (_, p) in &mut self.points {
            *p = math::point(m, *p);
        }
        self
    }
}

/// Where scene evaluation reads nodes from: a whole composition, or the nodes a
/// render plan already holds.
pub struct Graph<'a> {
    pub width: u32,
    pub height: u32,
    pub node: &'a dyn Fn(NodeId) -> Option<&'a veylvik_document::Node>,
}
/// The scene produced by `root` at `time`.
pub fn evaluate(doc: &Composition, root: NodeId, time: Time) -> Result<Scene> {
    let lookup = |id: NodeId| doc.nodes.get(&id);
    evaluate_in(
        &Graph {
            width: doc.width,
            height: doc.height,
            node: &lookup,
        },
        root,
        time,
    )
}
/// The scene produced by `root` at `time`, reading nodes from `graph`.
pub fn evaluate_in(graph: &Graph<'_>, root: NodeId, time: Time) -> Result<Scene> {
    let scene = node(graph, root, time, 0)?;
    if scene.objects.len() + scene.cameras.len() + scene.lights.len() > MAX_OBJECTS {
        return fail(format!(
            "a 3D scene may hold at most {MAX_OBJECTS} objects, cameras and lights"
        ));
    }
    Ok(scene)
}
fn node(doc: &Graph<'_>, id: NodeId, time: Time, depth: usize) -> Result<Scene> {
    if depth > 256 {
        return fail("3D scene nesting is too deep");
    }
    let Some(n) = (doc.node)(id) else {
        return fail("a 3D scene input is missing");
    };
    let (w, h) = (doc.width, doc.height);
    let centre = [w as f32 * 0.5, h as f32 * 0.5, 0.0];
    let input = |port: &str| n.inputs.get(port).copied();
    let mut scene = Scene::default();
    if n.bypass {
        // A bypassed transform passes its scene through; other scene nodes vanish.
        if let (Operation::Transform3d(_), Some(child)) = (&n.operation, input("scene")) {
            return node(doc, child, time, depth + 1);
        }
        return Ok(scene);
    }
    match &n.operation {
        Operation::Shape3d(s) => scene.objects.push(Object {
            node: id,
            geometry: Geometry::Shape {
                kind: s.kind,
                size: [s.size_x.at(time)?, s.size_y.at(time)?, s.size_z.at(time)?],
                segments: s.detail.at(time)?.round().clamp(3.0, 256.0) as u32,
            },
            world: placed(&s.placement, time, centre)?,
            surface: Surface {
                colour: s.colour.at(time)?,
                roughness: s.roughness.at(time)?.clamp(0.0, 1.0),
                metallic: s.metallic.at(time)?.clamp(0.0, 1.0),
                unlit: false,
                look: s.look.clone(),
                catcher: s.shadow_catcher,
            },
        }),
        Operation::Text3d(t) => {
            let source =
                input("outline").ok_or_else(|| Error("3D text needs an outline".into()))?;
            let Some(source) = (doc.node)(source) else {
                return fail("3D text's outline is missing");
            };
            let outline = match &source.operation {
                op @ Operation::Text(_) => {
                    veylvik_text::outline(op, time).map_err(|e| Error(e.to_string()))?
                }
                Operation::Path(p) => p.geometry.at(time)?,
                _ => return fail("3D text extrudes a Text or Path node"),
            };
            // Turn about the outline's own centre, so the title spins in place.
            let [x0, y0, x1, y1] = veylvik_text::bounds(&outline);
            let pivot = [(x0 + x1) * 0.5, (y0 + y1) * 0.5, 0.0];
            let world = mul(
                &placed(&t.placement, time, pivot)?,
                &translate([-pivot[0], -pivot[1], 0.0]),
            );
            scene.objects.push(Object {
                node: id,
                geometry: Geometry::Text {
                    outline,
                    depth: t.depth.at(time)?.max(0.0),
                    bevel: t.bevel.at(time)?.max(0.0),
                    round: t.round_bevel,
                },
                world,
                surface: Surface {
                    colour: t.colour.at(time)?,
                    roughness: t.roughness.at(time)?.clamp(0.0, 1.0),
                    metallic: t.metallic.at(time)?.clamp(0.0, 1.0),
                    unlit: false,
                    look: t.look.clone(),
                    catcher: false,
                },
            });
        }
        Operation::ImageCard(c) => scene.objects.push(Object {
            node: id,
            geometry: Geometry::Card {
                image: input("image")
                    .ok_or_else(|| Error("an image card needs an image".into()))?,
                size: [w as f32, h as f32],
            },
            world: placed(&c.placement, time, centre)?,
            surface: Surface {
                colour: [1.0; 3],
                roughness: 1.0,
                metallic: 0.0,
                unlit: true,
                look: None,
                catcher: false,
            },
        }),
        Operation::Model3d(m) if !m.model.is_empty() => scene.objects.push(Object {
            node: id,
            geometry: Geometry::Model {
                path: m.model.clone(),
                size: m.size.at(time)?.max(1.0),
                extent: m.extent,
                part: None,
            },
            world: placed(&m.placement, time, centre)?,
            surface: Surface {
                colour: [0.8; 3],
                roughness: 0.5,
                metallic: 0.0,
                unlit: false,
                look: m.look.clone(),
                catcher: false,
            },
        }),
        Operation::Model3d(_) => {}
        Operation::Transform3d(t) => {
            let child = input("scene").ok_or_else(|| Error("3D transform needs a scene".into()))?;
            let m = mul(
                &placed(&t.placement, time, centre)?,
                &translate(centre.map(|v| -v)),
            );
            scene = node(doc, child, time, depth + 1)?.transformed(&m);
        }
        Operation::MergeScenes(_) => {
            for port in veylvik_document::nodes::MERGE_PORTS {
                if let Some(child) = input(port) {
                    let part = node(doc, child, time, depth + 1)?;
                    scene.objects.extend(part.objects);
                    scene.cameras.extend(part.cameras);
                    scene.lights.extend(part.lights);
                    scene.environment = scene.environment.or(part.environment);
                    scene.points.extend(part.points);
                }
            }
        }
        Operation::Camera3d(c) => {
            let (focal_length, sensor_width) = (c.focal_length.at(time)?, c.sensor_width.at(time)?);
            // Scale is ignored: a camera is rigid.
            let position = c.placement.position.at(time)?;
            let home = [
                centre[0],
                centre[1],
                home_distance(w, focal_length, sensor_width),
            ];
            scene.cameras.push(Camera {
                node: Some(id),
                world: mul(
                    &translate([
                        home[0] + position[0],
                        home[1] + position[1],
                        home[2] + position[2],
                    ]),
                    &rotate(c.placement.rotation.at(time)?),
                ),
                focal_length,
                sensor_width,
                f_stop: c.f_stop.at(time)?.max(0.0),
                focus_distance: c.focus_distance.at(time)?.max(0.0),
                focus_on: c.focus_on,
            });
        }
        Operation::Light3d(l) => scene.lights.push(Light {
            node: id,
            kind: l.kind,
            shadows: l.shadows,
            world: placed(&l.placement, time, centre)?,
            colour: l.colour.at(time)?,
            intensity: l.intensity.at(time)?.max(0.0),
            cone: l.cone.at(time)?.clamp(1.0, 179.0),
            softness: l.softness.at(time)?.clamp(0.0, 1.0),
        }),
        Operation::Environment3d(e) => {
            scene.environment = Some(Environment {
                node: id,
                image: input("image"),
                intensity: e.intensity.at(time)?.max(0.0),
                rotation: e.rotation.at(time)?,
                background: e.background,
                blur: e.blur.at(time)?.clamp(0.0, 1.0),
            })
        }
        Operation::PointCloud(c) => scene.points.extend(
            c.points
                .iter()
                .map(|p| (id, [centre[0] + p[0], centre[1] + p[1], centre[2] + p[2]])),
        ),
        _ => return fail(format!("{} is not a 3D scene node", n.label)),
    }
    Ok(scene)
}

/// The object's mesh in its own frame (before `world`).
pub fn mesh(object: &Object) -> Result<Mesh> {
    use veylvik_mesh::Primitive as P;
    match &object.geometry {
        Geometry::Shape {
            kind,
            size,
            segments,
        } => veylvik_mesh::primitive(
            match kind {
                ShapeKind::Cube => P::Cube,
                ShapeKind::Sphere => P::Sphere,
                ShapeKind::Plane => P::Plane,
                ShapeKind::Cylinder => P::Cylinder,
                ShapeKind::Cone => P::Cone,
                ShapeKind::Torus => P::Torus,
            },
            *size,
            *segments,
        ),
        Geometry::Text {
            outline,
            depth,
            bevel,
            round,
        } => veylvik_mesh::extrude_bevelled(
            &outline.commands,
            outline.fill_rule,
            *depth,
            OUTLINE_TOLERANCE,
            veylvik_mesh::Bevel {
                width: *bevel,
                round: *round,
            },
        ),
        Geometry::Card { size, .. } => {
            veylvik_mesh::primitive(P::Plane, [size[0], size[1], 0.0], 3)
        }
        // Model meshes come from the loaded file, which the renderer holds.
        Geometry::Model { .. } => Ok(Mesh::default()),
    }
}
