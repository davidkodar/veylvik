use crate::studio_lighting::Lighting;
/// Nearest upstream material, with deterministic breadth-first traversal.
pub(crate) fn material<'a>(
    node: &veylvik_document::Node,
    nodes: &'a [veylvik_document::Node],
) -> Option<&'a veylvik_document::nodes::Material> {
    use std::collections::{BTreeSet, VecDeque};
    let mut pending = VecDeque::from([*node.inputs.get("lighting")?]);
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        let Some(n) = nodes.iter().find(|n| n.id == id) else {
            continue;
        };
        if let veylvik_document::Operation::Material(m) = &n.operation {
            return Some(m);
        }
        pending.extend(n.inputs.values().copied());
    }
    None
}
pub(crate) fn for_node(
    node: &veylvik_document::Node,
    nodes: &[veylvik_document::Node],
    time: veylvik_document::Time,
) -> crate::Result<Option<Lighting>> {
    use veylvik_document::Operation;
    if matches!(node.operation, Operation::Condensation(_)) {
        let mut upstream = node.clone();
        upstream
            .inputs
            .insert("lighting".into(), node.inputs["image"]);
        let material = material(&upstream, nodes);
        let fallback = veylvik_document::optical::StudioRig::default();
        let rig = material.map_or(&fallback, |m| &m.studio);
        let mut lighting = Lighting::new(
            veylvik_document::MaterialKind::Silver,
            Some(veylvik_document::optical::SurfaceOptics {
                metallic: false,
                reflectance: [0.02; 3],
                ior: 1.333,
            }),
            None,
            rig,
        );
        if let Some(m) = material {
            let angle = m.lighting.at(time)? * std::f32::consts::TAU;
            let (s, c) = angle.sin_cos();
            lighting.environment[3] += angle;
            for light in &mut lighting.lights {
                for v in &mut light[..3] {
                    let (x, z) = (v[0], v[2]);
                    v[0] = c * x + s * z;
                    v[2] = -s * x + c * z;
                }
            }
        }
        return Ok(Some(lighting));
    }
    let m = match &node.operation {
        Operation::Material(m) => Some(m),
        Operation::Shadow(s) if s.follow_lighting => material(node, nodes),
        _ => None,
    };
    let Some(m) = m else {
        return Ok(None);
    };
    let mut lighting = Lighting::material(m, m.shot.at(time)?);
    if matches!(node.operation, Operation::Shadow(_)) {
        // Match the material's rotation of the light frame (inverse Y rotation).
        let (s, c) = (m.lighting.at(time)? * std::f32::consts::TAU).sin_cos();
        for light in &mut lighting.lights {
            for v in &mut light[..3] {
                let (x, z) = (v[0], v[2]);
                v[0] = c * x + s * z;
                v[2] = -s * x + c * z;
            }
        }
    }
    Ok(Some(lighting))
}
