//! A Render scene's render passes (normals, position, motion, Cryptomatte objects), cast by the tracer through the same camera as the picture: every object it would draw solid, with each placing named by its node's id and, for motion, placed where it was a frame before and is a frame after.
use crate::traced::{copied, read_copies};
use crate::{Engine, error};
use veylvik_document::NodeId;
use veylvik_document::nodes::ScenePass;
use veylvik_render::{Result, SceneJob};
use veylvik_scene::Geometry;
use veylvik_scene::math::Mat4;
use veylvik_trace::{CARD, CUT, Moved, Pass, PassJob, Placed, Surface, Texture, Tracer, View};

impl Engine {
    pub(crate) fn render_pass(&self, job: SceneJob<'_>) -> Result<()> {
        let SceneJob {
            device,
            queue,
            encoder,
            scene,
            camera,
            width,
            height,
            images,
            target,
            models,
            instances,
            pass,
            ids,
            moved,
            ..
        } = job;
        let pass = match pass {
            ScenePass::Picture => return Err(error("the picture is not a render pass")),
            ScenePass::Normals => Pass::Normals,
            ScenePass::Position => Pass::Position,
            ScenePass::Motion => Pass::Motion,
            ScenePass::Objects => Pass::Objects,
            ScenePass::ObjectsMore => Pass::ObjectsMore,
        };
        let expanded = self.expand(scene, models)?;
        let picture_of = |source| images.iter().find(|(id, ..)| *id == source);
        let mut pictures: Vec<(wgpu::Buffer, u32, u32)> = Vec::new();
        let mut number = |buffer: &wgpu::Buffer, w: u32, h: u32| -> u32 {
            match pictures.iter().position(|(b, ..)| b == buffer) {
                Some(i) => i as u32,
                None => {
                    pictures.push((buffer.clone(), w, h));
                    (pictures.len() - 1) as u32
                }
            }
        };
        let mut read: Vec<(NodeId, Vec<[[f32; 4]; 3]>)> = Vec::new();
        let mut placed = Vec::new();
        // Each placing's node and which of that node's placings it is (copies apart).
        let mut origins: Vec<(NodeId, Option<usize>)> = Vec::new();
        let mut seen: Vec<(NodeId, usize)> = Vec::new();
        for object in &expanded.objects {
            if matches!(object.geometry, Geometry::Splats { .. }) {
                continue;
            }
            let copies = match object.instanced {
                Some(id) => {
                    if !read.iter().any(|(node, _)| *node == id) {
                        let copies = match instances.iter().find(|i| i.node == id) {
                            Some(found) => read_copies(device, queue, found)?,
                            None => Vec::new(),
                        };
                        read.push((id, copies));
                    }
                    match read.iter().find(|(node, _)| *node == id) {
                        Some((_, copies)) if !copies.is_empty() => Some(copies),
                        _ => continue,
                    }
                }
                None => None,
            };
            // Cards are met where their picture is opaque, shapes cut where theirs is not.
            let mut flags = 0;
            let texture = match &object.geometry {
                Geometry::Card { image, .. } => picture_of(*image).map(|(_, b, [w, h], _)| {
                    flags |= CARD;
                    number(b, *w, *h)
                }),
                _ if object.surface.catcher || object.surface.look.is_some() => None,
                _ => object
                    .texture
                    .filter(|_| object.projection.is_none())
                    .and_then(picture_of)
                    .map(|(_, b, [w, h], _)| {
                        flags |= CUT;
                        number(b, *w, *h)
                    }),
            };
            let surface = Surface {
                opacity: object.surface.opacity,
                flags,
                texture,
                ..Surface::default()
            };
            let tree = self.tree(object)?;
            match copies {
                None => {
                    let k = match seen.iter_mut().find(|(node, _)| *node == object.node) {
                        Some((_, count)) => {
                            *count += 1;
                            *count - 1
                        }
                        None => {
                            seen.push((object.node, 1));
                            0
                        }
                    };
                    origins.push((object.node, Some(k)));
                    placed.push(Placed {
                        tree,
                        world: object.world,
                        surface,
                        tint: [1.0; 3],
                    });
                }
                Some(copies) => {
                    for [at, turn, _] in copies.iter().filter(|c| c[0][3] > 0.0) {
                        origins.push((object.node, None));
                        placed.push(Placed {
                            tree: tree.clone(),
                            world: copied(&object.world, *at, *turn),
                            surface,
                            tint: [1.0; 3],
                        });
                    }
                }
            }
        }
        let packed = veylvik_trace::pack(&placed);
        let placing_ids: Vec<u32> = origins
            .iter()
            .map(|(node, _)| ids.get(node).copied().unwrap_or(0))
            .collect();
        // Where each placing was and will be: the same node's same placing then, or
        // where it is now (copies at particles, and things that came or went).
        let elsewhere =
            |other: &veylvik_scene::Scene| -> Result<Vec<Mat4>> {
                let other = self.expand(other, models)?;
                let mut by_node: Vec<(NodeId, Vec<Mat4>)> = Vec::new();
                for o in other.objects.iter().filter(|o| {
                    o.instanced.is_none() && !matches!(o.geometry, Geometry::Splats { .. })
                }) {
                    match by_node.iter_mut().find(|(n, _)| *n == o.node) {
                        Some((_, worlds)) => worlds.push(o.world),
                        None => by_node.push((o.node, vec![o.world])),
                    }
                }
                Ok(origins
                    .iter()
                    .zip(&placed)
                    .map(|((node, k), p)| {
                        k.and_then(|k| {
                            let (_, worlds) = by_node.iter().find(|(n, _)| n == node)?;
                            let mine = seen.iter().find(|(n, _)| n == node).map_or(0, |s| s.1);
                            (worlds.len() == mine).then(|| worlds[k])
                        })
                        .unwrap_or(p.world)
                    })
                    .collect())
            };
        let (before, after) = match moved {
            Some(m) if pass == Pass::Motion => (elsewhere(&m.before.0)?, elsewhere(&m.after.0)?),
            _ => (Vec::new(), Vec::new()),
        };
        let moved = moved.filter(|_| pass == Pass::Motion).map(|m| Moved {
            before: &before,
            after: &after,
            camera_before: m.before.1.world,
            camera_after: m.after.1.world,
        });
        let textures: Vec<Texture<'_>> = pictures
            .iter()
            .map(|(buffer, width, height)| Texture {
                buffer,
                width: *width,
                height: *height,
            })
            .collect();
        let tracer = self.tracer.get_or_init(|| {
            Tracer::new(
                device,
                veylvik_render::SURFACE_WGSL,
                veylvik_render::ENVIRONMENT_WGSL,
            )
        });
        tracer
            .passes(PassJob {
                device,
                encoder,
                scene: &packed,
                textures: &textures,
                view: View {
                    camera: camera.world,
                    focal: veylvik_scene::home_distance(
                        width,
                        camera.focal_length,
                        camera.sensor_width,
                    ),
                    lens_radius: 0.0,
                    focus: 1.0,
                },
                width,
                height,
                pass,
                ids: &placing_ids,
                moved,
                target,
            })
            .map_err(error)
    }
}
