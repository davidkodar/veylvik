//! Portable typography and procedural appearance, independent of the title wording.
use crate::{presets::text_style, *};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleStyle {
    pub schema: u32,
    pub label: String,
    pub text: NodeId,
    pub composition: Composition,
}
impl TitleStyle {
    pub fn capture(doc: &Composition, text: NodeId, label: &str) -> Result<Self> {
        let output = text_style::chain(doc, text)?
            .last()
            .copied()
            .unwrap_or(text_style::output(doc, text)?);
        let mut pending = vec![output];
        let mut keep = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if keep.insert(id) {
                pending.extend(doc.nodes[&id].inputs.values().copied());
            }
        }
        if !keep.contains(&text) {
            return fail("Style has no editable text");
        }
        let mut composition = doc.clone();
        composition.output = output;
        composition.nodes.retain(|id, _| keep.contains(id));
        composition
            .groups
            .retain(|_, g| g.members.iter().all(|id| keep.contains(id)));
        let style = Self {
            schema: 1,
            label: label.into(),
            text,
            composition,
        };
        style.validate()?;
        Ok(style)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 || self.label.trim().is_empty() || self.label.len() > 128 {
            return fail("Invalid title style name or version");
        }
        self.composition.validate()?;
        if !self
            .composition
            .nodes
            .get(&self.text)
            .is_some_and(|n| matches!(n.operation, Operation::Text(crate::nodes::Text { .. })))
        {
            return fail("Title style needs an editable text source");
        }
        if self.composition.nodes.values().any(|n| {
            matches!(
                n.operation,
                Operation::Image(crate::nodes::Image { .. })
                    | Operation::Footage(crate::nodes::Footage { .. })
            )
        }) {
            return fail(
                "Image-based fills need external assets. Save procedural materials or gradients as title styles.",
            );
        }
        Ok(())
    }
    pub fn apply(&self, doc: &Composition, text: NodeId) -> Result<Vec<Command>> {
        self.validate()?;
        let Operation::Text(crate::nodes::Text { content, x, y, .. }) = &doc
            .nodes
            .get(&text)
            .ok_or_else(|| Error("Select editable text".into()))?
            .operation
        else {
            return fail("Select editable text");
        };
        let mut source = self.composition.clone();
        crate::presets::resolution::map_graph(
            &mut source.nodes,
            &mut source.groups,
            [self.composition.width, self.composition.height],
            [doc.width, doc.height],
            crate::presets::resolution::Sizing::Fit,
        )?;
        let mut ids: std::collections::BTreeMap<_, _> = source
            .nodes
            .keys()
            .map(|id| (*id, NodeId::new_v4()))
            .collect();
        ids.insert(self.text, text);
        let mut commands = Vec::new();
        for node in source.nodes.values() {
            let mut next = node.clone();
            next.id = ids[&node.id];
            for input in next.inputs.values_mut() {
                *input = ids[input];
            }
            if node.id == self.text {
                next.label = doc.nodes[&text].label.clone();
                next.position = doc.nodes[&text].position;
                if let Operation::Text(crate::nodes::Text {
                    content: wording,
                    typography,
                    x: tx,
                    y: ty,
                    ..
                }) = &mut next.operation
                {
                    typography.remap(wording, content);
                    *wording = content.clone();
                    *tx = x.clone();
                    *ty = y.clone();
                }
            }
            commands.push(Command::Put {
                node: Box::new(next),
            });
        }
        for group in source.groups.values() {
            let mut group = group.clone();
            group.id = NodeId::new_v4();
            group.output = ids[&group.output];
            group.members = group.members.iter().map(|id| ids[id]).collect();
            for input in group.inputs.values_mut() {
                for target in &mut input.targets {
                    target.node = ids[&target.node];
                }
            }
            for control in group.controls.values_mut() {
                control.node = ids[&control.node];
            }
            commands.push(Command::PutGroup {
                group: Box::new(group),
            });
        }
        // Existing effects are disconnected; they remain available for node-level reuse.
        let previous = text_style::chain(doc, text)?
            .last()
            .copied()
            .unwrap_or(text_style::output(doc, text)?);
        let output = ids[&source.output];
        crate::presets::connect_after(doc, previous, output, commands)
    }
}
