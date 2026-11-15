use crate::studio_lighting::Lighting;
/// The rig that lights a material: its connected Light rig, else its own.
fn rig<'a>(
    material: &'a veylvik_document::Node,
    nodes: &'a [veylvik_document::Node],
) -> Option<&'a veylvik_document::optical::StudioRig> {
    let veylvik_document::Operation::Material(m) = &material.operation else {
        return None;
    };
    Some(
        match material
            .inputs
            .get("lights")
            .and_then(|id| nodes.iter().find(|n| n.id == *id))
            .map(|n| &n.operation)
        {
            Some(veylvik_document::Operation::LightRig(rig)) => &rig.studio,
            _ => &m.studio,
        },
    )
}
/// Nearest upstream material node, with deterministic breadth-first traversal.
pub(crate) fn material<'a>(
    node: &veylvik_document::Node,
    nodes: &'a [veylvik_document::Node],
) -> Option<&'a veylvik_document::Node> {
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
        if let veylvik_document::Operation::Material(_) = &n.operation {
            return Some(n);
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
        let found = material(&upstream, nodes);
        let fallback = veylvik_document::optical::StudioRig::default();
        let rig = found.and_then(|n| rig(n, nodes)).unwrap_or(&fallback);
        let material = found.and_then(|n| match &n.operation {
            Operation::Material(m) => Some(m),
            _ => None,
        });
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
    let source = match &node.operation {
        Operation::Material(_) => Some(node),
        Operation::Shadow(s) if s.follow_lighting => material(node, nodes),
        _ => None,
    };
    let Some((source, Operation::Material(m))) = source.map(|n| (n, &n.operation)) else {
        return Ok(None);
    };
    let studio = rig(source, nodes).unwrap_or(&m.studio);
    let mut lighting = Lighting::new(m.kind, m.optics, m.colours, studio);
    lighting.palette[0][3] = m.shot.at(time)?.clamp(0.0, 1.0);
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
