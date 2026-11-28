//! USD scenes (`.usd`, `.usda`, `.usdc`, `.usdz`) read into a Veylvik `Model`, the same as an imported glTF: every active mesh of the composed stage with its transforms, normals, uvs and base material, turned y up and normalised; transforms keyed over time become the model's clip, and points keyed over time become a point cache that bends the part. Read through the pure-Rust `openusd` crate; nothing else in Veylvik sees its types.
use openusd::{sdf, usd, vt};
use std::path::Path;
use veylvik_mesh::animation as anim;
use veylvik_mesh::baked::{BakedCache, BakedCamera, BakedPart, bake};
use veylvik_mesh::{Mesh, Model, Vertex};

mod xform;
use xform::{mul, world_at};

/// The file extensions read as USD.
pub const EXTENSIONS: [&str; 4] = ["usd", "usda", "usdc", "usdz"];
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
    let mut camera_prims = Vec::new();
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
        if prim
            .type_name()
            .map_err(error)?
            .is_some_and(|t| t.as_str() == "Camera")
        {
            camera_prims.push(prim.clone());
        }
        stack.extend(prim.children().map_err(error)?);
    }
    meshes.sort_by(|a, b| a.path().as_str().cmp(b.path().as_str()));
    camera_prims.sort_by(|a, b| a.path().as_str().cmp(b.path().as_str()));
    let mut parts = Vec::new();
    let mut last = 0.0f64;
    let at = |t: Option<f64>| t.map(usd::TimeCode::new);
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
        let Some(geometry) = geometry(prim, at(rest))? else {
            continue;
        };
        let (colour, metallic, roughness) = material(prim, at(rest))?;
        let mut places = vec![(rest.map_or(0.0, seconds), mul(&up, &world_at(prim, rest)?))];
        if moves.len() > 1 {
            places.clear();
            for &t in &moves {
                places.push((seconds(t), mul(&up, &world_at(prim, Some(t))?)));
                last = last.max(t);
            }
        }
        let cache = if point_times.len() > 1 {
            // Every frame's points where its place puts them then.
            let mut frames = Vec::with_capacity(point_times.len());
            for &t in &point_times {
                let points = read_points(&points_attr, Some(usd::TimeCode::new(t)))?;
                if points.len() != geometry.points {
                    return Err(format!("{} changes its point count over time", prim.path()));
                }
                let world = mul(&up, &world_at(prim, Some(t))?);
                frames.push(points.iter().map(|p| xform::point(&world, *p)).collect());
                last = last.max(t);
            }
            Some(BakedCache {
                seconds: point_times.iter().map(|&t| seconds(t)).collect(),
                frames,
                point_of: geometry.point_of.clone(),
            })
        } else {
            None
        };
        parts.push(BakedPart {
            mesh: Mesh {
                vertices: geometry.vertices,
                indices: geometry.indices,
                cap_indices: 0,
            },
            colour,
            metallic,
            roughness,
            places,
            cache,
        });
    }
    // Each camera's keys: seconds, camera to world (y up), lens.
    let mut cameras = Vec::new();
    for prim in camera_prims
        .iter()
        .take(veylvik_document::nodes::MAX_MODEL_CAMERAS)
    {
        let lens = |name: &str| prim.attribute(name);
        let mut times = xform::sample_times(prim)?;
        for name in ["focalLength", "horizontalAperture"] {
            times.extend(lens(name).time_sample_times().map_err(error)?);
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        if times.len() > MAX_SAMPLES {
            return Err(format!(
                "{} is keyed at more than {MAX_SAMPLES} moments",
                prim.path()
            ));
        }
        let moments: Vec<Option<f64>> = if times.is_empty() {
            vec![None]
        } else {
            times.iter().map(|&t| Some(t)).collect()
        };
        let mut keys = Vec::with_capacity(moments.len());
        for when in moments {
            let code = at(when);
            // USD's own fallbacks, 50 and 20.955, in the same units: only their ratio
            // (the field of view) matters.
            let focal = number_of(
                lens("focalLength")
                    .get_at::<vt::Value>(code)
                    .map_err(error)?,
            )
            .unwrap_or(50.0);
            let aperture = number_of(
                lens("horizontalAperture")
                    .get_at::<vt::Value>(code)
                    .map_err(error)?,
            )
            .unwrap_or(20.955);
            if !(focal > 0.0 && aperture > 0.0) {
                continue;
            }
            keys.push((
                when.map_or(0.0, seconds),
                mul(&up, &world_at(prim, when)?),
                focal,
                aperture,
            ));
            if let Some(t) = when {
                last = last.max(t);
            }
        }
        cameras.push(BakedCamera {
            name: prim.path().name().unwrap_or("Camera").to_owned(),
            keys,
        });
    }
    if parts.is_empty() {
        return Err("the USD file has no meshes".into());
    }
    let end = stage.end_time_code();
    let length = seconds(if end.is_finite() && end > last {
        end
    } else {
        last
    });
    bake(parts, cameras, length)
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
