//! Native-resolution mapping of authored spatial quantities, never a bitmap resize.
use crate::{Node, NodeId, Operation, Result, Scalar, Segment, groups::Group, vector::PathCommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sizing {
    /// Preserve proportions, center the authored canvas inside the target.
    #[default]
    Fit,
    /// Preserve proportions, center and crop the authored canvas to cover the target.
    Fill,
    /// Keep authored pixels, for callers intentionally working in absolute units.
    Original,
}
impl Sizing {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fit => "Fit composition",
            Self::Fill => "Fill composition",
            Self::Original => "Authored pixels",
        }
    }
}
#[derive(Clone, Copy)]
struct Mapping {
    scale: f32,
    offset: [f32; 2],
}
fn map_scalar(s: &mut Scalar, scale: f32, offset: f32) {
    if let Scalar::Constant { value } = s {
        *value = *value * scale + offset;
        return;
    }
    for key in s.keys_mut().unwrap() {
        key.value = key.value * scale + offset;
        if let Some(Segment::Bezier { outgoing, incoming }) = &mut key.segment {
            outgoing.value *= scale;
            incoming.value *= scale;
        }
    }
}
fn offset(op: &Operation, parameter: &str, m: Mapping) -> f32 {
    let axis = match parameter {
        "pivot_x" | "start_x" | "end_x" => Some(0),
        "pivot_y" | "start_y" | "end_y" => Some(1),
        "x" if matches!(
            op,
            Operation::Text(crate::nodes::Text { .. })
                | Operation::Ellipse(crate::nodes::Ellipse { .. })
                | Operation::Rectangle(crate::nodes::Rectangle { .. })
                | Operation::Fire(crate::nodes::Fire { .. })
                | Operation::MetallicShine(crate::nodes::MetallicShine { .. })
                | Operation::Atmosphere(crate::nodes::Atmosphere { .. })
        ) =>
        {
            Some(0)
        }
        "y" if matches!(
            op,
            Operation::Text(crate::nodes::Text { .. })
                | Operation::Ellipse(crate::nodes::Ellipse { .. })
                | Operation::Rectangle(crate::nodes::Rectangle { .. })
                | Operation::Fire(crate::nodes::Fire { .. })
                | Operation::MetallicShine(crate::nodes::MetallicShine { .. })
                | Operation::Atmosphere(crate::nodes::Atmosphere { .. })
        ) =>
        {
            Some(1)
        }
        _ => None,
    };
    axis.map_or(0.0, |i| m.offset[i])
}
pub fn map_graph(
    nodes: &mut BTreeMap<NodeId, Node>,
    groups: &mut BTreeMap<NodeId, Group>,
    from: [u32; 2],
    to: [u32; 2],
    sizing: Sizing,
) -> Result<()> {
    if from.contains(&0) || to.contains(&0) || from.iter().chain(&to).any(|v| *v > 8192) {
        return Err(crate::Error("invalid resolution mapping".into()));
    }
    if sizing == Sizing::Original || from == to {
        return Ok(());
    }
    let ratios = [to[0] as f32 / from[0] as f32, to[1] as f32 / from[1] as f32];
    let scale = if sizing == Sizing::Fill {
        ratios[0].max(ratios[1])
    } else {
        ratios[0].min(ratios[1])
    };
    let m = Mapping {
        scale,
        offset: [
            (to[0] as f32 - from[0] as f32 * scale) * 0.5,
            (to[1] as f32 - from[1] as f32 * scale) * 0.5,
        ],
    };
    for group in groups.values_mut() {
        for control in group.controls.values_mut() {
            let op = &nodes[&control.node].operation;
            if op.descriptor().parameters.iter().any(|p| {
                p.name == control.parameter && p.unit == "pixels" && op.scalar(p.name).is_some()
            }) {
                let shift = offset(op, &control.parameter, m);
                let mut scalar: Scalar = serde_json::from_value(control.default.clone())
                    .map_err(|e| crate::Error(e.to_string()))?;
                map_scalar(&mut scalar, scale, shift);
                control.default =
                    serde_json::to_value(scalar).map_err(|e| crate::Error(e.to_string()))?;
                if let Some(range) = &mut control.range {
                    for v in range {
                        *v = *v * scale + shift;
                    }
                }
            }
        }
    }
    for node in nodes.values_mut() {
        for parameter in node.operation.descriptor().parameters {
            if parameter.unit == "pixels" {
                let shift = offset(&node.operation, parameter.name, m);
                if let Some(scalar) = node.operation.scalar_mut(parameter.name) {
                    map_scalar(scalar, scale, shift);
                }
            }
        }
        if let Operation::Path(crate::nodes::Path { geometry }) = &mut node.operation {
            let point = |p: &mut [f32; 2]| {
                p[0] = p[0] * m.scale + m.offset[0];
                p[1] = p[1] * m.scale + m.offset[1];
            };
            for command in geometry.commands.iter_mut().chain(
                geometry
                    .keys
                    .iter_mut()
                    .flat_map(|key| key.commands.iter_mut()),
            ) {
                match command {
                    PathCommand::Move { to } | PathCommand::Line { to } => point(to),
                    PathCommand::Quad { control, to } => {
                        point(control);
                        point(to);
                    }
                    PathCommand::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        point(control1);
                        point(control2);
                        point(to);
                    }
                    PathCommand::Close {} => {}
                }
            }
        }
        node.operation.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Handle, Key, Time};
    #[test]
    fn fit_and_fill_center_geometry_but_only_scale_relative_offsets() {
        for (sizing, scale, shift) in [
            (Sizing::Fit, 2.0, [0.0, 100.0]),
            (Sizing::Fill, 4.0, [-200.0, 0.0]),
        ] {
            let ellipse = Node::new(
                "circle",
                Operation::Ellipse(crate::nodes::Ellipse {
                    x: 100.0.into(),
                    y: 50.0.into(),
                    radius_x: 20.0.into(),
                    radius_y: 20.0.into(),
                    feather: 1.0.into(),
                }),
                &[],
            );
            let id = ellipse.id;
            let mut nodes = [(id, ellipse)].into();
            map_graph(
                &mut nodes,
                &mut BTreeMap::new(),
                [200, 100],
                [400, 400],
                sizing,
            )
            .unwrap();
            let op = &nodes[&id].operation;
            let time = Time::new(0, 1).unwrap();
            assert_eq!(
                op.scalar("x").unwrap().at(time).unwrap(),
                100.0 * scale + shift[0]
            );
            assert_eq!(
                op.scalar("y").unwrap().at(time).unwrap(),
                50.0 * scale + shift[1]
            );
            assert_eq!(
                op.scalar("radius_x").unwrap().at(time).unwrap(),
                20.0 * scale
            );
            assert_eq!(
                offset(
                    &Operation::Transform(crate::nodes::Transform {
                        x: 1.0.into(),
                        y: 1.0.into()
                    }),
                    "x",
                    Mapping {
                        scale,
                        offset: shift
                    }
                ),
                0.0
            );
        }
    }
    #[test]
    fn mapping_preserves_bezier_times_and_scales_relative_handle_values() {
        let mut scalar = Scalar::Linear {
            keys: vec![Key {
                time: Time::new(1, 3).unwrap(),
                value: 5.0,
                segment: Some(Segment::Bezier {
                    outgoing: Handle {
                        time: 0.25,
                        value: 2.0,
                    },
                    incoming: Handle {
                        time: 0.75,
                        value: -3.0,
                    },
                }),
            }],
        };
        map_scalar(&mut scalar, 4.0, 10.0);
        let key = &scalar.keys_mut().unwrap()[0];
        assert_eq!((key.time.ticks, key.time.rate, key.value), (1, 3, 30.0));
        if let Some(Segment::Bezier { outgoing, incoming }) = key.segment {
            assert_eq!(
                (outgoing.time, outgoing.value, incoming.time, incoming.value),
                (0.25, 8.0, 0.75, -12.0)
            );
        } else {
            panic!("lost bezier handles");
        }
    }
}
