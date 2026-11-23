//! Force nodes reach particles as data: each force's settings at the moments the particle settings are kept, so a particle feels them as keyed whichever frame computes it.
use super::{Cx, RenderNode, particles::ROWS};
use crate::{Result, error};
use veylvik_document::{
    Operation, Time,
    nodes::{FORCES_MOST, Force, ForceKind},
};

/// Values a force has at each moment.
const VALUES: usize = 12;

/// A force is read as data by the particles it drives; it has no pass of its own.
impl RenderNode for Force {}

/// The chain of forces at the particles' Forces input, as data: how many; for each,
/// its kind and its values at the ROWS moments of the last `life` seconds (the last at
/// the frame): point, strength, radius, softness, its direction (a vortex's axis) and
/// scale.
pub(crate) fn forces(cx: &Cx<'_>, life: f32) -> Result<Vec<f32>> {
    let by_id: std::collections::BTreeMap<_, _> = cx.nodes.iter().map(|n| (n.id, n)).collect();
    let mut chain: Vec<&Force> = Vec::new();
    let mut next = cx.node.inputs.get("forces").copied();
    while let Some(id) = next {
        let Some(node) = by_id.get(&id) else { break };
        let Operation::Force(force) = &node.operation else {
            break;
        };
        if chain.len() == FORCES_MOST {
            break;
        }
        chain.push(force);
        next = node.inputs.get("forces").copied();
    }
    if chain.is_empty() {
        return Ok(Vec::new());
    }
    let seconds = cx.time.seconds().map_err(error)?;
    let moments: Vec<Time> = (0..ROWS)
        .map(|r| {
            if r == ROWS - 1 {
                return Ok(cx.time);
            }
            let at = seconds - f64::from(life) + f64::from(life) * r as f64 / (ROWS - 1) as f64;
            Time::new((at * 24000.0).round() as i64, 24000).map_err(error)
        })
        .collect::<Result<_>>()?;
    let mut values = vec![chain.len() as f32];
    for force in chain {
        values.push(
            ForceKind::ALL
                .iter()
                .position(|k| serde_json::to_value(force.kind).is_ok_and(|v| v == *k))
                .unwrap_or(0) as f32,
        );
        for &time in &moments {
            let at = |s: &veylvik_document::Scalar| s.at(time).map_err(error);
            let (turn, lean) = (
                at(&force.direction)?.to_radians(),
                at(&force.lean)?.to_radians(),
            );
            let way = if force.kind == ForceKind::Vortex {
                // From the viewer's direction, leaned towards `direction`.
                [
                    lean.sin() * turn.cos(),
                    -lean.sin() * turn.sin(),
                    lean.cos(),
                ]
            } else {
                [
                    turn.cos() * lean.cos(),
                    -turn.sin() * lean.cos(),
                    lean.sin(),
                ]
            };
            let row: [f32; VALUES] = [
                at(&force.x)?,
                at(&force.y)?,
                at(&force.z)?,
                at(&force.strength)?,
                at(&force.radius)?,
                at(&force.softness)?,
                way[0],
                way[1],
                way[2],
                at(&force.scale)?,
                0.0,
                0.0,
            ];
            values.extend_from_slice(&row);
        }
    }
    Ok(values)
}
