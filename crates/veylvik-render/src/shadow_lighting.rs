use crate::studio_lighting::Lighting;
/// The rig that lights a material at `time`: its connected Light rig (with following
/// lights carried by the rig's planar track), else its own.
fn rig(
    material: &veylvik_document::Node,
    plan: &crate::Plan,
    time: veylvik_document::Time,
) -> crate::Result<Option<veylvik_document::optical::RigState>> {
    use veylvik_document::Operation;
    let Operation::Material(m) = &material.operation else {
        return Ok(None);
    };
    let Some(rig) = material
        .inputs
        .get("lights")
        .and_then(|id| plan.nodes.iter().find(|n| n.id == *id))
    else {
        return Ok(Some(m.studio.at(time)?));
    };
    let Operation::LightRig(light) = &rig.operation else {
        return Ok(Some(m.studio.at(time)?));
    };
    let track = rig
        .inputs
        .get("track")
        .and_then(|id| plan.light_data.iter().find(|n| n.id == *id))
        .and_then(|n| match &n.operation {
            Operation::PlanarTrack(track) => Some(&*track.data),
            _ => None,
        });
    Ok(Some(light.studio.followed(
        track,
        time,
        plan.width,
        plan.height,
    )?))
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
    plan: &crate::Plan,
    time: veylvik_document::Time,
) -> crate::Result<Option<Lighting>> {
    let nodes = &plan.nodes;
    use veylvik_document::Operation;
    if matches!(node.operation, Operation::Condensation(_)) {
        let mut upstream = node.clone();
        upstream
            .inputs
            .insert("lighting".into(), node.inputs["image"]);
        let found = material(&upstream, nodes);
        let rig = match found {
            Some(n) => rig(n, plan, time)?.unwrap_or_default(),
            None => veylvik_document::optical::RigState::default(),
        };
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
            &rig,
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
        Operation::ContactShadow(_) => material(node, nodes),
        _ => None,
    };
    let Some((source, Operation::Material(m))) = source.map(|n| (n, &n.operation)) else {
        return Ok(None);
    };
    let studio = match rig(source, plan, time)? {
        Some(studio) => studio,
        None => m.studio.at(time)?,
    };
    let colours = m
        .colours
        .as_ref()
        .map(|c| veylvik_document::optical::palette_at(c, time))
        .transpose()?;
    let mut lighting = Lighting::new(m.kind, m.optics, colours, &studio);
    lighting.palette[0][3] = m.shot.at(time)?.clamp(0.0, 1.0);
    lighting.set_finish(m, time)?;
    if matches!(node.operation, Operation::Material(_)) {
        let bit = |port: &str, bit: u32| u32::from(node.inputs.contains_key(port)) * bit;
        let flags = bit("normal_map", 1)
            | bit("roughness_map", 2)
            | bit("height_map", 4)
            | (u32::from(m.normal_directx) * 8);
        lighting.maps = [
            m.normal_strength.at(time)?,
            m.height_depth.at(time)?,
            flags as f32,
            0.,
        ];
    }
    if matches!(
        node.operation,
        Operation::Shadow(_) | Operation::ContactShadow(_)
    ) {
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
