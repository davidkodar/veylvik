//! Whether the editor can take an action now, and if not why: one rule for the menus, the panels, dropped files and live control, so they refuse alike and say the same thing.
use crate::Studio;

/// What holds the editor back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Busy {
    /// A file job (import, save, export, tracking...) is running.
    FileJob,
    /// A dialog that edits the composition is open.
    Dialog,
    /// A question (save before closing?) waits for an answer.
    Question,
    /// Something is being dragged in the graph or the curves.
    Dragging,
}
impl Busy {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::FileJob => "editor is busy: a file operation is running",
            Self::Dialog => "editor is busy: close the open dialog first",
            Self::Question => "editor is busy: answer the open question first",
            Self::Dragging => "editor is busy: finish the drag first",
        }
    }
}
/// What the rule looks at.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Activity {
    pub file_job: bool,
    pub dialog: bool,
    pub question: bool,
    pub dragging: bool,
}
/// The first thing that holds the editor back, if any.
pub(crate) fn busy(now: Activity) -> Option<Busy> {
    [
        (now.file_job, Busy::FileJob),
        (now.dialog, Busy::Dialog),
        (now.question, Busy::Question),
        (now.dragging, Busy::Dragging),
    ]
    .into_iter()
    .find_map(|(held, why)| held.then_some(why))
}
impl Studio {
    /// What holds the editor back from an action now, if anything.
    pub(crate) fn busy(&self) -> Option<Busy> {
        busy(Activity {
            file_job: self.files.busy(),
            dialog: self.authoring_dialog_open(),
            question: self.confirm.is_some(),
            dragging: self.graph.dragging() || self.animation_editor.dragging(),
        })
    }
    pub(crate) fn ready(&self) -> bool {
        self.busy().is_none()
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
            dialog: true,
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
        assert!(Busy::Dialog.message().contains("dialog"));
    }
}
