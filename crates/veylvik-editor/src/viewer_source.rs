//! What the viewer shows and how: the composition the picture is made from (a key being previewed, the inspector's unapplied changes, or the saved composition) and the look kept pictures are made with. The preview request and preparing frames ahead both ask here, so they agree on when the picture is the saved one.
use crate::Studio;
use veylvik_document::Command;

/// Where the viewer's picture comes from.
pub(crate) enum Source {
    /// A key being dragged on the timeline, previewed before it is applied.
    KeyPreview(Vec<Command>),
    /// The inspector's changes, not applied yet.
    Draft(Vec<Command>),
    /// The saved composition.
    Saved,
}
/// The look a kept picture is made with, from the viewer's exposure and display.
pub(crate) fn look(view: &crate::display::ViewSettings) -> veylvik_preview::screen::Look {
    veylvik_preview::screen::Look {
        exposure: if view.exposure.is_finite() {
            view.exposure.clamp(-16.0, 16.0)
        } else {
            0.0
        },
        filmic: view.filmic,
        host: view.host,
    }
}
impl Studio {
    pub(crate) fn viewer_source(&self) -> Source {
        if let Some(commands) = self.key_timeline.preview_commands() {
            Source::KeyPreview(commands.to_vec())
        } else if self.draft_dirty {
            Source::Draft(self.draft_commands())
        } else {
            Source::Saved
        }
    }
    /// The viewer shows the saved composition as the camera films it: the only picture
    /// worth preparing ahead and keeping.
    pub(crate) fn shows_saved(&self) -> bool {
        !self.draft_dirty
            && self.key_timeline.preview_commands().is_none()
            && self.scene_view.is_none()
            && !self.scene_reflections
    }
    pub(crate) fn screen_look(&self) -> veylvik_preview::screen::Look {
        look(&self.effective_view())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_kept_picture_never_takes_an_exposure_it_cannot_show() {
        let mut view = crate::display::ViewSettings {
            exposure: 40.0,
            ..Default::default()
        };
        assert_eq!(look(&view).exposure, 16.0);
        view.exposure = f32::NAN;
        assert_eq!(look(&view).exposure, 0.0);
    }
}
