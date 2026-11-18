//! A plain name for an undo step, read from the commands that made it and the composition before them. For people to read in a history list; nothing depends on the wording.
use crate::{Command, Composition, NodeId};

const LONGEST: usize = 80;

fn name(doc: &Composition, id: &NodeId) -> String {
    doc.nodes
        .get(id)
        .map(|node| node.label.clone())
        .or_else(|| doc.groups.get(id).map(|group| group.label.clone()))
        .unwrap_or_else(|| "a node".into())
}
fn words(parameter: &str) -> String {
    parameter.replace(['_', '.'], " ")
}
fn one(command: &Command, before: &Composition) -> String {
    match command {
        Command::Put { node } => {
            if before.nodes.contains_key(&node.id) {
                format!("Change {}", node.label)
            } else {
                format!("Add {}", node.label)
            }
        }
        Command::SetParameter { id, parameter, .. } => {
            format!("Change {} of {}", words(parameter), name(before, id))
        }
        Command::SetKey { id, parameter, .. } => {
            format!("Key {} of {}", words(parameter), name(before, id))
        }
        Command::RemoveKey { id, parameter, .. } => {
            format!(
                "Remove a key of {} of {}",
                words(parameter),
                name(before, id)
            )
        }
        Command::SetTrackKey { id, track, .. } => {
            format!("Key {} of {}", words(track), name(before, id))
        }
        Command::Remove { id } => format!("Remove {}", name(before, id)),
        Command::Output { id } => format!("Make {} the output", name(before, id)),
        Command::Resize { width, height } => format!("Resize to {width} x {height}"),
        Command::Place { id, .. } => format!("Move {}", name(before, id)),
        Command::FrameRate { .. } => "Change the frame rate".into(),
        Command::FrameCount { .. } => "Change the length".into(),
        Command::OcioViews { .. } | Command::WorkingSpace { .. } | Command::Display { .. } => {
            "Change colour settings".into()
        }
        Command::Markers { markers } => match markers.len().cmp(&before.markers.len()) {
            std::cmp::Ordering::Greater => "Add a marker".into(),
            std::cmp::Ordering::Less => "Remove a marker".into(),
            std::cmp::Ordering::Equal => "Change a marker".into(),
        },
        Command::Connect { id, source, .. } => format!(
            "{} {}",
            if source.is_some() {
                "Connect"
            } else {
                "Disconnect"
            },
            name(before, id)
        ),
        Command::Annotate { id, .. } => format!("Tag or note {}", name(before, id)),
        Command::Backdrops { backdrops } => match backdrops.len().cmp(&before.backdrops.len()) {
            std::cmp::Ordering::Greater => "Add a backdrop".into(),
            std::cmp::Ordering::Less => "Remove a backdrop".into(),
            std::cmp::Ordering::Equal => "Change a backdrop".into(),
        },
        Command::PutGroup { group } => {
            if before.groups.contains_key(&group.id) {
                format!("Change group {}", group.label)
            } else {
                format!("Group {}", group.label)
            }
        }
        Command::Ungroup { id } => format!("Ungroup {}", name(before, id)),
        Command::SetControl { id, control, .. } => {
            format!("Change {} of {}", words(control), name(before, id))
        }
        Command::ConnectGroup { id, input, .. } => {
            format!("Connect {} of {}", words(input), name(before, id))
        }
    }
}
/// The name of the step `commands` make from `before`.
pub fn step(commands: &[Command], before: &Composition) -> String {
    let text = match commands {
        [] => "No change".into(),
        [only] => one(only, before),
        many if many.iter().all(|c| matches!(c, Command::Place { .. })) => {
            format!("Arrange {} nodes", many.len())
        }
        many => {
            // Lead with what was added, since that is what the artist did; the rest
            // (connections, placement) follows from it.
            let lead = many
                .iter()
                .find(
                    |c| matches!(c, Command::Put { node } if !before.nodes.contains_key(&node.id)),
                )
                .or_else(|| many.iter().find(|c| !matches!(c, Command::Place { .. })))
                .unwrap_or(&many[0]);
            format!("{} and {} more", one(lead, before), many.len() - 1)
        }
    };
    if text.chars().count() > LONGEST {
        text.chars().take(LONGEST - 3).collect::<String>() + "..."
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use crate::{Command, Document, Node, NodePosition, Operation, Time, blank, nodes};
    #[test]
    fn steps_are_named_and_the_names_follow_undo_and_redo() {
        let composition = blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
        let background = composition.output;
        let mut document = Document::new(composition).unwrap();
        let apply = |document: &mut Document, commands: &[Command]| {
            let revision = document.revision();
            document.apply(revision, commands).unwrap();
        };
        let blur = Node::new(
            "Soft focus",
            Operation::Blur(nodes::Blur {
                radius: 2.0.into(),
                filter: Default::default(),
            }),
            &[("image", background)],
        );
        let id = blur.id;
        apply(
            &mut document,
            &[Command::Put {
                node: Box::new(blur.clone()),
            }],
        );
        apply(
            &mut document,
            &[
                Command::Output { id },
                Command::Place { id, position: None },
            ],
        );
        apply(
            &mut document,
            &[Command::SetKey {
                id,
                parameter: "radius".into(),
                time: Time::new(1, 1).unwrap(),
                value: 9.0,
            }],
        );
        apply(
            &mut document,
            &[Command::Put {
                node: Box::new(blur),
            }],
        );
        let place = |x| Command::Place {
            id,
            position: Some(NodePosition { x, y: 0.0 }),
        };
        apply(&mut document, &[place(10.0), place(20.0), place(30.0)]);
        apply(&mut document, &[Command::FrameCount { frame_count: 48 }]);
        let names = [
            "Add Soft focus",
            "Make Soft focus the output and 1 more",
            "Key radius of Soft focus",
            "Change Soft focus",
            "Arrange 3 nodes",
            "Change the length",
        ];
        assert_eq!(document.steps(), (names.map(String::from).to_vec(), vec![]));
        // Two steps back: they wait to be redone, nearest first.
        for _ in 0..2 {
            let revision = document.revision();
            document.undo(revision).unwrap();
        }
        let (undo, redo) = document.steps();
        assert_eq!(undo, names[..4]);
        assert_eq!(redo, ["Arrange 3 nodes", "Change the length"]);
        let revision = document.revision();
        document.redo(revision).unwrap();
        let (undo, redo) = document.steps();
        assert_eq!(
            (undo.len(), redo),
            (5, vec!["Change the length".to_owned()])
        );
        assert_eq!(undo[4], "Arrange 3 nodes");
        // Fewer steps kept: the oldest are forgotten at once, the composition stays.
        let length = document.snapshot().frame_count;
        document.set_history_limits(3, usize::MAX);
        let (undo, redo) = document.steps();
        assert_eq!(undo.len() + redo.len(), 3);
        assert_eq!(undo.last().unwrap(), "Arrange 3 nodes");
        assert_eq!(document.snapshot().frame_count, length);
        document.set_history_limits(0, usize::MAX);
        assert_eq!(
            document.steps().0.len() + document.steps().1.len(),
            1,
            "never fewer than one step"
        );
        document.set_history_limits(1000, usize::MAX);
        // A new edit forgets what could be redone; long names are cut.
        let mut long = document.snapshot().nodes[&id].clone();
        long.label = "x".repeat(200);
        apply(
            &mut document,
            &[Command::Put {
                node: Box::new(long),
            }],
        );
        let (undo, redo) = document.steps();
        assert!(redo.is_empty());
        let last = undo.last().unwrap();
        assert!(
            last.starts_with("Change xxx") && last.ends_with("...") && last.chars().count() == 80
        );
    }
}
