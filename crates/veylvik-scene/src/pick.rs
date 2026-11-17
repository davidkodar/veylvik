//! Rays against a scene's solid surfaces: where a view ray first meets one, and where a light must sit for its reflection to show there.
use crate::{Geometry, Scene, math, mesh};
use veylvik_document::{NodeId, Result};

/// Where a ray first meets a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub node: NodeId,
    pub point: [f32; 3],
    /// The surface's unit normal at the point, on the side the ray came from.
    pub normal: [f32; 3],
    pub distance: f32,
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt().max(1e-12);
    v.map(|c| c / l)
}
/// The nearest solid surface along the ray from `origin` in `direction` (cards and
/// shadow catchers are passed through). Objects whose mesh cannot be built are skipped.
pub fn hit(scene: &Scene, origin: [f32; 3], direction: [f32; 3]) -> Result<Option<Hit>> {
    let direction = unit(direction);
    let mut nearest: Option<Hit> = None;
    for object in &scene.objects {
        if object.surface.catcher || matches!(object.geometry, Geometry::Card { .. }) {
            continue;
        }
        // A sphere round the object rules most of them out cheaply.
        let (centre, radius) = object.sphere();
        let to = sub(centre, origin);
        let along = dot(to, direction);
        if dot(to, to) - along * along > radius * radius * 1.01 + 1.0 {
            continue;
        }
        let Ok(mesh) = mesh(object) else {
            continue;
        };
        let world: Vec<[f32; 3]> = mesh
            .vertices
            .iter()
            .map(|v| math::point(&object.world, v.position))
            .collect();
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|k| world[triangle[k] as usize]);
            // Möller-Trumbore.
            let (ab, ac) = (sub(b, a), sub(c, a));
            let p = cross(direction, ac);
            let det = dot(ab, p);
            if det.abs() < 1e-9 {
                continue;
            }
            let t = sub(origin, a);
            let u = dot(t, p) / det;
            let q = cross(t, ab);
            let v = dot(direction, q) / det;
            let distance = dot(ac, q) / det;
            if u < 0.0 || v < 0.0 || u + v > 1.0 || distance <= 1e-3 {
                continue;
            }
            if nearest.is_none_or(|n| distance < n.distance) {
                let mut normal = unit(cross(ab, ac));
                if dot(normal, direction) > 0.0 {
                    normal = normal.map(|c| -c);
                }
                nearest = Some(Hit {
                    node: object.node,
                    point: [0, 1, 2].map(|k| origin[k] + direction[k] * distance),
                    normal,
                    distance,
                });
            }
        }
    }
    Ok(nearest)
}
/// Where a light `distance` away must sit for an eye at `eye` to see its reflection at
/// `hit`, and the unit direction its light travels (towards the surface).
pub fn mirrored(eye: [f32; 3], hit: &Hit, distance: f32) -> ([f32; 3], [f32; 3]) {
    let view = unit(sub(hit.point, eye));
    let d = dot(view, hit.normal);
    let away = unit([0, 1, 2].map(|k| view[k] - 2.0 * d * hit.normal[k]));
    (
        [0, 1, 2].map(|k| hit.point[k] + away[k] * distance),
        away.map(|c| -c),
    )
}
/// The ray from a camera through a frame pixel: its origin and unit direction.
pub fn ray(
    camera: &crate::Camera,
    pixel: [f32; 2],
    width: u32,
    height: u32,
) -> ([f32; 3], [f32; 3]) {
    let focal = crate::home_distance(width, camera.focal_length, camera.sensor_width);
    let towards = [
        (pixel[0] - width as f32 * 0.5) / focal,
        (pixel[1] - height as f32 * 0.5) / focal,
        -1.0,
    ];
    (
        math::point(&camera.world, [0.0; 3]),
        unit(math::direction(&camera.world, towards)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, Operation, Time, blank, registry};
    #[test]
    fn a_ray_finds_the_surface_and_a_light_placed_from_it_mirrors_into_the_eye() {
        let mut doc = blank(640, 360, Time::new(24, 1).unwrap()).unwrap();
        let mut cube = registry::SHAPE_3D.default_operation();
        cube["size_x"] = serde_json::json!({"kind": "constant", "value": 100.0});
        cube["size_y"] = serde_json::json!({"kind": "constant", "value": 100.0});
        cube["size_z"] = serde_json::json!({"kind": "constant", "value": 100.0});
        let cube: Operation = serde_json::from_value(cube).unwrap();
        let node = Node::new("cube", cube, &[]);
        let id = node.id;
        doc.nodes.insert(id, node);
        let time = Time::new(0, 1).unwrap();
        let scene = crate::evaluate(&doc, id, time).unwrap();
        let camera = crate::home_camera(640, 360);
        // Through the frame's centre: the cube's front face, 50 px in front of the frame.
        let (origin, direction) = ray(&camera, [320.0, 180.0], 640, 360);
        let found = hit(&scene, origin, direction).unwrap().unwrap();
        assert_eq!(found.node, id);
        assert!((found.point[0] - 320.0).abs() < 1e-2 && (found.point[1] - 180.0).abs() < 1e-2);
        assert!((found.point[2] - 50.0).abs() < 1e-2, "{:?}", found.point);
        assert!((found.normal[2] - 1.0).abs() < 1e-4, "{:?}", found.normal);
        // Beside the cube the ray meets nothing.
        let (origin, direction) = ray(&camera, [20.0, 20.0], 640, 360);
        assert!(hit(&scene, origin, direction).unwrap().is_none());
        // Off-centre on the front face: the light sits mirrored about the normal, so the
        // eye and the light make equal angles with it, on opposite sides.
        let (origin, direction) = ray(&camera, [350.0, 190.0], 640, 360);
        let found = hit(&scene, origin, direction).unwrap().unwrap();
        let (light, travel) = mirrored(origin, &found, 300.0);
        let to_eye = unit(sub(origin, found.point));
        let to_light = unit(sub(light, found.point));
        assert!((dot(to_eye, found.normal) - dot(to_light, found.normal)).abs() < 1e-4);
        let half = unit([0, 1, 2].map(|k| to_eye[k] + to_light[k]));
        assert!((dot(half, found.normal) - 1.0).abs() < 1e-4);
        assert!(
            (dot(sub(light, found.point), sub(light, found.point)).sqrt() - 300.0).abs() < 1e-2
        );
        assert!((dot(travel, to_light) + 1.0).abs() < 1e-4);
        // A light shining along that travel points back at the surface.
        let rotation = crate::shining(travel);
        let facing = math::direction(&math::rotate(rotation), [0.0, 0.0, -1.0]);
        assert!(
            dot(unit(facing), travel) > 0.999,
            "{facing:?} vs {travel:?}"
        );
    }
}
