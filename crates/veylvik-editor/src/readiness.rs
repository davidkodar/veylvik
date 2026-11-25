//! Whether the editor can take an action now, and if not why: one rule for the menus, the panels, dropped files and live control, so they refuse alike and say the same thing.
use crate::Studio;

/// What holds the editor back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Busy {
    /// A file job (import, save, export, tracking...) is running.
    FileJob,
    /// A dialog that edits the composition is open (named).
    Dialog(&'static str),
    /// A question (save before closing?) waits for an answer.
    Question,
    /// Something is being dragged in the graph or the curves.
    Dragging,
}
impl Busy {
    pub(crate) fn message(self) -> String {
        match self {
            Self::FileJob => "editor is busy: a file operation is running".into(),
            Self::Dialog(name) => format!("editor is busy: close the {name} dialog first"),
            Self::Question => "editor is busy: answer the open question first".into(),
            Self::Dragging => "editor is busy: finish the drag first".into(),
        }
    }
}
/// What the rule looks at.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Activity {
    pub file_job: bool,
    pub dialog: Option<&'static str>,
    pub question: bool,
    pub dragging: bool,
}
/// The first thing that holds the editor back, if any.
pub(crate) fn busy(now: Activity) -> Option<Busy> {
    [
        (now.file_job, Busy::FileJob),
        (now.dialog.is_some(), Busy::Dialog(now.dialog.unwrap_or(""))),
        (now.question, Busy::Question),
        (now.dragging, Busy::Dragging),
    ]
    .into_iter()
    .find_map(|(held, why)| held.then_some(why))
}
impl Studio {
    /// The dialog that edits the composition and is open now, by name. Every such dialog
    /// is listed here once: while one is open the editor takes no other edits.
    pub(crate) fn open_dialog(&self) -> Option<&'static str> {
        [
            (self.preferences.open, "Preferences"),
            (self.library.open, "Effect library"),
            (self.browser.choosing(), "Library choice"),
            (self.occlusion.open, "Occlusion"),
            (self.effects.open, "Effect group"),
            (self.outline_controls.open, "Outline text"),
            (self.composition_controls.open, "Composition settings"),
            (self.media_import.is_some(), "Match media"),
            (self.node_palette.open, "Add node"),
            (self.graph.renaming(), "Rename node"),
            (self.node_finder.open, "Find node"),
        ]
        .into_iter()
        .find_map(|(open, name)| open.then_some(name))
    }
    pub(crate) fn authoring_dialog_open(&self) -> bool {
        self.open_dialog().is_some()
    }
    /// What holds the editor back from an action now, if anything.
    pub(crate) fn busy(&self) -> Option<Busy> {
        busy(Activity {
            file_job: self.files.busy(),
            dialog: self.open_dialog(),
            question: self.confirm.is_some(),
            dragging: self.graph.dragging() || self.animation_editor.dragging(),
        })
    }
    pub(crate) fn ready(&self) -> bool {
        self.busy().is_none()
    }
    /// Nothing holds the editor back but, at most, a drag in progress: what gates whole
    /// panels and windows, which must stay live while something in them is dragged
    /// (disabling the widget being dragged would end the drag and never let it finish).
    pub(crate) fn settled(&self) -> bool {
        matches!(self.busy(), None | Some(Busy::Dragging))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_editor_is_ready_only_when_nothing_holds_it_and_says_what_does() {
        assert_eq!(busy(Activity::default()), None);
        let all = Activity {
            file_job: true,
            dialog: Some("Find node"),
            question: true,
            dragging: true,
        };
        assert_eq!(busy(all), Some(Busy::FileJob));
        assert_eq!(
            busy(Activity {
                question: true,
                ..Default::default()
            }),
            Some(Busy::Question)
        );
        assert_eq!(
            busy(Activity {
                dragging: true,
                ..Default::default()
            }),
            Some(Busy::Dragging)
        );
        assert_eq!(
            busy(Activity {
                dialog: Some("Find node"),
                ..Default::default()
            }),
            Some(Busy::Dialog("Find node"))
        );
        assert!(
            Busy::Dialog("Find node")
                .message()
                .contains("Find node dialog")
        );
    }
}
