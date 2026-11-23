//! File > Import Nuke script: the script is rebuilt on the file worker as a new, unsaved composition beside the script, and what did not come over is said in the status line and the messages.
use crate::files::{self, Choice, Completed};
use crate::project::Project;

pub struct Imported {
    pub project: Project,
    pub summary: String,
}
impl crate::Studio {
    pub(crate) fn import_nuke(&mut self) {
        let folder = self.project.base.clone();
        let result = self.files.start(move || {
            let Some(path) = files::select(Choice::Nuke, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let imported = veylvik_nuke::import(&text)?;
            let rate = imported.composition.frame_rate;
            let c = &imported.composition;
            let mut project = Project::blank(c.width, c.height, rate, c.frame_count)
                .map_err(|e| e.to_string())?;
            project.selected = imported.composition.output;
            project.document =
                veylvik_document::Document::new(imported.composition).map_err(|e| e.to_string())?;
            if let Some(parent) = path.parent() {
                project.base = parent.to_path_buf();
            }
            // Media missing from this machine is listed, not refused, as when opening.
            project.load_available_inputs();
            let mut summary = format!("Rebuilt {} nodes from {}", imported.rebuilt, path.display());
            if !imported.not_rebuilt.is_empty() {
                summary += &format!(
                    "; not rebuilt (passed through): {}",
                    imported.not_rebuilt.join(", ")
                );
            }
            for note in &imported.notes {
                summary += &format!("; {note}");
            }
            Ok(Completed::NukeImported(Box::new(Imported {
                project,
                summary,
            })))
        });
        self.file_started(result);
    }
}
