//! Skinned parts: each vertex follows its joints, blended by its weights.
use std::sync::Arc;
use veylvik_mesh::animation::{Between, Channel, Clip, IDENTITY, Joint, Keyed};
use veylvik_mesh::{Mesh, Model, Part, Skin, Vertex};

/// A bar along x from 0 to 2 with a joint at 0 (the root) and one at 1 (its child): the
/// far end hangs from the child, the middle from both, the start from the root.
fn bar(turn: f32) -> Model {
    let positions = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    let vertices = positions
        .iter()
        .map(|p| Vertex {
            position: *p,
            normal: [0.0, 1.0, 0.0],
            uv: [0.0; 2],
        })
        .collect();
    let mut child_bind = IDENTITY;
    child_bind[0][3] = -1.0;
    let skin = Skin {
        joints: vec![0, 1],
        inverse_bind: vec![IDENTITY, child_bind],
        positions: positions.to_vec(),
        normals: vec![[0.0, 1.0, 0.0]; 3],
        influences: vec![[0, 0, 0, 0], [0, 1, 0, 0], [1, 0, 0, 0]],
        weights: vec![
            [1.0, 0.0, 0.0, 0.0],
            [0.5, 0.5, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ],
    };
    let (s, c) = (turn * 0.5).sin_cos();
    Model {
        parts: vec![Part {
            mesh: Mesh {
                vertices,
                indices: vec![0, 1, 2],
                cap_indices: 0,
            },
            colour: [1.0; 4],
            metallic: 0.0,
            roughness: 0.5,
            texture: None,
            joint: Some(2),
            rest_inverse: IDENTITY,
            skin: Some(Arc::new(skin)),
            cache: None,
        }],
        extent: [1.0; 3],
        joints: vec![
            Joint {
                parent: None,
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            Joint {
                parent: Some(0),
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            Joint {
                parent: None,
                translation: [5.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
        ],
        // The child joint turns about z by `turn` over a second.
        clips: vec![Clip {
            length: 0.0,
            name: "bend".into(),
            channels: vec![Channel {
                node: 1,
                keyed: Keyed::Rotation,
                between: Between::Linear,
                times: vec![0.0, 1.0],
                values: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, s, c]],
            }],
        }],
        centre: [0.0; 3],
        largest: 1.0,
    }
}
fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
}
#[test]
fn a_skinned_bar_bends_at_its_joint_and_its_weighted_middle_goes_halfway() {
    let model = bar(std::f32::consts::FRAC_PI_2);
    // At rest: where the file put it (y flipped into the normalised frame).
    let rest = model.deformed(0, None).unwrap();
    assert!(close(rest.vertices[2].position, [2.0, 0.0, 0.0]));
    // At the end of the clip the child joint has turned a right angle about z: the far
    // end (on the child) is at (1, 1) in glTF's frame, (1, -1) once y points down.
    let bent = model.deformed(0, Some((0, 1.0, false))).unwrap();
    assert!(close(bent.vertices[0].position, [0.0, 0.0, 0.0]));
    assert!(
        close(bent.vertices[2].position, [1.0, -1.0, 0.0]),
        "{:?}",
        bent.vertices[2]
    );
    // The middle is half on each joint: both place it at (1, 0), so it stays.
    assert!(close(bent.vertices[1].position, [1.0, 0.0, 0.0]));
    // The far end's normal turned with its joint: from up to the left (-x).
    assert!(
        close(bent.vertices[2].normal, [-1.0, 0.0, 0.0]),
        "{:?}",
        bent.vertices[2].normal
    );
    // Halfway through the clip the far end is a quarter turn's half along.
    let half = model.deformed(0, Some((0, 0.5, false))).unwrap();
    let r = std::f32::consts::FRAC_PI_4;
    assert!(close(
        half.vertices[2].position,
        [1.0 + r.cos(), -r.sin(), 0.0]
    ));
    // A skinned part takes no rigid pose of its own.
    assert_eq!(model.poses(0, 1.0, false).unwrap()[0], IDENTITY);
    // A part without a skin has no skinned mesh.
    let mut rigid = model.clone();
    rigid.parts[0].skin = None;
    assert!(rigid.deformed(0, None).is_none());
}
