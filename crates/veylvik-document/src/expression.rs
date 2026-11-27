//! Expressions on scalar parameters: links are stored by node id and, after every change, each expression gets the linked parameters' own curves attached, so sampling needs no document. Loops and chains deeper than MAX_LINK_DEPTH are refused.
use crate::{Composition, Error, NodeId, Result, Scalar, Time, registry::ParameterKind};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Longest chain of expressions reading expressions.
pub const MAX_LINK_DEPTH: usize = 16;

/// An expression made ready to sample: its program, the curve behind each link (by
/// the program's link index), the parameter's range and the frame rate.
#[derive(Debug)]
pub struct Resolved {
    program: veylvik_expr::Program,
    links: Vec<Scalar>,
    range: (f32, f32),
    rate: f64,
}
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}
fn parse(source: &str) -> Result<veylvik_expr::Program> {
    veylvik_expr::parse(source).map_err(|e| Error(format!("expression: {e}")))
}
pub(crate) fn validate(source: &str) -> Result<()> {
    parse(source).map(|_| ())
}
/// The value of an expression at `time`, clamped to its parameter's range.
pub(crate) fn evaluate(source: &str, resolved: Option<&Resolved>, time: Time) -> Result<f32> {
    let seconds = time.seconds()?;
    let Some(r) = resolved else {
        // Outside a document only time-only expressions can be sampled.
        let program = parse(source)?;
        if !program.links().is_empty() {
            return fail("a linked expression needs its project to be sampled");
        }
        let value = program
            .eval(seconds, f64::NAN, &mut |_| unreachable_link())
            .map_err(|e| Error(format!("expression: {e}")))?;
        return Ok(value.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32);
    };
    let value = r
        .program
        .eval(seconds, seconds * r.rate, &mut |i| {
            r.links[i]
                .at(time)
                .map(f64::from)
                .map_err(|e| veylvik_expr::Error(e.0))
        })
        .map_err(|e| Error(format!("expression: {e}")))?;
    Ok(value.clamp(f64::from(r.range.0), f64::from(r.range.1)) as f32)
}
fn unreachable_link() -> veylvik_expr::Result<f64> {
    Err(veylvik_expr::Error("no link".into()))
}

type Key = (NodeId, &'static str);
/// Every expression in the document with the parameters it reads.
fn graph(composition: &Composition) -> Result<BTreeMap<Key, (veylvik_expr::Program, Vec<Key>)>> {
    let mut graph = BTreeMap::new();
    for (id, node) in &composition.nodes {
        for parameter in node.operation.descriptor().parameters {
            let Some(Scalar::Expression { expression, .. }) = node.operation.scalar(parameter.name)
            else {
                continue;
            };
            let program = parse(expression)
                .map_err(|e| Error(format!("{} {}: {}", node.label, parameter.name, e.0)))?;
            let mut reads = Vec::new();
            for link in program.links() {
                let target = link
                    .node
                    .parse::<NodeId>()
                    .ok()
                    .and_then(|t| composition.nodes.get(&t))
                    .ok_or_else(|| {
                        Error(format!(
                            "{} {} links to a node that is not in the project ([{}]); change or remove its expression first",
                            node.label, parameter.name, link.node
                        ))
                    })?;
                let Some(name) = scalar_parameter(target, &link.parameter) else {
                    return fail(format!(
                        "{} {}: {} has no animatable parameter '{}' (it has {})",
                        node.label,
                        parameter.name,
                        target.label,
                        link.parameter,
                        scalar_names(target).join(", ")
                    ));
                };
                reads.push((target.id, name));
            }
            graph.insert((*id, parameter.name), (program, reads));
        }
    }
    Ok(graph)
}
fn scalar_parameter(node: &crate::Node, name: &str) -> Option<&'static str> {
    node.operation
        .descriptor()
        .parameters
        .iter()
        .find(|p| p.name == name && matches!(p.kind, ParameterKind::Scalar))
        .map(|p| p.name)
}
/// The animatable parameters of `node`, by name.
pub fn scalar_names(node: &crate::Node) -> Vec<&'static str> {
    node.operation
        .descriptor()
        .parameters
        .iter()
        .filter(|p| matches!(p.kind, ParameterKind::Scalar))
        .map(|p| p.name)
        .collect()
}
/// The expressions in an order where each comes after those it reads, refusing loops
/// and chains deeper than MAX_LINK_DEPTH (no recursion: an explicit stack).
fn order(
    composition: &Composition,
    graph: &BTreeMap<Key, (veylvik_expr::Program, Vec<Key>)>,
) -> Result<Vec<Key>> {
    let mut depth: BTreeMap<Key, usize> = BTreeMap::new();
    let mut out = Vec::with_capacity(graph.len());
    let name = |k: &Key| format!("{} {}", composition.nodes[&k.0].label, k.1);
    for start in graph.keys() {
        if depth.contains_key(start) {
            continue;
        }
        // (key, next read to visit); keys on the stack are in progress.
        let mut stack: Vec<(Key, usize)> = vec![(*start, 0)];
        while let Some(&mut (key, ref mut next)) = stack.last_mut() {
            let reads = &graph[&key].1;
            if *next < reads.len() {
                let read = reads[*next];
                *next += 1;
                if !graph.contains_key(&read) || depth.contains_key(&read) {
                    continue;
                }
                if let Some(at) = stack.iter().position(|(k, _)| *k == read) {
                    let path: Vec<String> = stack[at..].iter().map(|(k, _)| name(k)).collect();
                    return fail(format!(
                        "expression links form a loop: {} → {}",
                        path.join(" → "),
                        name(&read)
                    ));
                }
                if stack.len() >= MAX_LINK_DEPTH {
                    return fail(format!(
                        "{}: expressions read through more than {MAX_LINK_DEPTH} links",
                        name(start)
                    ));
                }
                stack.push((read, 0));
                continue;
            }
            let own = reads
                .iter()
                .filter_map(|r| depth.get(r))
                .max()
                .map_or(1, |d| d + 1);
            if own > MAX_LINK_DEPTH {
                return fail(format!(
                    "{}: expressions read through more than {MAX_LINK_DEPTH} links",
                    name(&key)
                ));
            }
            depth.insert(key, own);
            out.push(key);
            stack.pop();
        }
    }
    Ok(out)
}
/// Whether every expression links by node id to an existing animatable parameter,
/// without loops.
pub(crate) fn check(composition: &Composition) -> Result<()> {
    let graph = graph(composition)?;
    if !graph.is_empty() {
        order(composition, &graph)?;
    }
    Ok(())
}
impl Composition {
    /// Store every expression's links by node id (a link may name a node by its label
    /// when that label is unique) and attach the linked curves, so expressions sample
    /// on their own. Run after every change; the result passes `validate`.
    pub fn resolve_expressions(&mut self) -> Result<()> {
        let mut found = false;
        let ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        for id in &ids {
            let names = scalar_names(&self.nodes[id]);
            for name in names {
                let Some(Scalar::Expression { expression, .. }) =
                    self.nodes[id].operation.scalar(name)
                else {
                    continue;
                };
                found = true;
                let source = expression.clone();
                let canonical = self
                    .canonical(&source)
                    .map_err(|e| Error(format!("{} {}: {}", self.nodes[id].label, name, e.0)))?;
                if canonical != source {
                    *self
                        .nodes
                        .get_mut(id)
                        .unwrap()
                        .operation
                        .scalar_mut(name)
                        .unwrap() = Scalar::Expression {
                        expression: canonical,
                        resolved: None,
                    };
                }
            }
        }
        if !found {
            return Ok(());
        }
        let graph = graph(self)?;
        let rate = self.frame_rate.seconds()?;
        for key in order(self, &graph)? {
            let (program, reads) = &graph[&key];
            let links = reads
                .iter()
                .map(|(node, parameter)| {
                    self.nodes[node]
                        .operation
                        .scalar(parameter)
                        .unwrap()
                        .clone()
                })
                .collect();
            let parameter = self.nodes[&key.0]
                .operation
                .descriptor()
                .parameters
                .iter()
                .find(|p| p.name == key.1)
                .unwrap();
            let range = (
                parameter.min.unwrap_or(f32::MIN),
                parameter.max.unwrap_or(f32::MAX),
            );
            let scalar = self
                .nodes
                .get_mut(&key.0)
                .unwrap()
                .operation
                .scalar_mut(key.1)
                .unwrap();
            if let Scalar::Expression { resolved, .. } = scalar {
                *resolved = Some(Arc::new(Resolved {
                    program: program.clone(),
                    links,
                    range,
                    rate,
                }));
            }
        }
        Ok(())
    }
    /// Whether some expression has not had its links attached (`resolve_expressions`).
    pub fn needs_resolving(&self) -> bool {
        self.nodes.values().any(|n| {
            scalar_names(n).into_iter().any(|p| {
                matches!(
                    n.operation.scalar(p),
                    Some(Scalar::Expression { resolved: None, .. })
                )
            })
        })
    }
    /// Every node whose parameters the expressions of `ids` read, directly or through
    /// other expressions (`ids` themselves excluded).
    pub fn linked_nodes(&self, ids: impl IntoIterator<Item = NodeId>) -> Vec<NodeId> {
        let start: std::collections::BTreeSet<NodeId> = ids.into_iter().collect();
        let mut seen = std::collections::BTreeSet::new();
        let mut todo: Vec<NodeId> = start.iter().copied().collect();
        let mut out = Vec::new();
        while let Some(id) = todo.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            for name in scalar_names(node) {
                let Some(Scalar::Expression { expression, .. }) = node.operation.scalar(name)
                else {
                    continue;
                };
                let Ok(program) = veylvik_expr::parse(expression) else {
                    continue;
                };
                for link in program.links() {
                    if let Ok(target) = link.node.parse::<NodeId>()
                        && self.nodes.contains_key(&target)
                        && seen.insert(target)
                    {
                        todo.push(target);
                        if !start.contains(&target) {
                            out.push(target);
                        }
                    }
                }
            }
        }
        out
    }
    /// `source` with links naming a node by label rewritten to its id.
    pub fn canonical(&self, source: &str) -> Result<String> {
        let mut problem = None;
        let out = veylvik_expr::relink(source, |text| {
            if text
                .parse::<NodeId>()
                .is_ok_and(|id| self.nodes.contains_key(&id))
            {
                return None;
            }
            let mut named = self.nodes.values().filter(|n| n.label == text);
            match (named.next(), named.next()) {
                (Some(node), None) => Some(node.id.to_string()),
                (Some(_), Some(_)) => {
                    problem = Some(format!("more than one node is called {text}; rename one"));
                    None
                }
                (None, _) => {
                    problem = Some(format!("no node is called {text}"));
                    None
                }
            }
        })
        .map_err(|e| Error(format!("expression: {e}")))?;
        match problem {
            Some(p) => fail(p),
            None => Ok(out),
        }
    }
    /// `source` with links shown by node label where that label is unique, for editing.
    pub fn readable(&self, source: &str) -> String {
        veylvik_expr::relink(source, |text| {
            let node = self.nodes.get(&text.parse::<NodeId>().ok()?)?;
            let unique = self
                .nodes
                .values()
                .filter(|n| n.label == node.label)
                .count()
                == 1;
            (unique && !node.label.contains(']')).then(|| node.label.clone())
        })
        .unwrap_or_else(|_| source.to_string())
    }
}

/// Rewrites the links in a copied node's expressions to the copies of the nodes they read,
/// by `mapping` (old id to new id): a preset placed into a project, or nodes pasted, then
/// read each other rather than the originals (or nothing). Links to nodes not copied stay.
pub fn moved_links(
    operation: &mut crate::Operation,
    mapping: &std::collections::BTreeMap<NodeId, NodeId>,
) -> Result<()> {
    fn walk(
        value: &mut serde_json::Value,
        mapping: &std::collections::BTreeMap<NodeId, NodeId>,
    ) -> Result<()> {
        match value {
            serde_json::Value::Object(fields) => {
                if fields.get("kind").and_then(|k| k.as_str()) == Some("expression")
                    && let Some(serde_json::Value::String(source)) = fields.get_mut("expression")
                {
                    *source = veylvik_expr::relink(source, |text| {
                        text.parse::<NodeId>()
                            .ok()
                            .and_then(|id| mapping.get(&id))
                            .map(|id| id.to_string())
                    })
                    .map_err(|e| crate::Error(e.to_string()))?;
                }
                for inner in fields.values_mut() {
                    walk(inner, mapping)?;
                }
            }
            serde_json::Value::Array(items) => {
                for inner in items {
                    walk(inner, mapping)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut value = serde_json::to_value(&*operation).map_err(|e| crate::Error(e.to_string()))?;
    if !value.to_string().contains("\"expression\"") {
        return Ok(());
    }
    walk(&mut value, mapping)?;
    *operation = serde_json::from_value(value).map_err(|e| crate::Error(e.to_string()))?;
    Ok(())
}
