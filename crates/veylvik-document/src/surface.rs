//! Surface appearance starting points and safe upgrades of existing tracked layers.
use crate::{Command, Composition, Document, Node, NodeId, Operation, Result, fail};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Painted,
    Weathered,
    SoftPrint,
    Sticker,
}
impl Look {
    pub const ALL: [Self; 4] = [
        Self::Painted,
        Self::Weathered,
        Self::SoftPrint,
        Self::Sticker,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Painted => "Painted",
            Self::Weathered => "Weathered paint",
            Self::SoftPrint => "Soft print",
            Self::Sticker => "Clean decal",
        }
    }
    pub fn operation(self) -> Operation {
        let (lighting, detail, wear, threshold, softness) = match self {
            Self::Painted => (0.35, 0.5, 0., 0.3, 0.),
            Self::Weathered => (0.45, 0.65, 0.8, 0.25, 0.3),
            Self::SoftPrint => (0.65, 0.3, 0., 0.3, 0.7),
            Self::Sticker => (0.2, 0.1, 0., 0.3, 0.),
        };
        Operation::SurfaceBlend(crate::nodes::SurfaceBlend {
            opacity: 1.0.into(),
            lighting: lighting.into(),
            detail: detail.into(),
            wear: wear.into(),
            threshold: threshold.into(),
            distortion: 0.0.into(),
            softness: softness.into(),
            grain: 0.0.into(),
        })
    }
}
/// Upgrade the composition's ordinary tracked-layer merge, preserving identity
/// and wiring; or composite a standalone output warp onto its tracked source.
/// Refuse ambiguous downstream graphs instead of silently discarding effects.
pub fn integrate(doc: &Composition, warp: NodeId, look: Look) -> Result<(NodeId, Vec<Command>)> {
    doc.validate()?;
    let Some(node) = doc.nodes.get(&warp) else {
        return fail("Tracked layer missing");
    };
    if !matches!(
        node.operation,
        Operation::PlanarWarp(crate::nodes::PlanarWarp {
            mode: crate::planar::WarpMode::Follow | crate::planar::WarpMode::Fit
        })
    ) {
        return fail("Select a Follow or Fit planar warp");
    }
    let source = doc.nodes[&node.inputs["track"]].inputs["image"];
    let current = &doc.nodes[&doc.output];
    let node = if matches!(current.operation, Operation::Merge(crate::nodes::Merge {}))
        && current.inputs.get("foreground") == Some(&warp)
        && current.inputs.get("background") == Some(&source)
    {
        let mut n = current.clone();
        n.operation = look.operation();
        n.label = "Layer on surface".into();
        n
    } else if doc.output == warp {
        Node::new(
            "Layer on surface",
            look.operation(),
            &[("background", source), ("foreground", warp)],
        )
    } else {
        return fail(
            "This layer has downstream effects. Add a Surface blend node and connect its background and foreground explicitly to preserve your graph.",
        );
    };
    let id = node.id;
    let mut commands = vec![Command::Put {
        node: Box::new(node),
    }];
    if doc.output != id {
        commands.push(Command::Output { id });
    }
    Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok((id, commands))
}
