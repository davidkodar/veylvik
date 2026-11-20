//! Settings carried between nodes: copy and paste a node's settings, and apply an inspector change to every selected node of the same kind. Both work on a node's settings by name, so they need no knowledge of node types; a result that is not a valid node is never produced. Plus the text that filters the inspector's sliders.
use crate::Studio;
use eframe::egui;
use veylvik_document::{Command, Node, NodeId, Operation};

/// Settings that say which file a node reads; pasting never changes them.
const SOURCES: [&str; 2] = ["asset", "layer"];

fn fields(operation: &Operation) -> Option<serde_json::Map<String, serde_json::Value>> {
    match serde_json::to_value(operation).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}
fn kind(map: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
    map.get("type").and_then(serde_json::Value::as_str)
}
fn built(map: serde_json::Map<String, serde_json::Value>) -> Option<Operation> {
    let operation: Operation = serde_json::from_value(serde_json::Value::Object(map)).ok()?;
    operation.validate().ok()?;
    Some(operation)
}
/// The names of the settings in which `after` differs from `before` (same kind of node).
pub(crate) fn changed(before: &Operation, after: &Operation) -> Vec<String> {
    let (Some(a), Some(b)) = (fields(before), fields(after)) else {
        return Vec::new();
    };
    if kind(&a) != kind(&b) {
        return Vec::new();
    }
    b.iter()
        .filter(|(name, value)| name.as_str() != "type" && a.get(name.as_str()) != Some(value))
        .map(|(name, _)| name.clone())
        .collect()
}
/// `target` with the settings `names` taken from `source`; None when the two are not
/// the same kind of node, nothing would change, or the result would not be valid.
pub(crate) fn with_settings(
    target: &Operation,
    source: &Operation,
    names: &[String],
) -> Option<Operation> {
    let (mut to, from) = (fields(target)?, fields(source)?);
    if kind(&to) != kind(&from) {
        return None;
    }
    let mut any = false;
    for name in names {
        if let Some(value) = from.get(name)
            && to.get(name) != Some(value)
        {
            to.insert(name.clone(), value.clone());
            any = true;
        }
    }
    any.then(|| built(to)).flatten()
}
/// `target` with every setting of `source` that it can take: all of them for the same
/// kind of node, and for another kind those of the same name that still make a valid
/// node. Which file a node reads is never pasted. Returns how many settings changed.
pub(crate) fn pasted(target: &Operation, source: &Operation) -> Option<(Operation, usize)> {
    let (mut to, from) = (fields(target)?, fields(source)?);
    let same = kind(&to) == kind(&from);
    let mut count = 0;
    for (name, value) in &from {
        if name == "type"
            || SOURCES.contains(&name.as_str())
            || !to.contains_key(name)
            || to.get(name) == Some(value)
        {
            continue;
        }
        if same {
            to.insert(name.clone(), value.clone());
            count += 1;
        } else {
            // One at a time: a setting of the same name may mean something else there.
            let mut candidate = to.clone();
            candidate.insert(name.clone(), value.clone());
            if built(candidate.clone()).is_some() {
                to = candidate;
                count += 1;
            }
        }
    }
    (count > 0)
        .then(|| built(to))
        .flatten()
        .map(|operation| (operation, count))
}
/// Whether a slider named `label` passes the inspector's filter `words`.
pub(crate) fn matches(label: &str, words: &str) -> bool {
    let label = label.to_lowercase().replace('_', " ");
    words
        .split_whitespace()
        .all(|word| label.contains(&word.to_lowercase()))
}
fn filter_id() -> egui::Id {
    egui::Id::new("inspector-filter")
}
/// Whether the inspector should draw the slider named `label`.
pub(crate) fn shown(ctx: &egui::Context, label: &str) -> bool {
    ctx.data(|d| d.get_temp::<String>(filter_id()))
        .is_none_or(|words| matches(label, &words))
}
/// What the inspector tools keep between frames.
pub struct Tools {
    /// The settings copied last, and the name of the node they came from.
    clip: Option<(String, Operation)>,
    /// Apply inspector changes to the other selected nodes of the same kind.
    pub spread: bool,
    filter: String,
}
impl Default for Tools {
    fn default() -> Self {
        Self {
            clip: None,
            spread: true,
            filter: String::new(),
        }
    }
}
impl Studio {
    /// The other selected nodes of the draft's kind, which an inspector change reaches.
    fn spread_targets(&self) -> Vec<NodeId> {
        let doc = self.project.document.snapshot();
        let kind = self.draft.operation.descriptor().type_id;
        self.graph
            .selected_nodes(doc)
            .into_iter()
            .filter(|id| *id != self.draft.id && !self.text_edits.contains_key(id))
            .filter(|id| {
                doc.nodes
                    .get(id)
                    .is_some_and(|n| n.operation.descriptor().type_id == kind)
            })
            .collect()
    }
    /// The pending change carried to the other selected nodes, as commands.
    pub(crate) fn spread_commands(&self) -> Vec<Command> {
        if !self.tools.spread || !self.draft_dirty {
            return Vec::new();
        }
        let doc = self.project.document.snapshot();
        let Some(saved) = doc.nodes.get(&self.draft.id) else {
            return Vec::new();
        };
        let names = changed(&saved.operation, &self.draft.operation);
        if names.is_empty() {
            return Vec::new();
        }
        self.spread_targets()
            .into_iter()
            .filter_map(|id| {
                let node = &doc.nodes[&id];
                with_settings(&node.operation, &self.draft.operation, &names).map(|operation| {
                    Command::Put {
                        node: Box::new(Node {
                            operation,
                            ..node.clone()
                        }),
                    }
                })
            })
            .collect()
    }
    /// The filter field, and the note that a change reaches other selected nodes.
    pub(crate) fn inspector_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tools.filter)
                    .hint_text("Filter sliders")
                    .desired_width(150.0),
            )
            .on_hover_text(
                "Show only the sliders whose names contain these words. Other controls stay",
            );
            if !self.tools.filter.is_empty() && ui.small_button("Clear").clicked() {
                self.tools.filter.clear();
            }
        });
        let words = self.tools.filter.trim().to_owned();
        ui.ctx().data_mut(|d| {
            if words.is_empty() {
                d.remove::<String>(filter_id());
            } else {
                d.insert_temp(filter_id(), words);
            }
        });
        let others = self.spread_targets().len();
        if others > 0 {
            ui.checkbox(
                &mut self.tools.spread,
                format!("Changes also apply to {others} other selected node(s)"),
            )
            .on_hover_text("Only the settings you change are carried over, to the selected nodes of the same kind");
        }
    }
    pub(crate) fn copy_settings(&mut self) {
        self.tools.clip = Some((self.draft.label.clone(), self.draft.operation.clone()));
        self.status = format!("Copied the settings of {}", self.draft.label);
        self.error = false;
    }
    /// The node the copied settings came from, when there are any.
    pub(crate) fn copied_from(&self) -> Option<&str> {
        self.tools.clip.as_ref().map(|(label, _)| label.as_str())
    }
    /// Paste the copied settings onto every selected node, as one undo step.
    pub(crate) fn paste_settings(&mut self) {
        let Some((from, source)) = self.tools.clip.clone() else {
            self.fail("Copy a node's settings first (Node actions > Copy settings).");
            return;
        };
        if !self.apply_draft() {
            return;
        }
        let doc = self.project.document.snapshot();
        let mut targets = self.graph.selected_nodes(doc);
        targets.insert(self.project.selected);
        let mut settings = 0;
        let commands: Vec<Command> = targets
            .iter()
            .filter_map(|id| doc.nodes.get(id))
            .filter_map(|node| {
                pasted(&node.operation, &source).map(|(operation, count)| {
                    settings += count;
                    Command::Put {
                        node: Box::new(Node {
                            operation,
                            ..node.clone()
                        }),
                    }
                })
            })
            .collect();
        if commands.is_empty() {
            self.fail(format!(
                "Nothing to paste: the selected node has no settings in common with {from}, or they are the same already."
            ));
            return;
        }
        let nodes = commands.len();
        match self.project.apply(&commands) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
                self.status =
                    format!("Pasted {settings} setting(s) from {from} onto {nodes} node(s)");
                self.error = false;
            }
            Err(error) => self.fail(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Time, nodes};
    fn shadow(softness: f32, strength: f32, x: f32) -> Operation {
        Operation::Shadow(nodes::Shadow {
            follow_lighting: false,
            distance: 20.0.into(),
            softness: softness.into(),
            strength: strength.into(),
            x: x.into(),
            y: 12.0.into(),
            light: 0.0.into(),
        })
    }
    fn number(operation: &Operation, name: &str) -> f32 {
        operation
            .scalar(name)
            .unwrap()
            .at(Time::new(0, 1).unwrap())
            .unwrap()
    }
    #[test]
    fn only_what_changed_is_carried_to_another_node_of_the_same_kind() {
        let (before, after) = (shadow(8.0, 0.65, 8.0), shadow(30.0, 0.65, 8.0));
        assert_eq!(changed(&before, &after), ["softness"]);
        assert!(changed(&before, &before).is_empty());
        // The other node keeps its own strength and offset, and takes the new softness.
        let other = shadow(2.0, 0.2, -40.0);
        let result = with_settings(&other, &after, &changed(&before, &after)).unwrap();
        assert_eq!(
            (
                number(&result, "softness"),
                number(&result, "strength"),
                number(&result, "x")
            ),
            (30.0, 0.2, -40.0)
        );
        // Nothing to do, another kind of node, or an unknown name: no result.
        assert!(with_settings(&after, &after, &["softness".into()]).is_none());
        let blur = Operation::Blur(nodes::Blur {
            radius: 4.0.into(),
            filter: Default::default(),
        });
        assert!(with_settings(&blur, &after, &["softness".into()]).is_none());
        assert!(changed(&blur, &after).is_empty());
        assert!(with_settings(&other, &after, &["no_such_setting".into()]).is_none());
    }
    #[test]
    fn pasting_takes_everything_from_the_same_kind_and_shared_names_from_another() {
        let (source, target) = (shadow(30.0, 0.9, 5.0), shadow(2.0, 0.2, -40.0));
        let (result, count) = pasted(&target, &source).unwrap();
        assert_eq!(count, 3);
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(&source).unwrap()
        );
        assert!(pasted(&source, &source).is_none());
        // Another kind: an ellipse and a shadow share x and y, and nothing else.
        let ellipse = Operation::Ellipse(nodes::Ellipse {
            x: 100.0.into(),
            y: 200.0.into(),
            radius_x: 50.0.into(),
            radius_y: 60.0.into(),
            feather: 0.0.into(),
        });
        let (result, count) = pasted(&ellipse, &source).unwrap();
        assert_eq!(count, 2);
        assert_eq!((number(&result, "x"), number(&result, "y")), (5.0, 12.0));
        assert_eq!(number(&result, "radius_x"), 50.0);
        // The file a footage node reads is never pasted.
        let clip = |asset: &str, start: i64| {
            let mut footage = nodes::Footage {
                asset: asset.into(),
                timing: Default::default(),
                placement: Default::default(),
                colour: Default::default(),
                layer: None,
                fallback: String::new(),
            };
            footage.timing.start = Time::new(start, 1).unwrap();
            Operation::Footage(footage)
        };
        let (result, count) = pasted(&clip("b.mp4", 0), &clip("a.mp4", 3)).unwrap();
        assert_eq!(count, 1);
        let Operation::Footage(footage) = result else {
            unreachable!()
        };
        assert_eq!(
            (footage.asset.as_str(), footage.timing.start.ticks),
            ("b.mp4", 3)
        );
        assert!(
            matches("Shadow softness", "soft")
                && matches("fade_in", "fade in")
                && !matches("x", "soft")
        );
        assert!(matches("anything", "  "));
    }
}
