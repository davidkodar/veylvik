//! Evaluates a 3D scene graph at one frame into flat, bounded lists of objects, cameras and lights with world matrices.
//!
//! The frame is the composition's: x right and y down in pixels, z towards the viewer.
//! "Home" is the frame centre at depth 0; positions are offsets from where each node
//! naturally sits. The home camera sees exactly the 2D frame at depth 0.
mod flutter;
pub mod math;
pub mod pick;
mod shot;
mod view;
pub use flutter::{Flutter, flutter};
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
    /// A walking person `height` pixels tall, posed after walking `distance` heights at
    /// `pace`; its body is centred on the pelvis along the walk (the object's place
    /// carries it along), standing on y = height / 2 below its centre. `detail` is the
    /// mesh's cells to the height.
    Figure {
        height: f32,
        distance: f32,
        pace: f32,
        detail: u32,
    },
    /// Gaussian splats from a file, its box's largest side `size` pixels, shown at
    /// `brightness` times their captured colours.
    Splats {
        path: String,
        size: f32,
        brightness: f32,
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
        /// The file's animation it plays (numbered from 0), how many seconds into it, and
        /// whether it loops; the renderer moves each part by it.
        play: Option<(usize, f32, bool)>,
    },
}
#[derive(Debug, Clone)]
pub struct Surface {
    pub colour: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    /// Finish: brushing, the grain's direction (degrees), imperfections, visible
    /// grain, ridges (all but the direction 0 to 1).
    pub finish: [f32; 5],
    /// Shows its own colours without lighting (image cards).
    pub unlit: bool,
    /// A library material that shades the surface instead of the values above.
    pub look: Option<Box<veylvik_document::nodes::Material>>,
    /// How much of an image card shows, 0 to 1 (1 for everything else).
    pub opacity: f32,
    /// Shows only the shadows falling on it.
    pub catcher: bool,
    /// How dark those shadows are, 0 to 1.
    pub shadow: f32,
    /// Its own light: the colour added unlit, times this.
    pub emission: f32,
}
#[derive(Debug, Clone)]
pub struct Object {
    pub node: NodeId,
    /// The node whose picture colours the surface (a shape's Texture input).
    pub texture: Option<NodeId>,
    pub geometry: Geometry,
    pub world: Mat4,
    pub surface: Surface,
    /// A thing the object keeps its front towards, and the frame it turns about
    /// (a 3D text's letters share one); applied once the whole scene is known.
    pub aim: Option<(NodeId, Mat4)>,
    /// Drawn as copies at the particles of a Particle instances node (this one), centred
    /// on itself, rather than once where it is.
    pub instanced: Option<NodeId>,
    /// Coloured by a picture as a camera saw it (a Camera projection node).
    pub projection: Option<Projection>,
    /// Waves bent into its shape by a Flutter node above it.
    pub flutter: Option<Flutter>,
}
/// A picture cast through a camera onto an object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// The node whose picture is cast.
    pub image: NodeId,
    /// The projecting camera, where it was when it saw the picture.
    pub camera: Camera,
    /// The picture becomes the surface's colour, lit by the scene (else shown as it is).
    pub lit: bool,
    /// What the camera did not see is left out (else keeps the object's own look).
    pub hide_unseen: bool,
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
    /// An object the camera keeps looking at; applied once the whole scene is known.
    pub aim_at: Option<NodeId>,
    /// Evaluated world-space orbit offset and fallback target.
    pub orbit: Option<([f32; 3], [f32; 3])>,
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
    /// An area light's fade towards its panel's edges, as a share of its half size.
    pub feather: f32,
    /// An area light's panel size.
    pub size: [f32; 2],
    /// How strongly it shows in shiny surfaces (0 to 1), its light on matte ones unchanged.
    pub reflections: f32,
    /// An object the light keeps pointing at, and the frame it turns about (its
    /// own, or the whole row's for a strip); applied once the whole scene is known.
    pub aim: Option<(NodeId, Mat4)>,
}
impl Light {
    /// The light's strength as the shaders use it in a frame `width` pixels wide. Point
    /// and spot lights fall off with the square of their distance in pixels, normalised
    /// to 1000 pixels of a 1920-wide frame, so the same scene is as bright at any size:
    /// at 3840 wide every distance is twice as many pixels, and the light four times as
    /// strong to match.
    pub fn strength(&self, width: u32) -> f32 {
        match self.kind {
            LightKind::Point | LightKind::Spot => self.intensity * (width as f32 / 1920.0).powi(2),
            _ => self.intensity,
        }
    }
    pub fn position(&self) -> [f32; 3] {
        math::point(&self.world, [0.0; 3])
    }
    /// An area light's half-width and half-height axes in the world.
    pub fn panel(&self) -> [[f32; 3]; 2] {
        [
            math::direction(&self.world, [self.size[0] * 0.5, 0.0, 0.0]),
            math::direction(&self.world, [0.0, self.size[1] * 0.5, 0.0]),
        ]
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
    /// The image is a frame of the shot, not a panorama.
    pub frame: bool,
}
/// A medium filling a shape (a Volume node), as the renderers read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    pub node: NodeId,
    /// Its box's frame: the box spans minus to plus half of `size` in it.
    pub world: Mat4,
    pub shape: veylvik_document::nodes::VolumeShape,
    pub size: [f32; 3],
    /// Light going 100 pixels through it is dimmed to e^-density.
    pub density: f32,
    pub colour: [f32; 3],
    pub absorption: f32,
    pub forward: f32,
    pub noise: f32,
    pub noise_size: f32,
    pub detail: f32,
    /// How far its noise has drifted by now, in pixels along its own axes.
    pub drift: [f32; 3],
    pub softness: f32,
    pub seed: u32,
}
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub objects: Vec<Object>,
    /// Smoke, mist and haze.
    pub volumes: Vec<Volume>,
    pub cameras: Vec<Camera>,
    pub lights: Vec<Light>,
    /// The first Environment in the scene.
    pub environment: Option<Environment>,
    /// Guide points (Point cloud nodes) in the world, by node.
    pub points: Vec<(NodeId, [f32; 3])>,
    /// Where a node made of several moving pieces (a 3D text arriving letter by
    /// letter) rests as a whole: what cameras and lights aim and focus at.
    pub anchors: Vec<(NodeId, [f32; 3])>,
}
impl Scene {
    /// The centre of node `id`'s object: its resting place when it has one, else
    /// the first object it made.
    pub fn centre(&self, id: NodeId) -> Option<[f32; 3]> {
        self.anchors
            .iter()
            .find(|(node, _)| *node == id)
            .map(|anchor| anchor.1)
            .or_else(|| {
                self.objects
                    .iter()
                    .find(|o| o.node == id)
                    .map(|o| o.sphere().0)
            })
    }
}
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}
/// Whether a placed thing is in the scene at `time`.
fn shown(p: &Placement, time: Time) -> Result<bool> {
    Ok(p.shown.at(time)? >= 0.5)
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
/// The colour the reflections view gives light number `n`: well separated hues, the
/// same in the renderer's shader and in the viewer's guides.
pub fn light_hue(n: u32) -> [f32; 3] {
    let h = (n as f32 * 0.618_034 + 0.08).fract() * 6.0;
    let rgb = [
        ((h - 3.0).abs() - 1.0).clamp(0.0, 1.0),
        (2.0 - (h - 2.0).abs()).clamp(0.0, 1.0),
        (2.0 - (h - 4.0).abs()).clamp(0.0, 1.0),
    ];
    rgb.map(|c| 1.0 + (c - 1.0) * 0.85)
}
impl Scene {
    /// Each light node's number in the order the scene lists them: a row of strips is
    /// one node and so one number. The reflections view colours by it.
    pub fn light_numbers(&self) -> Vec<(NodeId, u32)> {
        let mut numbers: Vec<(NodeId, u32)> = Vec::new();
        for light in &self.lights {
            if !numbers.iter().any(|(node, _)| *node == light.node) {
                numbers.push((light.node, numbers.len() as u32));
            }
        }
        numbers
    }
}
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
        aim_at: None,
        orbit: None,
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
        let swing = self.flutter.map_or(0.0, |f| f.amplitude * 1.3);
        (
            centre,
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() + swing,
        )
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
        // The box most of its splats lie in, at its size (its proportions are the file's).
        Geometry::Splats { size, .. } => ([-size * 0.5; 3], [size * 0.5; 3]),
        // Round a person centred on the pelvis: a stride either way, head to foot.
        Geometry::Figure { height, .. } => (
            [-0.5 * height, -0.5 * height, -0.2 * height],
            [0.5 * height, 0.5 * height, 0.2 * height],
        ),
    }
}
impl Camera {
    /// The distance in focus: the Focus on object's centre along the view when it
    /// is in the scene, else Focus distance (0 = the frame centre).
    pub fn focus(&self, scene: &Scene) -> f32 {
        self.focus_on
            .and_then(|id| scene.centre(id))
            .map(|centre| {
                let v = math::point(&math::rigid_inverse(&self.world), centre);
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
    /// Turns every camera, light and object with an Aim at thing in the scene towards
    /// its centre. A missing object leaves the placement's rotation in charge.
    fn aim(&mut self) {
        let mut centres: Vec<(NodeId, [f32; 3])> = self.anchors.clone();
        centres.extend(self.objects.iter().map(|o| (o.node, o.sphere().0)));
        // Resolve orbit positions before collecting camera targets: billboards and
        // lights aimed at a moving camera must see its actual position this frame.
        for camera in &mut self.cameras {
            if let Some((offset, fallback)) = camera.orbit {
                let target = camera
                    .aim_at
                    .and_then(|id| centres.iter().find(|c| c.0 == id).map(|c| c.1))
                    .unwrap_or(fallback);
                camera.world = translate(std::array::from_fn(|i| target[i] + offset[i]));
                camera.world = math::aimed(&camera.world, &camera.world, target);
            }
        }
        let own = centres.len();
        // Objects may also face a camera or a light.
        centres.extend(
            self.cameras
                .iter()
                .filter_map(|c| Some((c.node?, math::point(&c.world, [0.0; 3])))),
        );
        centres.extend(self.lights.iter().map(|l| (l.node, l.position())));
        let faced: Vec<Option<([f32; 3], [f32; 3])>> = self
            .objects
            .iter()
            .map(|o| {
                let (id, _) = o.aim?;
                let at = centres[..own].iter().find(|c| c.0 == o.node)?.1;
                let target = centres.iter().find(|c| c.0 == id && c.0 != o.node)?.1;
                Some((at, target))
            })
            .collect();
        for (o, faced) in self.objects.iter_mut().zip(faced) {
            if let (Some((_, mut pivot)), Some((at, target))) = (o.aim, faced) {
                // Turn about the object's middle; its front is +z, where a camera's
                // view runs along -z, so it looks away from the mirrored target.
                for (row, value) in at.into_iter().enumerate() {
                    pivot[row][3] = value;
                }
                let away = [0, 1, 2].map(|k| 2.0 * at[k] - target[k]);
                o.world = math::aimed(&o.world, &pivot, away);
            }
        }
        let centre = |id: NodeId| centres.iter().find(|(node, _)| *node == id).map(|c| c.1);
        for camera in &mut self.cameras {
            if let Some(target) = camera.aim_at.and_then(centre) {
                camera.world = math::aimed(&camera.world, &camera.world, target);
            }
        }
        for light in &mut self.lights {
            if let Some((id, pivot)) = light.aim
                && let Some(target) = centre(id)
            {
                light.world = math::aimed(&light.world, &pivot, target);
            }
        }
    }
    fn transformed(mut self, m: &Mat4) -> Self {
        // Copies at particles are placed by the particles alone.
        for o in self.objects.iter_mut().filter(|o| o.instanced.is_none()) {
            o.world = mul(m, &o.world);
            if let Some((_, pivot)) = &mut o.aim {
                *pivot = mul(m, pivot);
            }
        }
        for c in &mut self.cameras {
            c.world = mul(m, &c.world);
        }
        for v in &mut self.volumes {
            v.world = mul(m, &v.world);
        }
        for l in &mut self.lights {
            l.world = mul(m, &l.world);
            if let Some((_, pivot)) = &mut l.aim {
                *pivot = mul(m, pivot);
            }
        }
        for (_, p) in self.points.iter_mut().chain(&mut self.anchors) {
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
    let mut scene = node(graph, root, time, 0)?;
    scene.aim();
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
    // Hidden for now: not in the scene (a hidden transform hides all it holds).
    let placement = match &n.operation {
        Operation::Shape3d(s) => Some(&s.placement),
        Operation::Text3d(t) => Some(&t.placement),
        Operation::ImageCard(c) => Some(&c.placement),
        Operation::Model3d(m) => Some(&m.placement),
        Operation::Figure3d(f) => Some(&f.placement),
        Operation::Transform3d(t) => Some(&t.placement),
        Operation::Light3d(l) => Some(&l.placement),
        Operation::Marker3d(m) => Some(&m.placement),
        Operation::Volume3d(v) => Some(&v.placement),
        _ => None,
    };
    if let Some(placement) = placement
        && !shown(placement, time)?
    {
        return Ok(scene);
    }
    match &n.operation {
        Operation::Shape3d(s) => scene.objects.push(Object {
            node: id,
            texture: input("texture"),
            geometry: Geometry::Shape {
                kind: s.kind,
                size: [s.size_x.at(time)?, s.size_y.at(time)?, s.size_z.at(time)?],
                segments: s.detail.at(time)?.round().clamp(3.0, 256.0) as u32,
            },
            world: placed(&s.placement, time, centre)?,
            surface: Surface {
                colour: s.colour.at(time)?,
                opacity: 1.0,
                roughness: s.roughness.at(time)?.clamp(0.0, 1.0),
                metallic: s.metallic.at(time)?.clamp(0.0, 1.0),
                finish: [
                    s.brushed.at(time)?.clamp(0.0, 1.0),
                    s.brush_angle.at(time)?,
                    s.imperfections.at(time)?.clamp(0.0, 1.0),
                    s.grain.at(time)?.clamp(0.0, 1.0),
                    s.ridges.at(time)?.clamp(0.0, 1.0),
                ],
                unlit: false,
                look: s.look.clone(),
                catcher: s.shadow_catcher,
                emission: s.emission.at(time)?.max(0.0),
                shadow: s.shadow_strength.at(time)?.clamp(0.0, 1.0),
            },
            aim: None,
            instanced: None,
            projection: None,
            flutter: None,
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
            let surface = Surface {
                colour: t.colour.at(time)?,
                opacity: 1.0,
                roughness: t.roughness.at(time)?.clamp(0.0, 1.0),
                metallic: t.metallic.at(time)?.clamp(0.0, 1.0),
                finish: [
                    t.brushed.at(time)?.clamp(0.0, 1.0),
                    t.brush_angle.at(time)?,
                    t.imperfections.at(time)?.clamp(0.0, 1.0),
                    t.grain.at(time)?.clamp(0.0, 1.0),
                    t.ridges.at(time)?.clamp(0.0, 1.0),
                ],
                unlit: false,
                look: t.look.clone(),
                catcher: false,
                emission: t.emission.at(time)?.max(0.0),
                shadow: 1.0,
            };
            // The pieces that arrive one by one, each moved in the outline's own frame.
            let mut pieces = vec![(outline, math::IDENTITY)];
            use veylvik_document::nodes::LetterMotion;
            let letters = t.letters;
            if letters.motion != LetterMotion::None
                && let op @ Operation::Text(_) = &source.operation
            {
                let seconds = time.seconds()?;
                pieces = veylvik_text::outline_units(op, time, letters.unit)
                    .map_err(|e| Error(e.to_string()))?
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, piece)| {
                        let p = letters.progress(seconds, i);
                        // Not yet arrived, or already gone.
                        if p <= 0.0 {
                            return None;
                        }
                        let [x0, y0, x1, y1] = veylvik_text::bounds(&piece);
                        let centre = [(x0 + x1) * 0.5, (y0 + y1) * 0.5, 0.0];
                        let about = |m: Mat4| {
                            mul(&translate(centre), &mul(&m, &translate(centre.map(|v| -v))))
                        };
                        let away = 1.0 - p;
                        let moved = match letters.motion {
                            LetterMotion::None => math::IDENTITY,
                            LetterMotion::Fly => translate([0.0, 0.0, letters.distance * away]),
                            LetterMotion::Drop => translate([0.0, -letters.distance * away, 0.0]),
                            LetterMotion::Turn => about(rotate([0.0, 90.0 * away, 0.0])),
                            LetterMotion::Grow => about(scale([p.max(1e-3); 3])),
                        };
                        Some((piece, moved))
                    })
                    .collect();
                scene.anchors.push((id, math::point(&world, pivot)));
            }
            let (depth, bevel) = (t.depth.at(time)?.max(0.0), t.bevel.at(time)?.max(0.0));
            for (outline, moved) in pieces {
                scene.objects.push(Object {
                    node: id,
                    texture: None,
                    geometry: Geometry::Text {
                        outline,
                        depth,
                        bevel,
                        round: t.round_bevel,
                    },
                    world: mul(&world, &moved),
                    surface: surface.clone(),
                    aim: t.aim_at.map(|target| (target, world)),
                    instanced: None,
                    projection: None,
                    flutter: None,
                });
            }
        }
        Operation::ImageCard(c) => scene.objects.push(Object {
            node: id,
            texture: None,
            geometry: Geometry::Card {
                image: input("image")
                    .ok_or_else(|| Error("an image card needs an image".into()))?,
                // A picture of its own size gives the card its shape.
                size: match input("image")
                    .and_then(|source| (doc.node)(source))
                    .map(|source| &source.operation)
                {
                    Some(Operation::OwnSize(own)) => [own.width as f32, own.height as f32],
                    _ => [w as f32, h as f32],
                },
            },
            world: placed(&c.placement, time, centre)?,
            surface: Surface {
                colour: c.colour.at(time)?,
                opacity: c.opacity.at(time)?.clamp(0.0, 1.0),
                roughness: 1.0,
                metallic: 0.0,
                finish: [0.0; 5],
                unlit: true,
                look: None,
                catcher: false,
                emission: 0.0,
                shadow: 1.0,
            },
            aim: None,
            instanced: None,
            projection: None,
            flutter: None,
        }),
        Operation::Model3d(m) if !m.model.is_empty() => scene.objects.push(Object {
            node: id,
            texture: None,
            geometry: Geometry::Model {
                path: m.model.clone(),
                size: m.size.at(time)?.max(1.0),
                extent: m.extent,
                part: None,
                play: if m.clip == 0 {
                    None
                } else {
                    // Its first pose until it begins, then on at its speed.
                    let since = time.seconds()? as f32 - m.clip_start.at(time)?;
                    Some((
                        m.clip as usize - 1,
                        since.max(0.0) * m.clip_speed.at(time)?,
                        m.clip_loop,
                    ))
                },
            },
            world: placed(&m.placement, time, centre)?,
            surface: Surface {
                colour: [0.8; 3],
                opacity: 1.0,
                roughness: 0.5,
                metallic: 0.0,
                finish: [0.0; 5],
                unlit: false,
                look: m.look.clone(),
                catcher: false,
                emission: 0.0,
                shadow: 1.0,
            },
            aim: None,
            instanced: None,
            projection: None,
            flutter: None,
        }),
        Operation::Model3d(_) => {}
        Operation::GaussianSplats(g) if !g.file.is_empty() => scene.objects.push(Object {
            node: id,
            texture: None,
            geometry: Geometry::Splats {
                path: g.file.clone(),
                size: g.size.at(time)?.max(1.0),
                brightness: g.brightness.at(time)?.max(0.0),
            },
            world: placed(&g.placement, time, centre)?,
            surface: Surface {
                colour: [1.0; 3],
                opacity: 1.0,
                roughness: 1.0,
                metallic: 0.0,
                finish: [0.0; 5],
                unlit: true,
                look: None,
                catcher: false,
                emission: 0.0,
                shadow: 1.0,
            },
            aim: None,
            instanced: None,
            projection: None,
            flutter: None,
        }),
        Operation::GaussianSplats(_) => {}
        Operation::Figure3d(f) => {
            let height = f.height.at(time)?.max(1.0);
            let distance = f.distance.at(time)? / height;
            let pace = f.pace.at(time)?.clamp(0.0, 1.0);
            // The walk carries the object along: its place is where the pelvis has got to,
            // so particles born on it are left behind as it moves on.
            let along = veylvik_figure::walk(distance, pace).pelvis[0] * height;
            scene.objects.push(Object {
                node: id,
                texture: None,
                geometry: Geometry::Figure {
                    height,
                    distance,
                    pace,
                    detail: f.detail.at(time)?.round().clamp(24.0, 400.0) as u32,
                },
                world: mul(
                    &placed(&f.placement, time, centre)?,
                    &translate([along, 0.0, 0.0]),
                ),
                surface: Surface {
                    colour: f.colour.at(time)?,
                    opacity: 1.0,
                    roughness: f.roughness.at(time)?.clamp(0.0, 1.0),
                    metallic: f.metallic.at(time)?.clamp(0.0, 1.0),
                    finish: [0.0; 5],
                    unlit: false,
                    look: f.look.clone(),
                    catcher: false,
                    emission: 0.0,
                    shadow: 1.0,
                },
                aim: None,
                instanced: None,
                projection: None,
                flutter: None,
            });
        }
        Operation::Flutter3d(f) => {
            let child = input("scene")
                .ok_or_else(|| Error(format!("{}: Flutter needs a scene", n.label)))?;
            scene = node(doc, child, time, depth + 1)?;
            let waves = Flutter {
                amplitude: f.amplitude.at(time)?.max(0.0),
                wavelength: f.wavelength.at(time)?.max(1.0),
                speed: f.speed.at(time)?,
                pinned: f.pinned.at(time)?.clamp(0.0, 1.0),
                time: time.seconds()? as f32,
            };
            for object in &mut scene.objects {
                object.flutter.get_or_insert(waves);
            }
        }
        Operation::Transform3d(t) => {
            let child = input("scene").ok_or_else(|| Error("3D transform needs a scene".into()))?;
            let m = mul(
                &placed(&t.placement, time, centre)?,
                &translate(centre.map(|v| -v)),
            );
            scene = node(doc, child, time, depth + 1)?.transformed(&m);
            // What cameras and lights aimed at the transform look at: the middle of what
            // it moves (its objects, else its markers), or where its pivot went when it
            // moves nothing.
            let mut middles: Vec<[f32; 3]> = scene.objects.iter().map(|o| o.sphere().0).collect();
            if middles.is_empty() {
                // Only markers (or other places) under it: their middle.
                middles = scene.anchors.iter().map(|a| a.1).collect();
            }
            let middle = if middles.is_empty() {
                math::point(&m, centre)
            } else {
                std::array::from_fn(|k| {
                    middles.iter().map(|p| p[k]).sum::<f32>() / middles.len() as f32
                })
            };
            scene.anchors.push((id, middle));
        }
        // Its objects, coloured by the picture as the camera saw it.
        Operation::CameraProjection(cast) => {
            let (Some(child), Some(lens), Some(image)) =
                (input("scene"), input("camera"), input("image"))
            else {
                return Err(Error(format!(
                    "{}: Camera projection needs objects on scene, a Camera on camera and a picture on image",
                    n.label
                )));
            };
            let at = if cast.hold {
                let seconds = f64::from(cast.hold_time.at(time)?);
                Time::new((seconds * 24000.0).round() as i64, 24000)?
            } else {
                time
            };
            let camera = *node(doc, lens, at, depth + 1)?
                .cameras
                .first()
                .ok_or_else(|| Error(format!("{}: the camera input takes a Camera", n.label)))?;
            scene = node(doc, child, time, depth + 1)?;
            for object in &mut scene.objects {
                object.projection = Some(Projection {
                    image,
                    camera,
                    lit: cast.lit,
                    hide_unseen: cast.hide_unseen,
                });
            }
        }
        // Its objects, centred on themselves, to be drawn at each particle.
        Operation::ParticleInstances(_) => {
            if let Some(child) = input("scene") {
                for mut object in node(doc, child, time, depth + 1)?.objects {
                    let (a, b) = object.local_bounds();
                    let middle = math::point(
                        &object.world,
                        [
                            (a[0] + b[0]) * 0.5,
                            (a[1] + b[1]) * 0.5,
                            (a[2] + b[2]) * 0.5,
                        ],
                    );
                    object.world = mul(&translate(middle.map(|v| -v)), &object.world);
                    object.instanced = Some(id);
                    object.aim = None;
                    scene.objects.push(object);
                }
            }
        }
        Operation::MergeScenes(_) => {
            for port in veylvik_document::nodes::MERGE_PORTS {
                if let Some(child) = input(port) {
                    let part = node(doc, child, time, depth + 1)?;
                    scene.objects.extend(part.objects);
                    scene.volumes.extend(part.volumes);
                    scene.cameras.extend(part.cameras);
                    scene.lights.extend(part.lights);
                    scene.environment = scene.environment.or(part.environment);
                    scene.points.extend(part.points);
                    scene.anchors.extend(part.anchors);
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
                aim_at: c.aim_at,
                orbit: c
                    .orbit
                    .filter(|o| o.enabled)
                    .map(|o| (o.offset(time), [centre[0], centre[1], 0.])),
            });
        }
        Operation::Light3d(l) => {
            let world = placed(&l.placement, time, centre)?;
            let light = Light {
                node: id,
                kind: l.kind,
                shadows: l.shadows,
                world,
                aim: l.aim_at.map(|target| (target, world)),
                colour: l.colour.at(time)?,
                intensity: l.intensity.at(time)?.max(0.0),
                cone: l.cone.at(time)?.clamp(1.0, 179.0),
                softness: l.softness.at(time)?.clamp(0.0, 1.0),
                feather: l.feather.at(time)?.clamp(0.0, 1.0),
                size: [l.width.at(time)?.max(1.0), l.height.at(time)?.max(1.0)],
                reflections: l.reflections.at(time)?.clamp(0.0, 1.0),
            };
            let count = l.strips.at(time)?.round().clamp(1.0, 32.0) as usize;
            if l.kind == veylvik_document::nodes::LightKind::Area && count > 1 {
                scene.lights.extend(strips(
                    &light,
                    count,
                    l.variation.at(time)?.clamp(0.0, 1.0),
                    l.seed,
                ));
            } else {
                scene.lights.push(light);
            }
        }
        Operation::Environment3d(e) => {
            scene.environment = Some(Environment {
                node: id,
                image: input("image"),
                intensity: e.intensity.at(time)?.max(0.0),
                rotation: e.rotation.at(time)?,
                background: e.background,
                blur: e.blur.at(time)?.clamp(0.0, 1.0),
                frame: e.frame,
            })
        }
        Operation::Volume3d(v) => {
            let seconds = time.seconds()? as f32;
            scene.volumes.push(Volume {
                node: id,
                world: placed(&v.placement, time, centre)?,
                shape: v.shape,
                size: [v.size_x.at(time)?, v.size_y.at(time)?, v.size_z.at(time)?]
                    .map(|s| s.max(1.0)),
                density: v.density.at(time)?.max(0.0),
                colour: v.colour.at(time)?.map(|c| c.max(0.0)),
                absorption: v.absorption.at(time)?.clamp(0.0, 1.0),
                forward: v.forward.at(time)?.clamp(-0.95, 0.95),
                noise: v.noise.at(time)?.clamp(0.0, 1.0),
                noise_size: v.noise_size.at(time)?.max(1.0),
                detail: v.detail.at(time)?.clamp(1.0, 6.0),
                drift: [
                    v.drift_x.at(time)? * seconds,
                    v.drift_y.at(time)? * seconds,
                    v.drift_z.at(time)? * seconds,
                ],
                softness: v.softness.at(time)?.clamp(0.0, 1.0),
                seed: v.seed,
            });
        }
        // Nothing to draw: only a place to aim, focus and face at.
        Operation::Marker3d(m) => scene.anchors.push((
            id,
            math::point(&placed(&m.placement, time, centre)?, [0.0, 0.0, 0.0]),
        )),
        Operation::PointCloud(c) => scene.points.extend(
            c.points
                .iter()
                .map(|p| (id, [centre[0] + p[0], centre[1] + p[1], centre[2] + p[2]])),
        ),
        _ => return fail(format!("{} is not a 3D scene node", n.label)),
    }
    // A shape, card or model that faces something turns about its own frame.
    let facing = match &n.operation {
        Operation::Shape3d(s) => s.aim_at,
        Operation::ImageCard(c) => c.aim_at,
        Operation::Model3d(m) => m.aim_at,
        _ => None,
    };
    if let Some(target) = facing {
        for o in &mut scene.objects {
            o.aim = Some((target, o.world));
        }
    }
    Ok(scene)
}

/// An area light split along its width into `count` strips. `variation` (0 to 1)
/// makes them uneven: lengths 0.5 to 1.5 times the even share, gaps up to half a
/// share, heights and brightness up to ±40%, tilt up to ±4 degrees. Deterministic
/// for a seed; with no variation the strips tile the panel exactly.
pub fn strips(light: &Light, count: usize, variation: f32, seed: u32) -> Vec<Light> {
    let mut state = u64::from(seed) ^ 0x9e37_79b9_7f4a_7c15;
    // Uniform in -1..1 (SplitMix64).
    let mut draw = move || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    };
    // Relative lengths and the gap after each strip, then fitted to the width.
    let parts: Vec<[f32; 5]> = (0..count)
        .map(|_| {
            [
                1.0 + 0.5 * variation * draw(),
                0.25 * variation * (draw() + 1.0),
                1.0 + 0.4 * variation * draw(),
                1.0 + 0.4 * variation * draw(),
                4.0 * variation * draw(),
            ]
        })
        .collect();
    let total: f32 = parts.iter().map(|p| p[0] + p[1]).sum::<f32>() - parts[count - 1][1];
    let unit = light.size[0] / total;
    let mut x = -light.size[0] * 0.5;
    parts
        .iter()
        .map(|&[length, gap, height, gain, tilt]| {
            let width = length * unit;
            let centre = x + width * 0.5;
            x += width + gap * unit;
            Light {
                world: math::mul(
                    &light.world,
                    &math::mul(
                        &math::translate([centre, 0.0, 0.0]),
                        &math::rotate([0.0, 0.0, tilt]),
                    ),
                ),
                intensity: light.intensity * gain,
                size: [width.max(1.0), (light.size[1] * height).max(1.0)],
                ..*light
            }
        })
        .collect()
}
/// The object's mesh in its own frame (before `world`).
pub fn mesh(object: &Object) -> Result<Mesh> {
    let mut mesh = built(object)?;
    if let Some(waves) = &object.flutter {
        flutter(&mut mesh, waves, None);
    }
    Ok(mesh)
}
fn built(object: &Object) -> Result<Mesh> {
    use veylvik_mesh::Primitive as P;
    // A flat plane or card that flutters is cut into a grid it can bend along.
    if object.flutter.is_some() {
        match &object.geometry {
            Geometry::Shape {
                kind: ShapeKind::Plane,
                size,
                segments,
            } => {
                return Ok(veylvik_mesh::bendable_plane(
                    [size[0], size[1]],
                    (*segments).max(24),
                ));
            }
            Geometry::Card { size, .. } => return Ok(veylvik_mesh::bendable_plane(*size, 96)),
            _ => {}
        }
    }
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
                ShapeKind::Ring => P::Ring,
            },
            *size,
            *segments,
        ),
        Geometry::Figure {
            height,
            distance,
            pace,
            detail,
        } => {
            let pose = veylvik_figure::walk(*distance, *pace);
            let mut mesh = veylvik_figure::mesh(&pose, *height, *detail);
            // Centred on the pelvis along the walk, standing half its height below.
            let along = pose.pelvis[0] * height;
            for v in &mut mesh.vertices {
                v.position[0] -= along;
                v.position[1] += height * 0.5;
            }
            Ok(mesh)
        }
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
        // Models are split into parts, splats drawn by their own renderer.
        Geometry::Model { .. } | Geometry::Splats { .. } => Ok(Mesh::default()),
    }
}
