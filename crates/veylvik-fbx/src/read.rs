//! Reading an FBX file with ufbx, in the helper process: the scene turned right-handed y up with cameras looking along -z, each mesh split by material into parts of triangles, the file's animation sampled at its frame rate (each mesh's place, its bent points when it has deformers, each camera's place and lens), all handed to the shared baked-scene builder.
use ufbx::{CoordinateAxes, CoordinateAxis};
use veylvik_mesh::animation::Mat;
use veylvik_mesh::baked::{BakedCache, BakedCamera, BakedPart, bake};
use veylvik_mesh::{Mesh, Model, Vertex};

/// The most frames of animation sampled.
const MOST_FRAMES: usize = 20_000;

fn matrix(m: &ufbx::Matrix) -> Mat {
    let r = |v: f64| v as f32;
    [
        [r(m.m00), r(m.m01), r(m.m02), r(m.m03)],
        [r(m.m10), r(m.m11), r(m.m12), r(m.m13)],
        [r(m.m20), r(m.m21), r(m.m22), r(m.m23)],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
fn vec3(v: ufbx::Vec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}
/// A mesh's corners, each with the control point it stands on, and its triangles,
/// split into one group for each material.
struct Split {
    material: Option<usize>,
    vertices: Vec<Vertex>,
    point_of: Vec<u32>,
    indices: Vec<u32>,
}
fn split(mesh: &ufbx::Mesh) -> Vec<Split> {
    let mut out: Vec<Split> = Vec::new();
    let mut scratch = vec![0u32; mesh.max_face_triangles * 3];
    for (f, face) in mesh.faces.iter().enumerate() {
        let material = mesh.face_material.get(f).map(|m| *m as usize);
        let group = match out.iter().position(|s| s.material == material) {
            Some(g) => g,
            None => {
                out.push(Split {
                    material,
                    vertices: Vec::new(),
                    point_of: Vec::new(),
                    indices: Vec::new(),
                });
                out.len() - 1
            }
        };
        let s = &mut out[group];
        let triangles = mesh.triangulate_face(&mut scratch, *face) as usize;
        for &index in &scratch[..triangles * 3] {
            let i = index as usize;
            let normal = if mesh.vertex_normal.exists {
                vec3(mesh.vertex_normal[i])
            } else {
                [0.0, 1.0, 0.0]
            };
            let uv = if mesh.vertex_uv.exists {
                let t = mesh.vertex_uv[i];
                // FBX's v runs up; Veylvik's (as glTF's) down.
                [t.x as f32, 1.0 - t.y as f32]
            } else {
                [0.0; 2]
            };
            s.indices.push(s.vertices.len() as u32);
            s.vertices.push(Vertex {
                position: vec3(mesh.vertex_position[i]),
                normal,
                uv,
            });
            s.point_of
                .push(mesh.vertex_indices.get(i).copied().unwrap_or(0));
        }
    }
    out
}
fn material(m: Option<&ufbx::Material>) -> ([f32; 4], f32, f32) {
    let Some(m) = m else {
        return ([0.8, 0.8, 0.8, 1.0], 0.0, 0.5);
    };
    let colour = if m.pbr.base_color.has_value {
        m.pbr.base_color.value_vec4
    } else {
        m.fbx.diffuse_color.value_vec4
    };
    let factor = if m.pbr.base_factor.has_value {
        m.pbr.base_factor.value_vec4.x
    } else {
        1.0
    };
    let metallic = if m.pbr.metalness.has_value {
        m.pbr.metalness.value_vec4.x
    } else {
        0.0
    };
    let roughness = if m.pbr.roughness.has_value {
        m.pbr.roughness.value_vec4.x
    } else {
        0.5
    };
    (
        [
            (colour.x * factor) as f32,
            (colour.y * factor) as f32,
            (colour.z * factor) as f32,
            1.0,
        ],
        (metallic as f32).clamp(0.0, 1.0),
        (roughness as f32).clamp(0.0, 1.0),
    )
}
/// The FBX file at `path` as a model, read in this process.
pub fn read(path: &std::path::Path) -> Result<Model, String> {
    let name = path.to_str().ok_or("the FBX file's path is not UTF-8")?;
    let y_up = CoordinateAxes::right_handed_y_up();
    let opts = ufbx::LoadOpts {
        target_axes: y_up,
        // Cameras as Veylvik's: right +x, up +y, looking along -z.
        target_camera_axes: CoordinateAxes {
            right: CoordinateAxis::PositiveX,
            up: CoordinateAxis::PositiveY,
            front: CoordinateAxis::PositiveZ,
        },
        generate_missing_normals: true,
        ..Default::default()
    };
    let scene = ufbx::load_file(name, opts)
        .map_err(|e| format!("{}: {}", path.display(), e.description))?;
    let anim = &scene.anim;
    let rate = if scene.settings.frames_per_second > 0.0 {
        scene.settings.frames_per_second
    } else {
        24.0
    };
    let (begin, end) = (anim.time_begin, anim.time_end);
    let frames = if end > begin {
        (((end - begin) * rate).round() as usize + 1).min(MOST_FRAMES)
    } else {
        1
    };
    let times: Vec<f64> = (0..frames).map(|k| begin + k as f64 / rate).collect();
    let seconds = |t: f64| (t - begin) as f32;
    // Each mesh node, its groups and what they show; each camera node.
    let mesh_nodes: Vec<usize> = scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.mesh.is_some() && n.visible)
        .map(|(k, _)| k)
        .collect();
    let camera_nodes: Vec<usize> = scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.camera.is_some())
        .map(|(k, _)| k)
        .collect();
    // The scene at every frame (only when something is animated).
    let evaluated: Vec<ufbx::SceneRoot> = if frames > 1 {
        let mut out = Vec::with_capacity(frames);
        for &t in &times {
            let opts = ufbx::EvaluateOpts {
                evaluate_skinning: true,
                ..Default::default()
            };
            out.push(
                ufbx::evaluate_scene(&scene, anim, t, opts)
                    .map_err(|e| e.description.to_string())?,
            );
        }
        out
    } else {
        Vec::new()
    };
    let mut parts = Vec::new();
    for &k in &mesh_nodes {
        let node = &scene.nodes[k];
        let mesh = node.mesh.as_ref().expect("a mesh node");
        let deforms = !mesh.skin_deformers.is_empty()
            || !mesh.blend_deformers.is_empty()
            || !mesh.cache_deformers.is_empty();
        let rest = matrix(&node.geometry_to_world);
        // Where it is at each frame, kept only when it moves.
        let mut places = vec![(0.0, rest)];
        if !evaluated.is_empty() && !deforms {
            let all: Vec<(f32, Mat)> = evaluated
                .iter()
                .zip(&times)
                .map(|(e, &t)| (seconds(t), matrix(&e.nodes[k].geometry_to_world)))
                .collect();
            if all.windows(2).any(|w| w[0].1 != w[1].1) {
                places = all;
            }
        }
        // The bent points of every frame, in the world.
        let cache_frames: Option<Vec<Vec<[f32; 3]>>> =
            (deforms && !evaluated.is_empty()).then(|| {
                evaluated
                    .iter()
                    .map(|e| {
                        let n = &e.nodes[k];
                        let m = n.mesh.as_ref().expect("the same mesh node");
                        let world = matrix(&n.geometry_to_world);
                        (0..m.num_vertices)
                            .map(|v| {
                                let index =
                                    m.vertex_first_index.get(v).copied().unwrap_or(0) as usize;
                                let p = vec3(if m.skinned_position.exists {
                                    m.skinned_position[index]
                                } else {
                                    m.vertex_position[index]
                                });
                                if m.skinned_is_local {
                                    point(&world, p)
                                } else {
                                    p
                                }
                            })
                            .collect()
                    })
                    .collect()
            });
        for group in split(mesh) {
            if group.indices.is_empty() {
                continue;
            }
            let (colour, metallic, roughness) = material(
                group
                    .material
                    .and_then(|m| mesh.materials.get(m))
                    .map(|m| &**m),
            );
            let cache = cache_frames.as_ref().map(|frames| BakedCache {
                seconds: times.iter().map(|&t| seconds(t)).collect(),
                frames: frames.clone(),
                point_of: group.point_of.clone(),
            });
            parts.push(BakedPart {
                mesh: Mesh {
                    vertices: group.vertices,
                    indices: group.indices,
                    cap_indices: 0,
                },
                colour,
                metallic,
                roughness,
                places: places.clone(),
                cache,
            });
        }
    }
    let mut cameras = Vec::new();
    for &k in &camera_nodes {
        let lens = |n: &ufbx::Node| {
            let c = n.camera.as_ref().expect("a camera node");
            (c.focal_length_mm as f32, (c.film_size_inch.x * 25.4) as f32)
        };
        let keyed: Vec<(f32, Mat, f32, f32)> = if evaluated.is_empty() {
            let n = &scene.nodes[k];
            let (f, s) = lens(n);
            vec![(0.0, matrix(&n.node_to_world), f, s)]
        } else {
            evaluated
                .iter()
                .zip(&times)
                .map(|(e, &t)| {
                    let n = &e.nodes[k];
                    let (f, s) = lens(n);
                    (seconds(t), matrix(&n.node_to_world), f, s)
                })
                .collect()
        };
        let keyed: Vec<_> = keyed
            .into_iter()
            .filter(|(_, _, f, s)| *f > 0.0 && *s > 0.0)
            .collect();
        cameras.push(BakedCamera {
            name: scene.nodes[k].element.name.to_string(),
            keys: keyed,
        });
    }
    bake(
        parts,
        cameras,
        if frames > 1 {
            (end - begin) as f32
        } else {
            0.0
        },
    )
}
fn point(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}
