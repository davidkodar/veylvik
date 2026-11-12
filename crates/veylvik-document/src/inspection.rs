//! Read-only graph queries that avoid copying full parameter payloads for discovery.
use crate::{Composition, NodeId, Result, fail};
use serde_json::{Value, json};

pub fn summary(document: &Composition) -> Value {
    let mut operations = std::collections::BTreeMap::<&str, usize>::new();
    for node in document.nodes.values() {
        *operations
            .entry(node.operation.descriptor().type_id)
            .or_default() += 1;
    }
    json!({"schema":document.schema,"width":document.width,"height":document.height,
        "frame_rate":document.frame_rate,"frame_count":document.frame_count,
        "group_count":document.groups.len(),"output":document.output,"node_count":document.nodes.len(),"operations":operations})
}

/// Stable UUID ordering; callers should compare document revisions between pages.
/// Labels are limited to 256 Unicode characters and explicitly marked when shortened.
pub fn page(document: &Composition, offset: u64, limit: u64) -> Result<Value> {
    if !(1..=128).contains(&limit) {
        return fail("limit must be in 1..=128");
    }
    if offset > document.nodes.len() as u64 {
        return fail("offset exceeds node count");
    }
    let nodes: Vec<_> = document.nodes.values().skip(offset as usize).take(limit as usize).map(|node| {
        let mut chars = node.label.chars();
        let label: String = chars.by_ref().take(256).collect();
        json!({"id":node.id,"version":node.version,"label":label,"label_truncated":chars.next().is_some(),
            "type":node.operation.descriptor().type_id,"inputs":node.inputs,"position":node.position})
    }).collect();
    let next = offset + nodes.len() as u64;
    Ok(
        json!({"nodes":nodes,"offset":offset,"next_offset":(next < document.nodes.len() as u64).then_some(next),"total":document.nodes.len()}),
    )
}

/// Read an exact authored node or one registered parameter, without evaluating curves.
pub fn node(document: &Composition, id: NodeId, parameter: Option<&str>) -> Result<Value> {
    let Some(node) = document.nodes.get(&id) else {
        return fail("node does not exist");
    };
    if let Some(name) = parameter {
        let kind = node.operation.descriptor();
        if !kind
            .parameters
            .iter()
            .any(|parameter| parameter.name == name)
        {
            return fail("unknown node parameter");
        }
        let operation = serde_json::to_value(&node.operation)
            .map_err(|error| crate::Error(error.to_string()))?;
        Ok(json!({"node_id":id,"operation":kind.type_id,"parameter":name,"value":operation[name]}))
    } else {
        Ok(json!({"node":node}))
    }
}
