//! USD scenes (`.usd`, `.usda`, `.usdc`, `.usdz`) read into a Veylvik `Model`, the same as an imported glTF: every active mesh of the composed stage with its transforms, normals, uvs and base material, turned y up and normalised; transforms keyed over time become the model's clip, and points keyed over time become a point cache that bends the part. Read through the pure-Rust `openusd` crate; nothing else in Veylvik sees its types.
use openusd::{sdf, usd, vt};
use std::path::Path;
use std::sync::Arc;
use veylvik_mesh::animation::{self as anim, Between, Channel, Clip, Joint, Keyed, Mat};
use veylvik_mesh::{MAX_PARTS, MAX_VERTICES, Mesh, Model, Part, PointCache, Vertex};

mod xform;
use xform::{decompose, mul, world_at};

/// The file extensions read as USD.
pub const EXTENSIONS: [&str; 4] = ["usd", "usda", "usdc", "usdz"];
/// The most values (three a point) all point caches of a file may hold together.
pub const MAX_CACHE_VALUES: usize = 64 * 1024 * 1024;
/// The most moments a file's transforms are keyed at.
pub const MAX_SAMPLES: usize = 100_000;

pub type Result<T> = std::result::Result<T, String>;

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
/// The scene in the USD file at `path` as a model.
pub fn load(path: &Path) -> Result<Model> {
    let name = path.to_str().ok_or("the USD file's path is not UTF-8")?;
    let stage = usd::Stage::open(name).map_err(|e| format!("{}: {e}", path.display()))?;
    // Z up turns to y up: (x, y, z) to (x, z, -y).
    let z_up = matches!(
        stage.stage_metadata("upAxis").map_err(error)?,
        Some(vt::Value::Token(t)) if t.as_str() == "Z"
    );
    let mut up = anim::IDENTITY;
    if z_up {
        up = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
    }
    let per_second = stage.time_codes_per_second();
    let per_second = if per_second.is_finite() && per_second > 0.0 {
        per_second
    } else {
        24.0
    };
    let start = stage.start_time_code();
    let start = if start.is_finite() { start } else { 0.0 };
    let seconds = |t: f64| ((t - start) / per_second) as f32;
    let mut meshes = Vec::new();
    let root = stage.prim(sdf::Path::abs_root()).map_err(error)?;
    let mut stack = root.children().map_err(error)?;
    while let Some(prim) = stack.pop() {
        if !prim.is_active().map_err(error)? || hidden(&prim)? {
            continue;
        }
        if prim
            .type_name()
            .map_err(error)?
            .is_some_and(|t| t.as_str() == "Mesh")
        {
            meshes.push(prim.clone());
        }
        stack.extend(prim.children().map_err(error)?);
    }
    meshes.sort_by(|a, b| a.path().as_str().cmp(b.path().as_str()));
    let mut parts = Vec::new();
    let mut joints = Vec::new();
    let mut channels = Vec::new();
    let mut vertices_total = 0usize;
    let mut cache_values = 0usize;
    let mut last = 0.0f64;
    for prim in &meshes {
        // The moments the mesh's place or points are keyed at.
        let moves = xform::sample_times(prim)?;
        let points_attr = prim.attribute("points");
        let point_times = points_attr.time_sample_times().map_err(error)?;
        if moves.len() > MAX_SAMPLES || point_times.len() > MAX_SAMPLES {
            return Err(format!(
                "{} is keyed at more than {MAX_SAMPLES} moments",
                prim.path()
            ));
        }
        let rest = moves.first().or(point_times.first()).copied();
        let at = |t: Option<f64>| t.map(usd::TimeCode::new);
        let Some(geometry) = geometry(prim, at(rest))? else {
            continue;
        };
        vertices_total += geometry.vertices.len();
        if vertices_total > MAX_VERTICES || parts.len() >= MAX_PARTS {
            return Err(format!(
                "the scene is too large (at most {MAX_VERTICES} vertices in {MAX_PARTS} parts)"
            ));
        }
        let world_rest = mul(&up, &world_at(prim, rest)?);
        let (colour, metallic, roughness) = material(prim, at(rest))?;
        let mut mesh = geometry.mesh(&world_rest);
        let mut part = Part {
            mesh: Mesh::default(),
            colour,
            metallic,
            roughness,
            texture: None,
            joint: None,
            rest_inverse: anim::IDENTITY,
            skin: None,
            cache: None,
        };
        if point_times.len() > 1 {
            // A point cache: every frame's points where its place puts them then.
            let mut frames = Vec::with_capacity(point_times.len());
            for &t in &point_times {
                let points = read_points(&points_attr, Some(usd::TimeCode::new(t)))?;
                if points.len() != geometry.points {
                    return Err(format!("{} changes its point count over time", prim.path()));
                }
                cache_values += points.len() * 3;
                if cache_values > MAX_CACHE_VALUES {
                    return Err("the scene's point caches are too large".into());
                }
                let world = mul(&up, &world_at(prim, Some(t))?);
                frames.push(points.iter().map(|p| xform::point(&world, *p)).collect());
                last = last.max(t);
            }
            part.cache = Some(Arc::new(PointCache {
                seconds: point_times.iter().map(|&t| seconds(t)).collect(),
                frames,
                point_of: geometry.point_of.clone(),
            }));
        } else if moves.len() > 1 {
            // Moved as one piece: a joint of its own keyed at every moment.
            let joint = joints.len();
            let rest_place = decompose(&world_rest);
            joints.push(Joint {
                parent: None,
                translation: rest_place.0,
                rotation: rest_place.1,
                scale: rest_place.2,
            });
            let mut keys: [Vec<[f32; 4]>; 3] = Default::default();
            for &t in &moves {
                let (tr, q, s) = decompose(&mul(&up, &world_at(prim, Some(t))?));
                keys[0].push([tr[0], tr[1], tr[2], 0.0]);
                keys[1].push(q);
                keys[2].push([s[0], s[1], s[2], 0.0]);
                last = last.max(t);
            }
            let times: Vec<f32> = moves.iter().map(|&t| seconds(t)).collect();
            for (keyed, values) in [Keyed::Translation, Keyed::Rotation, Keyed::Scale]
                .into_iter()
                .zip(keys)
            {
                channels.push(Channel {
                    node: joint,
                    keyed,
                    between: Between::Linear,
                    times: times.clone(),
                    values,
                });
            }
            part.joint = Some(joint);
            part.rest_inverse = anim::affine_inverse(&world_rest).unwrap_or(anim::IDENTITY);
        }
        // Veylvik's frame: y down (which mirrors, so triangles turn round).
        for v in &mut mesh.vertices {
            v.position[1] = -v.position[1];
            v.normal[1] = -v.normal[1];
        }
        for t in mesh.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        part.mesh = mesh;
        parts.push(part);
    }
    if parts.is_empty() {
        return Err("the USD file has no meshes".into());
    }
    // Normalise: centre the box on the origin and make its largest side 1.
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in parts.iter().flat_map(|p| &p.mesh.vertices) {
        for k in 0..3 {
            lo[k] = lo[k].min(v.position[k]);
            hi[k] = hi[k].max(v.position[k]);
        }
    }
    let size = [0, 1, 2].map(|k| hi[k] - lo[k]);
    let largest = size.iter().copied().fold(0.0f32, f32::max);
    if !largest.is_finite() || largest <= 0.0 {
        return Err("the scene has no size".into());
    }
    let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    for v in parts.iter_mut().flat_map(|p| &mut p.mesh.vertices) {
        v.position = [0, 1, 2].map(|k| (v.position[k] - centre[k]) / largest);
    }

    let animated = !channels.is_empty() || parts.iter().any(|p| p.cache.is_some());
    let end = stage.end_time_code();
    let length = seconds(if end.is_finite() && end > last {
        end
    } else {
        last
    });
    Ok(Model {
        parts,
        extent: size.map(|s| s / largest),
        joints,
        clips: if animated {
            vec![Clip {
                name: "Animation".into(),
                channels,
                length: length.max(0.0),
            }]
        } else {
            Vec::new()
        },
        // As glTF's: the box centre in Veylvik's frame (y down), which the normalised
        // frame is measured from.
        centre,
        largest,
    })
}
/// Whether the prim is made invisible (statically), or is only a guide.
fn hidden(prim: &usd::Prim) -> Result<bool> {
    let token = |name: &str| -> Result<Option<String>> {
        Ok(
            match prim.attribute(name).get::<vt::Value>().map_err(error)? {
                Some(vt::Value::Token(t)) => Some(t.as_str().to_owned()),
                _ => None,
            },
        )
    };
    Ok(token("visibility")?.as_deref() == Some("invisible")
        || matches!(token("purpose")?.as_deref(), Some("guide" | "proxy")))
}
/// A mesh's faces as triangles with a vertex for each face corner.
struct Geometry {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    /// The point each vertex stands on, and how many points there are.
    point_of: Vec<u32>,
    points: usize,
}
impl Geometry {
    /// The mesh placed by `world` (into the file's y-up frame).
    fn mesh(&self, world: &Mat) -> Mesh {
        let normals = xform::normal_matrix(world);
        Mesh {
            vertices: self
                .vertices
                .iter()
                .map(|v| Vertex {
                    position: xform::point(world, v.position),
                    normal: xform::unit(xform::direction(&normals, v.normal)),
                    uv: v.uv,
                })
                .collect(),
            indices: self.indices.clone(),
            cap_indices: 0,
        }
    }
}
fn read_points(attr: &usd::Attribute, at: Option<usd::TimeCode>) -> Result<Vec<[f32; 3]>> {
    Ok(attr
        .get_at::<vt::Value>(at)
        .map_err(error)?
        .map(vec3s)
        .unwrap_or_default())
}
fn vec3s(value: vt::Value) -> Vec<[f32; 3]> {
    match value {
        vt::Value::Vec3fVec(v) => v.iter().map(|p| [p.x, p.y, p.z]).collect(),
        vt::Value::Vec3dVec(v) => v
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect(),
        vt::Value::Vec3hVec(v) => v
            .iter()
            .map(|p| [p.x.to_f32(), p.y.to_f32(), p.z.to_f32()])
            .collect(),
        _ => Vec::new(),
    }
}
fn vec2s(value: vt::Value) -> Vec<[f32; 2]> {
    match value {
        vt::Value::Vec2fVec(v) => v.iter().map(|p| [p.x, p.y]).collect(),
        vt::Value::Vec2dVec(v) => v.iter().map(|p| [p.x as f32, p.y as f32]).collect(),
        vt::Value::Vec2hVec(v) => v.iter().map(|p| [p.x.to_f32(), p.y.to_f32()]).collect(),
        _ => Vec::new(),
    }
}
fn ints(value: Option<vt::Value>) -> Vec<i32> {
    match value {
        Some(vt::Value::IntVec(v)) => v,
        _ => Vec::new(),
    }
}
fn interpolation(attr: &usd::Attribute) -> Result<String> {
    Ok(
        match attr.metadata::<vt::Value>("interpolation").map_err(error)? {
            Some(vt::Value::Token(t)) => t.as_str().to_owned(),
            _ => "vertex".to_owned(),
        },
    )
}
/// A primvar's values for each face corner: `corner` is the corner's number, `point`
/// the point it stands on, `face` its face. None when the primvar is not there.
struct Primvar<T> {
    values: Vec<T>,
    indices: Vec<i32>,
    interpolation: String,
}
impl<T: Copy> Primvar<T> {
    fn at(&self, corner: usize, point: usize, face: usize) -> Option<T> {
        let k = match self.interpolation.as_str() {
            "faceVarying" => corner,
            "uniform" => face,
            "constant" => 0,
            _ => point,
        };
        let k = if self.indices.is_empty() {
            k
        } else {
            usize::try_from(*self.indices.get(k)?).ok()?
        };
        self.values.get(k).copied()
    }
}
fn primvar<T>(
    prim: &usd::Prim,
    name: &str,
    at: Option<usd::TimeCode>,
    read: impl Fn(vt::Value) -> Vec<T>,
) -> Result<Option<Primvar<T>>> {
    let attr = prim.attribute(name);
    let Some(value) = attr.get_at::<vt::Value>(at).map_err(error)? else {
        return Ok(None);
    };
    let indices = ints(
        prim.attribute(format!("{name}:indices"))
            .get_at::<vt::Value>(at)
            .map_err(error)?,
    );
    Ok(Some(Primvar {
        values: read(value),
        indices,
        interpolation: interpolation(&attr)?,
    }))
}
/// A mesh prim's faces at `at`, fanned into triangles, counter-clockwise seen from
/// outside in a right-handed y-up frame; None for a mesh with no faces.
fn geometry(prim: &usd::Prim, at: Option<usd::TimeCode>) -> Result<Option<Geometry>> {
    let points = read_points(&prim.attribute("points"), at)?;
    let counts = ints(
        prim.attribute("faceVertexCounts")
            .get_at::<vt::Value>(at)
            .map_err(error)?,
    );
    let corners = ints(
        prim.attribute("faceVertexIndices")
            .get_at::<vt::Value>(at)
            .map_err(error)?,
    );
    if points.is_empty() || counts.is_empty() {
        return Ok(None);
    }
    let left = matches!(
        prim.attribute("orientation").get::<vt::Value>().map_err(error)?,
        Some(vt::Value::Token(t)) if t.as_str() == "leftHanded"
    );
    let normals = match primvar(prim, "primvars:normals", at, vec3s)? {
        Some(n) => Some(n),
        None => primvar(prim, "normals", at, vec3s)?,
    };
    let uvs = match primvar(prim, "primvars:st", at, vec2s)? {
        Some(uv) => Some(uv),
        None => primvar(prim, "primvars:UVMap", at, vec2s)?,
    };
    let mut vertices = Vec::with_capacity(corners.len());
    let mut point_of = Vec::with_capacity(corners.len());
    let mut indices = Vec::new();
    let mut corner = 0usize;
    for (face, &count) in counts.iter().enumerate() {
        let count = usize::try_from(count).map_err(|_| "a face has a negative size")?;
        if corner + count > corners.len() {
            return Err(format!(
                "{} has more face corners than indices",
                prim.path()
            ));
        }
        let first = vertices.len() as u32;
        for k in 0..count {
            let c = corner + k;
            let p = usize::try_from(corners[c]).map_err(|_| "a negative point index")?;
            let position = *points
                .get(p)
                .ok_or_else(|| format!("{} indexes past its points", prim.path()))?;
            vertices.push(Vertex {
                position,
                normal: normals
                    .as_ref()
                    .and_then(|n| n.at(c, p, face))
                    .unwrap_or([0.0; 3]),
                // USD's v runs up; Veylvik's (as glTF's) down.
                uv: uvs
                    .as_ref()
                    .and_then(|u| u.at(c, p, face))
                    .map_or([0.0; 2], |[u, v]| [u, 1.0 - v]),
            });
            point_of.push(p as u32);
        }
        for k in 1..count.saturating_sub(1) {
            let (a, b) = (first + k as u32, first + k as u32 + 1);
            if left {
                indices.extend([first, b, a]);
            } else {
                indices.extend([first, a, b]);
            }
        }
        corner += count;
    }
    if indices.is_empty() {
        return Ok(None);
    }
    // Normals where the file has none: each point's faces summed (by area).
    if normals.is_none() {
        let mut sums = vec![[0.0f32; 3]; points.len()];
        for t in indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|v| vertices[v as usize].position);
            let (u, v) = (
                [0, 1, 2].map(|k| b[k] - a[k]),
                [0, 1, 2].map(|k| c[k] - a[k]),
            );
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            for &v in t {
                let p = point_of[v as usize] as usize;
                for k in 0..3 {
                    sums[p][k] += n[k];
                }
            }
        }
        for (v, p) in vertices.iter_mut().zip(&point_of) {
            v.normal = xform::unit(sums[*p as usize]);
        }
    } else if left {
        for v in &mut vertices {
            v.normal = v.normal.map(|x| -x);
        }
    }
    Ok(Some(Geometry {
        vertices,
        indices,
        point_of,
        points: points.len(),
    }))
}
fn colour_of(value: Option<vt::Value>) -> Option<[f32; 3]> {
    match value? {
        vt::Value::Vec3f(c) => Some([c.x, c.y, c.z]),
        vt::Value::Vec3d(c) => Some([c.x as f32, c.y as f32, c.z as f32]),
        vt::Value::Vec3fVec(v) => v.first().map(|c| [c.x, c.y, c.z]),
        _ => None,
    }
}
fn number_of(value: Option<vt::Value>) -> Option<f32> {
    match value? {
        vt::Value::Float(v) => Some(v),
        vt::Value::Double(v) => Some(v as f32),
        vt::Value::Half(v) => Some(v.to_f32()),
        _ => None,
    }
}
/// The base colour, metallic and roughness of the UsdPreviewSurface bound to the mesh
/// (its own values; a texture leaves the colour to the mesh's display colour), else the
/// display colour.
fn material(prim: &usd::Prim, at: Option<usd::TimeCode>) -> Result<([f32; 4], f32, f32)> {
    let display = colour_of(
        prim.attribute("primvars:displayColor")
            .get_at::<vt::Value>(at)
            .map_err(error)?,
    );
    let mut colour = display.unwrap_or([0.8; 3]);
    let (mut metallic, mut roughness) = (0.0, 0.5);
    let stage = prim.stage();
    // The binding on the mesh or the nearest prim above it.
    let mut bound = None;
    let mut look = Some(prim.clone());
    while let Some(p) = look {
        let targets = p
            .relationship("material:binding")
            .targets()
            .unwrap_or_default();
        if let Some(t) = targets.into_iter().next() {
            bound = Some(t);
            break;
        }
        look = p.parent();
    }
    if let Some(material) = bound.and_then(|path| stage.prim(path).ok()) {
        let surface = material
            .attribute("outputs:surface")
            .connections()
            .unwrap_or_default();
        if let Some(shader) = surface
            .into_iter()
            .next()
            .and_then(|out| stage.prim(out.prim_path()).ok())
        {
            let input = |name: &str| shader.attribute(format!("inputs:{name}"));
            let diffuse = input("diffuseColor");
            if diffuse.connections().unwrap_or_default().is_empty()
                && let Some(c) = colour_of(diffuse.get_at::<vt::Value>(at).map_err(error)?)
            {
                colour = c;
            }
            if let Some(m) = number_of(input("metallic").get_at::<vt::Value>(at).map_err(error)?) {
                metallic = m.clamp(0.0, 1.0);
            }
            if let Some(r) = number_of(input("roughness").get_at::<vt::Value>(at).map_err(error)?) {
                roughness = r.clamp(0.0, 1.0);
            }
        }
    }
    Ok(([colour[0], colour[1], colour[2], 1.0], metallic, roughness))
}
