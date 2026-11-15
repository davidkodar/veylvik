//! Collection and relinking use file workers; edits apply transactionally to the captured revision.
use crate::{
    Studio,
    files::{self, Choice, Completed},
};
use eframe::egui;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use veylvik_document::{Command, Composition, NodeId};
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    folder: String,
    rows: Vec<veylvik_io::relink::Proposal>,
    session: Option<(NodeId, u64)>,
    work: Option<Arc<Work>>,
    report: String,
}
#[derive(Default)]
struct Work {
    cancel: AtomicBool,
    progress: Mutex<String>,
}
pub enum Finished {
    Collected(veylvik_io::collect::Collected),
    Verified(usize),
    Located {
        session: NodeId,
        revision: u64,
        rows: Vec<veylvik_io::relink::Proposal>,
    },
    Linked {
        session: NodeId,
        revision: u64,
        commands: Vec<Command>,
        images: Vec<(String, veylvik_io::footage::Media)>,
    },
}
enum Action {
    Collect,
    Verify,
    Search,
    Locate(String),
    Apply,
}
impl Studio {
    pub(super) fn portable_window(&mut self, ctx: &egui::Context) {
        let mut open = self.portable.open;
        if !open {
            return;
        }
        let mut action = None;
        let missing: BTreeMap<String, String> = self
            .project
            .document
            .snapshot()
            .nodes
            .values()
            .filter_map(|n| n.operation.asset())
            .filter(|a| {
                !self.project.inputs.contains_key(a) || !self.project.base.join(a).is_file()
            })
            .map(|a| (a.to_owned(), "Media or LUT".into()))
            .chain(
                self.project
                    .document
                    .snapshot()
                    .ocio_transforms()
                    .filter(|t| !t.config.starts_with("ocio://"))
                    .map(|t| (t.config.clone(), "Custom OCIO configuration".into())),
            )
            .collect();
        if self.portable.folder.is_empty() {
            self.portable.folder = "Collected project".into();
        }
        let ready = !self.files.busy() && !self.draft_dirty && self.confirm.is_none();
        egui::Window::new("Project files").open(&mut open).collapsible(false).default_width(720.).default_pos(egui::pos2(250.,90.)).show(ctx,|ui|{
            ui.heading("Portable collection");
            ui.label("Create an independent copy with media, sequences, embedded fonts and colour conversions. Your current project stays unchanged.");
            ui.horizontal(|ui|{ui.label("New folder");ui.add(egui::TextEdit::singleline(&mut self.portable.folder).desired_width(240.));});
            let name=&self.portable.folder;
            let valid=!name.trim().is_empty()&&!matches!(name.as_str(),"."|"..")&&!name.contains(['/', '\\','\0']);
            ui.horizontal(|ui|{
                if ui.add_enabled(ready&&valid,egui::Button::new("Choose parent and collect…")).clicked(){action=Some(Action::Collect);}
                if ui.add_enabled(ready,egui::Button::new("Verify collection…")).on_hover_text("Choose a collected folder and verify SHA-256 hashes and all project references").clicked(){action=Some(Action::Verify);}
            });
            ui.separator();ui.heading("Locate and relink sources");
            ui.weak("Missing media and custom OCIO configs. Search a moved folder or locate a replacement. Name-only matches require your choice.");
            if ui.add_enabled(ready&&!missing.is_empty(),egui::Button::new("Search folder…")).clicked(){action=Some(Action::Search);}
            if missing.is_empty(){ui.label("All media is loaded. No custom OCIO configuration to relink.");}
            egui::ScrollArea::vertical().max_height(320.).show(ui,|ui|{
                for (asset,kind) in &missing{
                    ui.push_id(asset,|ui|{
                        egui::Frame::group(ui.style()).show(ui,|ui|{
                            ui.set_min_width(ui.available_width());
                            ui.add(egui::Label::new(egui::RichText::new(asset).strong()).truncate()).on_hover_text(asset);ui.weak(kind);
                            ui.horizontal(|ui|{
                                if ui.add_enabled(ready,egui::Button::new("Locate…")).clicked(){action=Some(Action::Locate(asset.clone()));}
                                if let Some(row)=self.portable.rows.iter_mut().find(|r|&r.asset==asset){
                                    let label=row.selected.and_then(|i|row.candidates.get(i)).map(|c|c.path.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_else(||"Choose a match…".into());
                                    egui::ComboBox::from_id_salt("candidate").selected_text(label).width(250.).show_ui(ui,|ui|{
                                        ui.selectable_value(&mut row.selected,None,"Do not relink");
                                        for (i,candidate) in row.candidates.iter().enumerate(){ui.selectable_value(&mut row.selected,Some(i),candidate.path.to_string_lossy()).on_hover_text(if candidate.verified{"SHA-256 matches collected file"}else{"Filename suggestion only; verify this is the intended source"});}
                                    });
                                    if let Some(c)=row.selected.and_then(|i|row.candidates.get(i)){ui.weak(if c.verified{"Verified content"}else{"Manual choice"});}
                                    else{ui.weak(format!("{} {}",row.candidates.len(),if row.candidates.len()==1 {"candidate"}else{"candidates"}));}
                                }
                            });
                            if let Some(candidate)=self.portable.rows.iter().find(|r|&r.asset==asset).and_then(|r|r.selected.and_then(|i|r.candidates.get(i))){ui.add(egui::Label::new(candidate.path.to_string_lossy()).truncate()).on_hover_text(candidate.path.to_string_lossy());}
                        });
                    });
                }
            });
            let count=self.portable.rows.iter().filter(|r|r.selected.is_some()).count();
            if ui.add_enabled(ready&&count>0,egui::Button::new(format!("Relink {count} selected sources"))).on_hover_text("Validate every selected file, then update all uses in one undo step").clicked(){action=Some(Action::Apply);}
            if let Some(work)=&self.portable.work{
                ui.separator(); if let Ok(p)=work.progress.lock(){ui.label(p.as_str());}
                if ui.add_enabled(!work.cancel.load(Ordering::Relaxed),egui::Button::new("Cancel operation")).clicked(){work.cancel.store(true,Ordering::Relaxed);}
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            if !self.portable.report.is_empty(){ui.separator();ui.add(egui::Label::new(&self.portable.report).wrap());}
        });
        self.portable.open = open;
        if let Some(action) = action {
            self.portable_action(action, missing.into_keys().collect());
        }
    }
    fn portable_action(&mut self, action: Action, missing: Vec<String>) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        if matches!(action, Action::Apply)
            && self.portable.session
                != Some((self.project.session, self.project.document.revision()))
        {
            self.fail(
                "Project changed since the search. Search or locate again before applying relinks.",
            );
            return;
        }
        let document = self.project.document.snapshot().clone();
        let base = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let name = self.portable.folder.clone();
        let rows = self.portable.rows.clone();
        let source_budget =
            veylvik_io::runtime::source_budget().saturating_sub(self.project.inputs.byte_size());
        let work = Arc::new(Work::default());
        self.portable.work = Some(work.clone());
        self.portable.report.clear();
        let result = self.files.start(move || {
            let update = |text: String| {
                if let Ok(mut p) = work.progress.lock() {
                    *p = text;
                }
                !work.cancel.load(Ordering::Relaxed)
            };
            update("Choose a location…".into());
            let done = match action {
                Action::Collect => {
                    let Some(parent) = files::select(Choice::Directory, &base, "")? else {
                        return Ok(Completed::Cancelled);
                    };
                    let report = veylvik_io::collect::collect_with_progress(
                        &document,
                        &base,
                        &parent.join(name),
                        |p| {
                            update(format!(
                                "{} · {} / {} files · {:.1} / {:.1} MiB",
                                p.phase,
                                p.files,
                                p.total_files,
                                p.bytes as f64 / 1048576.,
                                p.total_bytes as f64 / 1048576.
                            ))
                        },
                    )
                    .map_err(|e| e.to_string())?;
                    Finished::Collected(report)
                }
                Action::Verify => {
                    let Some(folder) = files::select(Choice::Directory, &base, "")? else {
                        return Ok(Completed::Cancelled);
                    };
                    let count = veylvik_io::collect::verify_with_progress(&folder, |n, bytes| {
                        update(format!(
                            "Verifying {n} files · {:.1} MiB",
                            bytes as f64 / 1048576.
                        ))
                    })
                    .map_err(|e| e.to_string())?;
                    Finished::Verified(count)
                }
                Action::Search => {
                    let Some(folder) = files::select(Choice::Directory, &base, "")? else {
                        return Ok(Completed::Cancelled);
                    };
                    let rows = veylvik_io::relink::scan(&base, &missing, &folder, |n| {
                        update(format!("Searching · {n} entries checked"))
                    })
                    .map_err(|e| e.to_string())?;
                    Finished::Located {
                        session,
                        revision,
                        rows,
                    }
                }
                Action::Locate(asset) => {
                    let Some(path) = files::select(Choice::Source, &base, "")? else {
                        return Ok(Completed::Cancelled);
                    };
                    let mut rows = rows;
                    rows.retain(|r| r.asset != asset);
                    rows.push(veylvik_io::relink::Proposal {
                        asset,
                        candidates: vec![veylvik_io::relink::Candidate {
                            path,
                            verified: false,
                        }],
                        selected: Some(0),
                    });
                    Finished::Located {
                        session,
                        revision,
                        rows,
                    }
                }
                Action::Apply => {
                    let mapping = rows
                        .iter()
                        .filter_map(|r| {
                            r.selected
                                .and_then(|i| r.candidates.get(i))
                                .map(|c| (r.asset.clone(), c.path.clone()))
                        })
                        .collect();
                    let (commands, images) =
                        prepare_relink(&document, &mapping, source_budget, || {
                            update("Validating selected sources…".into())
                        })?;
                    Finished::Linked {
                        session,
                        revision,
                        commands,
                        images,
                    }
                }
            };
            Ok(Completed::Portable(Box::new(done)))
        });
        if result.is_err() {
            self.portable.work = None;
        }
        self.file_started(result);
    }
    pub(super) fn portable_finished(&mut self, result: Finished) {
        self.portable.open = true;
        match result {
            Finished::Collected(report) => {
                self.portable.report = format!(
                    "Collection verified: {} files · {:.1} MiB. Open {} to work on the copy.",
                    report.assets,
                    report.bytes as f64 / 1048576.,
                    report.project.display()
                );
                self.status = "Portable collection complete".into();
                self.error = false;
            }
            Finished::Verified(count) => {
                self.portable.report = format!(
                    "Verified {count} files. Hashes match and all file dependencies are inside this collection."
                );
                self.status = "Collection verified".into();
                self.error = false;
            }
            Finished::Located {
                session,
                revision,
                rows,
            } => {
                if self.project.session != session || self.project.document.revision() != revision {
                    self.fail("Project changed during search. Search again for this project.");
                    return;
                }
                self.status = "Review relink candidates".into();
                self.error = false;
                self.portable.rows = rows;
                self.portable.session = Some((session, revision));
                self.portable.report="Review the proposed paths, then relink the selected sources. Ambiguous matches are never chosen automatically.".into();
            }
            Finished::Linked {
                session,
                revision,
                commands,
                images,
            } => {
                if self.project.session != session || self.project.document.revision() != revision {
                    self.fail("Project changed during relink. Search again for this project.");
                    return;
                }
                if let Err(e) = self.project.apply(&commands) {
                    self.fail(e);
                    return;
                }
                for (asset, image) in images {
                    image.insert(&mut self.project.inputs, asset);
                }
                self.portable.rows.clear();
                self.portable.session = None;
                self.refresh_draft();
                self.request_preview();
                self.portable.report =
                    "Selected sources relinked. Undo restores their previous paths.".into();
                self.status = "Sources relinked".into();
                self.error = false;
            }
        }
    }
    pub(super) fn portable_poll_finished(&mut self, error: Option<&str>) {
        if self.portable.work.take().is_some()
            && let Some(error) = error
        {
            self.portable.report = error.into();
            self.portable.open = true;
        }
    }
}
type Relinked = (Vec<Command>, Vec<(String, veylvik_io::footage::Media)>);
fn prepare_relink(
    document: &Composition,
    mapping: &BTreeMap<String, PathBuf>,
    mut budget: u64,
    mut keep_going: impl FnMut() -> bool,
) -> Result<Relinked, String> {
    let mut copy = document.clone();
    let mut images = vec![];
    let mut commands = vec![];
    for (old, path) in mapping {
        if !keep_going() {
            return Err("Relink cancelled".into());
        }
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let next = path.to_str().ok_or("Source path is not UTF-8")?.to_owned();
        if document
            .nodes
            .values()
            .any(|n| n.operation.asset() == Some(old))
        {
            let media = veylvik_io::footage::load(&path, budget.min(128 * 1024 * 1024))
                .map_err(|e| e.to_string())?;
            budget = budget.saturating_sub(media.byte_size());
            images.push((next.clone(), media));
            for node in copy.nodes.values_mut() {
                if node.operation.asset() == Some(old) {
                    *node.operation.asset_mut().unwrap() = next.clone();
                }
            }
        }
        for transform in copy.ocio_transforms_mut() {
            if &transform.config == old {
                transform.config = next.clone();
                let config = veylvik_color::Config::load(&next)?;
                config.processor(
                    &transform.source,
                    &transform.destination,
                    &transform.display,
                    &transform.view,
                )?;
            }
        }
    }
    for (id, node) in &copy.nodes {
        if serde_json::to_value(node).map_err(|e| e.to_string())?
            != serde_json::to_value(&document.nodes[id]).map_err(|e| e.to_string())?
        {
            commands.push(Command::Put {
                node: Box::new(node.clone()),
            });
        }
    }
    if serde_json::to_value(&copy.ocio).map_err(|e| e.to_string())?
        != serde_json::to_value(&document.ocio).map_err(|e| e.to_string())?
    {
        commands.push(Command::OcioViews {
            settings: copy.ocio,
        });
    }
    Ok((commands, images))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_relink_preserves_nodes_and_animation_and_is_one_undo_step() {
        let root = std::env::temp_dir().join(format!("veylvik-relink-edit-{}", NodeId::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let replacement = root.join("found.png");
        veylvik_io::save_png(&replacement, 1, 1, &[[1., 0., 0., 1.]]).unwrap();
        let mut doc = veylvik_document::demo();
        let first = veylvik_document::Node::new(
            "First",
            veylvik_document::Operation::Image(veylvik_document::nodes::Image {
                asset: "missing.png".into(),
            }),
            &[],
        );
        let second = veylvik_document::Node::new("Second", first.operation.clone(), &[]);
        let ids = [first.id, second.id];
        doc.nodes.insert(first.id, first);
        doc.nodes.insert(second.id, second);
        let before = serde_json::to_value(&doc).unwrap();
        let (commands, images) = prepare_relink(
            &doc,
            &BTreeMap::from([("missing.png".into(), replacement.clone())]),
            128 * 1024 * 1024,
            || true,
        )
        .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(commands.len(), 2);
        let mut document = veylvik_document::Document::new(doc.clone()).unwrap();
        document.apply(0, &commands).unwrap();
        assert_eq!(document.history().undo_steps, 1);
        for id in ids {
            assert_eq!(
                document.snapshot().nodes[&id].operation.asset(),
                replacement.to_str()
            );
        }
        document.undo(document.revision()).unwrap();
        assert_eq!(serde_json::to_value(document.snapshot()).unwrap(), before);
        let bad = BTreeMap::from([("missing.png".into(), root.join("absent.png"))]);
        assert!(prepare_relink(&doc, &bad, 128 * 1024 * 1024, || true).is_err());
        assert_eq!(serde_json::to_value(&doc).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }
}
