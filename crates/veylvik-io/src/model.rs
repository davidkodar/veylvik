//! glTF 2.0 import (.glb or .gltf with its buffers and images): every triangle part of the default scene with its node transforms and metallic-roughness material, in Veylvik's frame (y down), normalised to a unit box.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use veylvik_mesh::animation::{self as anim, Between, Channel, Clip, Joint, Keyed};
use veylvik_mesh::{
    MAX_PARTS, MAX_TEXTURE, MAX_VERTICES, Mesh, Model, Part, Skin, Texture, Vertex,
};

type Result<T> = std::result::Result<T, String>;
type Mat = [[f32; 4]; 4];

fn mul(a: &Mat, b: &Mat) -> Mat {
    // glTF matrices are column-major: m[column][row].
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, value) in column.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}
fn point(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}
fn direction(m: &Mat, d: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[0][r] * d[0] + m[1][r] * d[1] + m[2][r] * d[2])
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 {
        v.map(|x| x / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}
fn srgb(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
/// A glTF image as linear straight RGBA, box-reduced to at most `MAX_TEXTURE` a side.
fn texture(data: &gltf::image::Data) -> Option<Texture> {
    use gltf::image::Format as F;
    let (w, h) = (data.width as usize, data.height as usize);
    let channels = match data.format {
        F::R8 => 1,
        F::R8G8 => 2,
        F::R8G8B8 => 3,
        F::R8G8B8A8 => 4,
        _ => return None,
    };
    if w == 0 || h == 0 || data.pixels.len() < w * h * channels {
        return None;
    }
    let texel = |x: usize, y: usize| -> [f32; 4] {
        let p = &data.pixels[(y * w + x) * channels..][..channels];
        let v = |i: usize| f32::from(p[i]) / 255.0;
        match channels {
            1 => [srgb(v(0)), srgb(v(0)), srgb(v(0)), 1.0],
            2 => [srgb(v(0)), srgb(v(0)), srgb(v(0)), v(1)],
            3 => [srgb(v(0)), srgb(v(1)), srgb(v(2)), 1.0],
            _ => [srgb(v(0)), srgb(v(1)), srgb(v(2)), v(3)],
        }
    };
    let step = (w.max(h) as u32).div_ceil(MAX_TEXTURE).max(1) as usize;
    let (tw, th) = (w.div_ceil(step), h.div_ceil(step));
    let mut pixels = Vec::with_capacity(tw * th);
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = [0.0f32; 4];
            let mut n = 0.0;
            for y in ty * step..((ty + 1) * step).min(h) {
                for x in tx * step..((tx + 1) * step).min(w) {
                    let t = texel(x, y);
                    for c in 0..4 {
                        sum[c] += t[c];
                    }
                    n += 1.0;
                }
            }
            pixels.push(sum.map(|v| v / n));
        }
    }
    Some(Texture {
        width: tw as u32,
        height: th as u32,
        pixels,
    })
}
/// Load a 3D model normalised to a unit box (largest side 1, centred): a glTF file, or a
/// USD scene (`.usd`, `.usda`, `.usdc`, `.usdz`).
pub fn load(path: &Path) -> Result<Model> {
    let usd = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| veylvik_usd::EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    if usd {
        return veylvik_usd::load(path);
    }
    load_gltf(path)
}
/// Load a glTF model normalised to a unit box (largest side 1, centred).
fn load_gltf(path: &Path) -> Result<Model> {
    let (document, buffers, images) =
        gltf::import(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or("the file has no scene")?;
    let mut textures: HashMap<usize, Option<Arc<Texture>>> = HashMap::new();
    let mut parts = Vec::new();
    let mut vertices_total = 0usize;
    let identity: Mat = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut pending: Vec<(gltf::Node, Mat)> = scene.nodes().map(|n| (n, identity)).collect();
    while let Some((node, parent)) = pending.pop() {
        let world = mul(&parent, &node.transform().matrix());
        pending.extend(node.children().map(|c| (c, world)));
        let Some(mesh) = node.mesh() else { continue };
        // A skinned mesh is placed by its joints, not by its node (glTF's rule).
        let skin = node.skin();
        let world = if skin.is_some() { identity } else { world };
        // A mirroring transform turns faces inside out; flip them back.
        let m3 = |c: usize, r: usize| world[c][r];
        let det = m3(0, 0) * (m3(1, 1) * m3(2, 2) - m3(2, 1) * m3(1, 2))
            - m3(1, 0) * (m3(0, 1) * m3(2, 2) - m3(2, 1) * m3(0, 2))
            + m3(2, 0) * (m3(0, 1) * m3(1, 2) - m3(1, 1) * m3(0, 2));
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<[f32; 3]> = positions.collect();
            vertices_total += positions.len();
            if vertices_total > MAX_VERTICES || parts.len() >= MAX_PARTS {
                return Err(format!(
                    "the model is too large (at most {MAX_VERTICES} vertices in {MAX_PARTS} parts)"
                ));
            }
            let mut indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            indices.truncate(indices.len() / 3 * 3);
            if indices.iter().any(|i| *i as usize >= positions.len()) {
                return Err("a part indexes past its vertices".into());
            }
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let normals_raw = normals.clone();
            let uvs: Option<Vec<[f32; 2]>> =
                reader.read_tex_coords(0).map(|t| t.into_f32().collect());
            let skinned = skin.as_ref().map(|skin| {
                (
                    skin,
                    reader
                        .read_joints(0)
                        .map(|j| j.into_u16().collect::<Vec<_>>()),
                    reader
                        .read_weights(0)
                        .map(|w| w.into_f32().collect::<Vec<_>>()),
                )
            });
            // Into Veylvik's frame: world transform, then y flipped (glTF is y up), which
            // mirrors, so triangle order flips too (twice for a mirroring transform).
            let flip = |v: [f32; 3]| [v[0], -v[1], v[2]];
            let placed: Vec<[f32; 3]> = positions.iter().map(|p| flip(point(&world, *p))).collect();
            if det > 0.0 {
                for t in indices.as_chunks_mut::<3>().0 {
                    t.swap(1, 2);
                }
            }
            let normals: Vec<[f32; 3]> = match normals {
                Some(n) if n.len() == positions.len() => n
                    .iter()
                    .map(|v| normalize(flip(direction(&world, *v))))
                    .collect(),
                _ => {
                    // Smooth normals from the faces around each vertex.
                    let mut sum = vec![[0.0f32; 3]; placed.len()];
                    for t in indices.as_chunks::<3>().0 {
                        let [a, b, c] = [0, 1, 2].map(|k| placed[t[k] as usize]);
                        let (u, v) = (
                            [0, 1, 2].map(|k| b[k] - a[k]),
                            [0, 1, 2].map(|k| c[k] - a[k]),
                        );
                        let n = [
                            u[1] * v[2] - u[2] * v[1],
                            u[2] * v[0] - u[0] * v[2],
                            u[0] * v[1] - u[1] * v[0],
                        ];
                        for &i in t {
                            for k in 0..3 {
                                sum[i as usize][k] += n[k];
                            }
                        }
                    }
                    sum.into_iter().map(normalize).collect()
                }
            };
            let vertices = placed
                .iter()
                .enumerate()
                .map(|(i, p)| Vertex {
                    position: *p,
                    normal: normals[i],
                    uv: uvs
                        .as_ref()
                        .and_then(|u| u.get(i))
                        .copied()
                        .unwrap_or([0.0; 2]),
                })
                .collect();
            let material = primitive.material();
            let pbr = material.pbr_metallic_roughness();
            let texture = pbr.base_color_texture().and_then(|info| {
                let index = info.texture().source().index();
                textures
                    .entry(index)
                    .or_insert_with(|| images.get(index).and_then(texture).map(Arc::new))
                    .clone()
            });
            let skin = match skinned {
                Some((skin, Some(influences), Some(weights)))
                    if influences.len() == positions.len() && weights.len() == positions.len() =>
                {
                    let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
                    let binds: Vec<Mat> = skin
                        .reader(|b| buffers.get(b.index()).map(|d| &d.0[..]))
                        .read_inverse_bind_matrices()
                        .map(|m| m.collect())
                        .unwrap_or_else(|| vec![identity; joints.len()]);
                    if binds.len() != joints.len()
                        || influences
                            .iter()
                            .flatten()
                            .any(|&j| usize::from(j) >= joints.len())
                    {
                        return Err("a skin names joints it does not have".into());
                    }
                    let weights = weights
                        .into_iter()
                        .map(|w| {
                            let sum: f32 = w.iter().sum();
                            if sum > 1e-6 {
                                w.map(|v| v / sum)
                            } else {
                                [1.0, 0.0, 0.0, 0.0]
                            }
                        })
                        .collect();
                    Some(Arc::new(Skin {
                        joints,
                        // glTF's matrices are by column; ours by row.
                        inverse_bind: binds
                            .iter()
                            .map(|b| std::array::from_fn(|r| std::array::from_fn(|c| b[c][r])))
                            .collect(),
                        positions: positions.clone(),
                        normals: vertices_normals_in_mesh(&normals_raw, &positions, &indices),
                        influences,
                        weights,
                    }))
                }
                _ => None,
            };
            parts.push(Part {
                mesh: Mesh {
                    vertices,
                    indices,
                    cap_indices: 0,
                },
                colour: pbr.base_color_factor(),
                metallic: pbr.metallic_factor().clamp(0.0, 1.0),
                roughness: pbr.roughness_factor().clamp(0.0, 1.0),
                texture,
                joint: Some(node.index()),
                // glTF's matrices are by column; ours by row.
                rest_inverse: anim::affine_inverse(&std::array::from_fn(|r| {
                    std::array::from_fn(|c| world[c][r])
                }))
                .unwrap_or(anim::IDENTITY),
                skin,
                cache: None,
            });
        }
    }
    if parts.is_empty() {
        return Err("the model has no triangle meshes".into());
    }
    let (joints, clips) = animation(&document, &buffers)?;
    // Skinned parts at rest, as their joints place them (still unnormalised, y down).
    if parts.iter().any(|p| p.skin.is_some()) {
        let rested = Model {
            joints: joints.clone(),
            ..Model::default()
        };
        let worlds = rested.rest_worlds();
        let mut flip = anim::IDENTITY;
        flip[1][1] = -1.0;
        for part in &mut parts {
            if let Some(skin) = part.skin.clone() {
                part.mesh = anim::skin_mesh(&part.mesh, &skin, &worlds, &flip);
            }
        }
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
        return Err("the model has no size".into());
    }
    let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    for v in parts.iter_mut().flat_map(|p| &mut p.mesh.vertices) {
        v.position = [0, 1, 2].map(|k| (v.position[k] - centre[k]) / largest);
    }
    Ok(Model {
        parts,
        extent: size.map(|s| s / largest),
        joints,
        clips,
        centre,
        largest,
    })
}
/// The file's node tree at rest and its animations (moves, turns and sizes of nodes;
/// morph target weights are left out).
fn animation(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Result<(Vec<Joint>, Vec<Clip>)> {
    let mut joints: Vec<Joint> = document
        .nodes()
        .map(|n| {
            let (translation, rotation, scale) = n.transform().decomposed();
            Joint {
                parent: None,
                translation,
                rotation,
                scale,
            }
        })
        .collect();
    for n in document.nodes() {
        for child in n.children() {
            if let Some(j) = joints.get_mut(child.index()) {
                j.parent = Some(n.index());
            }
        }
    }
    let mut clips = Vec::new();
    let mut keys = 0usize;
    for (k, a) in document
        .animations()
        .enumerate()
        .take(veylvik_document::nodes::MAX_MODEL_CLIPS)
    {
        let mut channels = Vec::new();
        for channel in a.channels() {
            let reader = channel.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let (Some(inputs), Some(outputs)) = (reader.read_inputs(), reader.read_outputs())
            else {
                continue;
            };
            use gltf::animation::util::ReadOutputs;
            let (keyed, values): (Keyed, Vec<[f32; 4]>) = match outputs {
                ReadOutputs::Translations(v) => (
                    Keyed::Translation,
                    v.map(|[x, y, z]| [x, y, z, 0.0]).collect(),
                ),
                ReadOutputs::Rotations(v) => (Keyed::Rotation, v.into_f32().collect()),
                ReadOutputs::Scales(v) => {
                    (Keyed::Scale, v.map(|[x, y, z]| [x, y, z, 0.0]).collect())
                }
                ReadOutputs::MorphTargetWeights(_) => continue,
            };
            let times: Vec<f32> = inputs.collect();
            keys += times.len();
            if keys > anim::MAX_KEYS {
                return Err(format!(
                    "the model's animations hold over {} keys",
                    anim::MAX_KEYS
                ));
            }
            let between = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Step => Between::Step,
                gltf::animation::Interpolation::Linear => Between::Linear,
                gltf::animation::Interpolation::CubicSpline => Between::CubicSpline,
            };
            let wanted = if between == Between::CubicSpline {
                3 * times.len()
            } else {
                times.len()
            };
            if values.len() < wanted
                || times.iter().any(|t| !t.is_finite())
                || times.windows(2).any(|w| w[1] < w[0])
            {
                return Err(format!(
                    "animation {} has keys out of order or missing",
                    k + 1
                ));
            }
            channels.push(Channel {
                node: channel.target().node().index(),
                keyed,
                between,
                times,
                values,
            });
        }
        clips.push(Clip {
            length: 0.0,
            name: a
                .name()
                .map_or_else(|| format!("Animation {}", k + 1), str::to_owned),
            channels,
        });
    }
    Ok((joints, clips))
}
/// A part's normals in its mesh's own space: the file's, or smooth ones from its faces.
fn vertices_normals_in_mesh(
    given: &Option<Vec<[f32; 3]>>,
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Vec<[f32; 3]> {
    if let Some(n) = given.as_ref().filter(|n| n.len() == positions.len()) {
        return n.iter().map(|v| normalize(*v)).collect();
    }
    let mut sum = vec![[0.0f32; 3]; positions.len()];
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = [0, 1, 2].map(|k| positions[t[k] as usize]);
        let (u, v) = (
            [0, 1, 2].map(|k| b[k] - a[k]),
            [0, 1, 2].map(|k| c[k] - a[k]),
        );
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &i in t {
            for k in 0..3 {
                sum[i as usize][k] += n[k];
            }
        }
    }
    sum.into_iter().map(normalize).collect()
}
#[cfg(test)]
mod tests {
    #[test]
    fn a_models_own_animations_move_its_parts_as_the_file_keys_them() {
        // A 1 x 1 quad at the origin (glTF, y up) with two clips on its node: Slide moves
        // it 2 to the right over a second; Turn turns it a quarter about z.
        let mut bin = Vec::new();
        let mut put = |values: &[f32]| {
            values
                .iter()
                .for_each(|v| bin.extend_from_slice(&v.to_le_bytes()))
        };
        put(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0]);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let mut tail = Vec::new();
        for i in [0u16, 1, 2, 0, 2, 3] {
            tail.extend_from_slice(&i.to_le_bytes());
        }
        bin.extend_from_slice(&tail);
        let mut put = |values: &[f32]| {
            values
                .iter()
                .for_each(|v| bin.extend_from_slice(&v.to_le_bytes()))
        };
        put(&[0.0, 1.0]);
        put(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        put(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, h, h]);
        let dir = std::env::temp_dir().join(format!("veylvik-moving-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("quad.bin"), &bin).unwrap();
        let gltf = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
            "buffers": [{"uri": "quad.bin", "byteLength": bin.len()}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 48},
                {"buffer": 0, "byteOffset": 48, "byteLength": 12},
                {"buffer": 0, "byteOffset": 60, "byteLength": 8},
                {"buffer": 0, "byteOffset": 68, "byteLength": 24},
                {"buffer": 0, "byteOffset": 92, "byteLength": 32}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
                {"bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR"},
                {"bufferView": 2, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1]},
                {"bufferView": 3, "componentType": 5126, "count": 2, "type": "VEC3"},
                {"bufferView": 4, "componentType": 5126, "count": 2, "type": "VEC4"}
            ],
            "animations": [
                {"name": "Slide", "samplers": [{"input": 2, "output": 3}], "channels": [{"sampler": 0, "target": {"node": 0, "path": "translation"}}]},
                {"name": "Turn", "samplers": [{"input": 2, "output": 4}], "channels": [{"sampler": 0, "target": {"node": 0, "path": "rotation"}}]}
            ]
        });
        std::fs::write(dir.join("quad.gltf"), gltf.to_string()).unwrap();
        let model = super::load(&dir.join("quad.gltf")).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let names: Vec<_> = model
            .clips
            .iter()
            .map(|c| (c.name.as_str(), c.seconds()))
            .collect();
        assert_eq!(names, [("Slide", 1.0), ("Turn", 1.0)]);
        let moved = |clip, t, looping, p: [f32; 3]| {
            let m = model.poses(clip, t, looping).unwrap()[0];
            [0, 1, 2].map(|i| (0..3).map(|k| m[i][k] * p[k]).sum::<f32>() + m[i][3])
        };
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        // The quad's corner glTF (1, 0) is (0.5, 0.5) in the normalised frame (y down).
        let corner = [0.5, 0.5, 0.0];
        assert!(near(moved(0, 0.0, true, corner), corner));
        assert!(
            near(moved(0, 0.5, true, corner), [1.5, 0.5, 0.0]),
            "{:?}",
            moved(0, 0.5, true, corner)
        );
        assert!(
            near(moved(0, 1.5, true, corner), [1.5, 0.5, 0.0]),
            "looped round"
        );
        assert!(
            near(moved(0, 1.5, false, corner), [2.5, 0.5, 0.0]),
            "held at its end"
        );
        // A quarter turn about z takes glTF (1, 0) to (0, 1): in the normalised frame, the
        // top-left corner (-0.5, -0.5).
        assert!(
            near(moved(1, 1.0, false, corner), [-0.5, -0.5, 0.0]),
            "{:?}",
            moved(1, 1.0, false, corner)
        );
        assert!(model.poses(2, 0.0, true).is_none());
    }
    #[test]
    fn loads_a_textured_part_upright_and_normalised() {
        // A 2 x 1 quad in the xy plane, 4 units up (glTF y is up), with a 2 x 1 texture.
        let mut bin = Vec::new();
        for p in [
            [0.0f32, 4.0, 0.0],
            [2.0, 4.0, 0.0],
            [2.0, 5.0, 0.0],
            [0.0, 5.0, 0.0],
        ] {
            for v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for uv in [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]] {
            for v in uv {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for i in [0u16, 1, 2, 0, 2, 3] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let dir = std::env::temp_dir().join(format!("veylvik-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("quad.bin"), &bin).unwrap();
        // A red then blue 2 x 1 PNG.
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 2, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[255, 0, 0, 0, 0, 255])
                .unwrap();
        }
        std::fs::write(dir.join("colours.png"), &png).unwrap();
        let gltf = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "TEXCOORD_0": 1}, "indices": 2, "material": 0}]}],
            "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.25, "roughnessFactor": 0.75}}],
            "textures": [{"source": 0}], "images": [{"uri": "colours.png"}],
            "buffers": [{"uri": "quad.bin", "byteLength": bin.len()}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 48},
                {"buffer": 0, "byteOffset": 48, "byteLength": 32},
                {"buffer": 0, "byteOffset": 80, "byteLength": 12}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0, 4, 0], "max": [2, 5, 0]},
                {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"},
                {"bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"}
            ]
        });
        std::fs::write(dir.join("quad.gltf"), gltf.to_string()).unwrap();
        let model = super::load(&dir.join("quad.gltf")).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(model.parts.len(), 1);
        assert_eq!(model.extent, [1.0, 0.5, 0.0]);
        let part = &model.parts[0];
        assert_eq!((part.metallic, part.roughness), (0.25, 0.75));
        // Centred, largest side 1, and y down: glTF's top edge (y 5) is now at -0.25.
        let top = part.mesh.vertices[2].position;
        assert!(
            (top[0] - 0.5).abs() < 1e-6 && (top[1] + 0.25).abs() < 1e-6,
            "{top:?}"
        );
        // Normals face the viewer (+z), and the texture is linear red then blue.
        assert!(
            part.mesh.vertices.iter().all(|v| v.normal[2] > 0.99),
            "{:?}",
            part.mesh.vertices[0].normal
        );
        let t = part.texture.as_ref().unwrap();
        assert_eq!((t.width, t.height), (2, 1));
        assert!(t.pixels[0][0] > 0.99 && t.pixels[1][2] > 0.99 && t.pixels[1][0] < 0.01);
    }
}
