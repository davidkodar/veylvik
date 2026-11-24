use eframe::egui;
use std::{path::PathBuf, sync::mpsc};
use veylvik_document::NodeId;
use veylvik_presets::title_style::TitleStyle;
type Result<T> = std::result::Result<T, String>;
enum Completed {
    List(Vec<(PathBuf, String)>),
    Saved,
    Loaded(Box<TitleStyle>, NodeId, u64, NodeId),
}
#[derive(Default)]
pub struct Library {
    name: String,
    entries: Vec<(PathBuf, String)>,
    pending: Option<mpsc::Receiver<Result<Completed>>>,
    notice: String,
}
fn directory() -> Result<PathBuf> {
    veylvik_io::presets::directory()
        .map(|p| p.with_file_name("title-styles"))
        .map_err(|e| e.to_string())
}
fn load(path: &std::path::Path) -> Result<TitleStyle> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Title style exceeds 16 MiB".into());
    }
    let style: TitleStyle = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    style.validate().map_err(|e| e.to_string())?;
    Ok(style)
}
impl Library {
    fn task(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<Completed> + Send + 'static,
    ) {
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        self.pending = Some(receive);
        std::thread::spawn(move || {
            let _ = send.send(work());
            ctx.request_repaint();
        });
    }
}
impl crate::Studio {
    pub(super) fn title_style_library(&mut self, ui: &mut egui::Ui) {
        let result = self
            .title_styles
            .pending
            .as_ref()
            .and_then(|r| r.try_recv().ok());
        if let Some(result) = result {
            self.title_styles.pending = None;
            match result {
                Ok(Completed::List(entries)) => self.title_styles.entries = entries,
                Ok(Completed::Saved) => {
                    self.title_styles.notice = "Style saved. Refresh the list to use it.".into()
                }
                Ok(Completed::Loaded(style, id, revision, session)) => {
                    if self.project.session != session
                        || self.draft.id != id
                        || self.project.document.revision() != revision
                        || self.draft_dirty
                    {
                        self.title_styles.notice =
                            "The title changed while loading. Choose the style again.".into();
                    } else {
                        match style.apply(self.project.document.snapshot(), id) {
                            Ok(commands) => match self.project.apply(&commands) {
                                Ok(()) => {
                                    self.refresh_draft();
                                    self.request_preview();
                                }
                                Err(e) => self.fail(e),
                            },
                            Err(e) => self.fail(e),
                        }
                    }
                }
                Err(e) => self.title_styles.notice = e,
            }
        }
        ui.collapsing("Saved text styles",|ui|{
            ui.weak("Save typography, procedural fills and effects. Applying preserves your wording and position.");
            ui.add(egui::TextEdit::singleline(&mut self.title_styles.name).hint_text("Style name…"));
            ui.add_enabled_ui(self.title_styles.pending.is_none(),|ui|{
                if ui.button("Save current style").clicked() && self.apply_draft(){
                    match TitleStyle::capture(self.project.document.snapshot(),self.draft.id,&self.title_styles.name){Ok(style)=>self.title_styles.task(ui.ctx(),move||{let dir=directory()?;std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let bytes=serde_json::to_vec(&style).map_err(|e|e.to_string())?;if bytes.len()>16*1024*1024{return Err("Title style exceeds 16 MiB".into());}veylvik_document::atomic_create(&dir.join(format!("{}.veytitle",NodeId::new_v4())),&bytes).map_err(|e|e.to_string())?;Ok(Completed::Saved)}),Err(e)=>self.title_styles.notice=e.to_string()}
                }
                if ui.button("Refresh saved styles").clicked(){self.title_styles.task(ui.ctx(),move||{let dir=directory()?;std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let mut entries=Vec::new();for file in std::fs::read_dir(dir).map_err(|e|e.to_string())?.take(128){let file=file.map_err(|e|e.to_string())?;let path=file.path();if path.extension().is_some_and(|e|e=="veytitle"){let style=load(&path)?;entries.push((path,style.label));}}entries.sort_by(|a,b|a.1.cmp(&b.1));Ok(Completed::List(entries))});}
                let mut chosen=None;
                egui::ComboBox::from_id_salt("saved-title-style").selected_text("Apply saved style…").show_ui(ui,|ui|{for (path,label) in &self.title_styles.entries {if ui.button(label).clicked(){chosen=Some(path.clone());}}});
                if let Some(path)=chosen && self.apply_draft(){let id=self.draft.id;let revision=self.project.document.revision();let session=self.project.session;self.title_styles.task(ui.ctx(),move||Ok(Completed::Loaded(Box::new(load(&path)?),id,revision,session)));}
            });
            if self.title_styles.pending.is_some(){ui.spinner();}
            ui.label(&self.title_styles.notice);
        });
    }
}
