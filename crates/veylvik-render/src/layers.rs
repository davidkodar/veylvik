//! Drawn layers (Path and Text) and media sources, as the renderer gives them to the GPU: a drawn layer is rasterised apart and kept over its own box, so its pass is told that box, its area is that box, and what it is (its identity) is its geometry or its text; a source's pass is told the source's region and placement. One place for what these node kinds need beside the per-node `RenderNode` code.
use crate::{GpuFrame, ImageInputs, Params, Plan, Result, error};
use std::collections::BTreeMap;
use std::sync::Arc;
use veylvik_document::Operation;

/// Tell drawn layers' and sources' passes their boxes, regions and placement.
pub(crate) fn place(
    evaluated: &mut [Params],
    plan: &Plan,
    vectors: &BTreeMap<usize, (Arc<GpuFrame>, [u32; 2])>,
    inputs: &ImageInputs,
) {
    // A drawn shape is kept over its own box: its pass is told the box.
    for (index, (frame, corner)) in vectors {
        if matches!(plan.nodes[*index].operation, Operation::Path(_)) {
            let p = &mut evaluated[*index];
            p.values = [
                frame.width() as f32,
                frame.height() as f32,
                corner[0] as f32,
                corner[1] as f32,
            ];
            p.extra[0] = frame.width() as f32;
        }
        // Text is drawn over its own box too: its pass reads the box (extra =
        // width, height, left, top) and is clear outside it.
        if matches!(plan.nodes[*index].operation, Operation::Text(_)) {
            evaluated[*index].extra = [
                frame.width() as f32,
                frame.height() as f32,
                corner[0] as f32,
                corner[1] as f32,
            ];
        }
    }
    for (node, p) in plan.nodes.iter().zip(evaluated.iter_mut()) {
        if let Some(asset) = node.operation.asset() {
            let key = if matches!(
                node.operation,
                Operation::Footage(veylvik_document::nodes::Footage { .. })
            ) {
                crate::plan::footage_key(node.id)
            } else {
                asset.to_owned()
            };
            let source = inputs.source(&key).unwrap();
            let region = source.region();
            if let Operation::Footage(veylvik_document::nodes::Footage { placement, .. }) =
                node.operation
            {
                use veylvik_document::media::Placement;
                p.size_op[3] = match placement {
                    Placement::Legacy => 0,
                    Placement::Fit => 1,
                    Placement::Fill => 2,
                    Placement::Original => 3,
                    Placement::Stretch => 4,
                };
            }
            p.values[0] = region.width as f32;
            p.values[1] = region.height as f32;
            p.extra = [
                source.storage_width() as f32,
                0.0,
                region.x as f32,
                region.y as f32,
            ];
        }
    }
}
/// A drawn layer's area in the frame: the box it was drawn over. None for other nodes.
pub(crate) fn extent(
    plan: &Plan,
    index: usize,
    vectors: &BTreeMap<usize, (Arc<GpuFrame>, [u32; 2])>,
) -> Option<crate::nodes::Extent> {
    if !matches!(
        plan.nodes[index].operation,
        Operation::Path(_) | Operation::Text(_)
    ) {
        return None;
    }
    let (frame, [x, y]) = vectors.get(&index)?;
    Some(crate::nodes::Extent::Area([
        *x as i32,
        *y as i32,
        (*x + frame.width()) as i32,
        (*y + frame.height()) as i32,
    ]))
}
/// What a drawn layer is at `time`, as words for the frame cache's key: a text by its
/// whole settings (and the time when it moves), a path by its geometry.
pub(crate) fn identity_words(
    op: &Operation,
    time: veylvik_document::Time,
    words: &mut Vec<u32>,
) -> Result<()> {
    if let veylvik_document::Operation::Text(veylvik_document::nodes::Text { .. }) = op {
        if let veylvik_document::Operation::Text(veylvik_document::nodes::Text {
            typography, ..
        }) = op
            && typography.motion != veylvik_document::text::TextMotion::None
        {
            let seconds = time.seconds()?.to_bits();
            words.extend([seconds as u32, (seconds >> 32) as u32]);
        }
        let bytes = serde_json::to_vec(op).map_err(error)?;
        words.extend([4, bytes.len() as u32]);
        for chunk in bytes.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            words.push(u32::from_le_bytes(word));
        }
        for parameter in op.descriptor().parameters {
            if let Some(value) = op.scalar(parameter.name) {
                words.push(value.at(time)?.to_bits());
            }
        }
        return Ok(());
    }
    use veylvik_document::vector::PathCommand::*;
    let veylvik_document::Operation::Path(veylvik_document::nodes::Path { geometry }) = op else {
        unreachable!()
    };
    let geometry = geometry.at(time)?;
    words.extend([3, geometry.fill_rule as u32, geometry.commands.len() as u32]);
    for command in &geometry.commands {
        match command {
            Move { to } => {
                words.push(0);
                words.extend(to.map(f32::to_bits));
            }
            Line { to } => {
                words.push(1);
                words.extend(to.map(f32::to_bits));
            }
            Quad { control, to } => {
                words.push(2);
                words.extend(control.map(f32::to_bits));
                words.extend(to.map(f32::to_bits));
            }
            Cubic {
                control1,
                control2,
                to,
            } => {
                words.push(3);
                words.extend(control1.map(f32::to_bits));
                words.extend(control2.map(f32::to_bits));
                words.extend(to.map(f32::to_bits));
            }
            Close {} => words.push(4),
        }
    }
    Ok(())
}
