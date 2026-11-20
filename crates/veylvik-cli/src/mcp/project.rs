//! Project tools of the headless server: start, open and import replace or extend the session's document, refusing to drop unsaved work unless told to.
use super::{Result, Session, tool};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use veylvik_document::{Command, Composition, Document, Node, Operation, Time};

pub(super) fn tools(revision: &Value) -> Vec<Value> {
    let rate = json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":1},"rate":{"type":"integer","minimum":1}},"required":["ticks","rate"],"additionalProperties":false});
    vec![
        tool(
            "new_project",
            "Replace the session's project with a new, empty composition (revision 0). frame_rate is {ticks, rate}, for example {\"ticks\":30,\"rate\":1} for 30 fps (default 24); frame_count defaults to 240. Refuses when the current project has changes since it was opened or saved unless discard_changes is true. Relative media paths then resolve against the server's working directory until the project is saved.",
            json!({"width":{"type":"integer","minimum":2,"maximum":8192},"height":{"type":"integer","minimum":2,"maximum":8192},"frame_rate":rate,"frame_count":{"type":"integer","minimum":1,"maximum":veylvik_document::MAX_FRAME_COUNT},"discard_changes":{"type":"boolean"}}),
            &["width", "height"],
        ),
        tool(
            "open_project",
            "Open a project file in place of the current one (revision 0). Refuses unsaved changes unless discard_changes is true.",
            json!({"path":{"type":"string"},"discard_changes":{"type":"boolean"}}),
            &["path"],
        ),
        tool(
            "new_from_media",
            "Replace the session's project with a composition matched to a media file (its size, frame rate and duration), with the media as the output's Footage node. Refuses unsaved changes unless discard_changes is true.",
            json!({"path":{"type":"string"},"discard_changes":{"type":"boolean"}}),
            &["path"],
        ),
        tool(
            "import_media",
            "Add a video, PNG, OpenEXR or sequence manifest (.veyseq, sequence.json) as a Footage node and return its id and the media's size, rate and duration. match_composition also gives the composition the media's size, frame rate and duration; output true makes the node the output. An audio file (wav, mp3, flac, ogg, opus, m4a, aac, aiff) is added as a Sound node that plays from the first frame (set timing.start to place it; it is mixed into render_video). An SVG drawing is added as vector shapes instead, fitted to 80% of the frame: per shape a Path (its outline: connect it to text_3d to extrude it), its paint (a Solid, or for a gradient a Gradient through a Colorize from its first stop's colour to its last) and a Mask layer, merged back to front; a shape cut by its group's clip path takes its mask through a Combine mask with the clip. The reply lists every node (result is the merged drawing) and left_out counts text, pictures and patterns that were not read. shapes keeps only the shapes with those numbers (1 is the first); scale (0.05 to 1) sets how much of the frame it fills; width and height fit it to a frame of another size, as inside an Own size branch of that size. One undo step.",
            json!({"revision":revision,"path":{"type":"string"},"match_composition":{"type":"boolean"},"output":{"type":"boolean"},
                "shapes":{"type":"array","items":{"type":"integer","minimum":1,"maximum":64},"maxItems":64},
                "scale":{"type":"number","minimum":0.05,"maximum":1.0},
                "width":{"type":"integer","minimum":1,"maximum":16384},"height":{"type":"integer","minimum":1,"maximum":16384}}),
            &["revision", "path"],
        ),
    ]
}
/// Whole frames covering `duration` at `rate`.
fn frames(duration: Time, rate: Time) -> u32 {
    let numerator = i128::from(duration.ticks) * i128::from(rate.ticks);
    let denominator = i128::from(duration.rate) * i128::from(rate.rate);
    ((numerator + denominator - 1) / denominator)
        .clamp(1, i128::from(veylvik_document::MAX_FRAME_COUNT)) as u32
}
fn flag(args: &Value, name: &str) -> bool {
    args.get(name).and_then(Value::as_bool).unwrap_or(false)
}
fn media(args: &Value) -> Result<PathBuf> {
    Ok(Path::new(args["path"].as_str().ok_or("path must be a string")?).canonicalize()?)
}
/// The Footage node for `path` and what the file holds.
fn footage(path: &Path) -> Result<(Node, veylvik_render::SourceInfo)> {
    let loaded = crate::assets::footage::load(path, 128 * 1024 * 1024)?;
    let info = loaded.info().ok_or("media has no size metadata")?;
    let node = Node::new(
        path.file_name().and_then(|s| s.to_str()).unwrap_or("Media"),
        Operation::Footage(veylvik_document::nodes::Footage {
            asset: path.to_str().ok_or("media path is not UTF-8")?.to_owned(),
            timing: Default::default(),
            placement: veylvik_document::media::Placement::Fit,
            colour: Default::default(),
            layer: None,
            fallback: String::new(),
        }),
        &[],
    );
    Ok((node, info))
}
impl Session {
    fn replace(&mut self, composition: Composition, base: PathBuf) -> Result<Value> {
        self.document = Document::new(composition)?;
        self.asset_base = base;
        self.saved_dir = None;
        self.saved = 0;
        Ok(
            json!({"revision":0,"summary":veylvik_document::inspection::summary(self.document.snapshot())}),
        )
    }
    /// Runs a project tool, or returns `None` for any other.
    pub(super) fn project(&mut self, name: &str, args: &Value) -> Result<Option<Value>> {
        if !matches!(
            name,
            "new_project" | "open_project" | "new_from_media" | "import_media"
        ) {
            return Ok(None);
        }
        if name != "import_media"
            && self.document.revision() != self.saved
            && !flag(args, "discard_changes")
        {
            return Err(
                "the project has unsaved changes: save it, or pass discard_changes: true".into(),
            );
        }
        let here = || std::env::current_dir();
        Ok(Some(match name {
            "new_project" => {
                let size = |key: &str| -> Result<u32> {
                    Ok(u32::try_from(
                        args[key]
                            .as_u64()
                            .ok_or(format!("{key} must be an integer"))?,
                    )?)
                };
                let rate = match args.get("frame_rate") {
                    Some(rate) => serde_json::from_value(rate.clone())?,
                    None => Time { ticks: 24, rate: 1 },
                };
                let mut composition =
                    veylvik_document::blank(size("width")?, size("height")?, rate)?;
                if let Some(count) = args.get("frame_count") {
                    composition.frame_count =
                        u32::try_from(count.as_u64().ok_or("frame_count must be an integer")?)?;
                }
                self.replace(composition, here()?)?
            }
            "open_project" => {
                let path = media(args)?;
                self.replace(
                    Composition::load(&path)?,
                    crate::assets::document_base(&path)?,
                )?
            }
            "new_from_media" => {
                let path = media(args)?;
                let (node, info) = footage(&path)?;
                let rate = info.frame_rate.unwrap_or(Time { ticks: 24, rate: 1 });
                let mut composition = veylvik_document::blank(info.width, info.height, rate)?;
                if let Some(duration) = info.duration {
                    composition.frame_count = frames(duration, rate);
                }
                composition.nodes.clear();
                composition.output = node.id;
                let id = node.id;
                composition.nodes.insert(id, node);
                let mut reply = self.replace(composition, here()?)?;
                reply["node"] = json!(id);
                reply["media"] = json!(info.description);
                reply
            }
            _ if crate::assets::export_audio::is_audio(&media(args)?) => {
                let path = media(args)?;
                let probe = crate::assets::export_audio::probe(&path)?;
                if !probe.has_audio {
                    return Err(format!("{} has no sound", path.display()).into());
                }
                let node = Node::new(
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("Sound"),
                    Operation::Sound(veylvik_document::nodes::Sound {
                        file: path.to_str().ok_or("path is not UTF-8")?.to_owned(),
                        timing: Default::default(),
                        gain: 0.0.into(),
                    }),
                    &[],
                );
                let id = node.id;
                let revision = self.document.apply(
                    self.document.revision(),
                    &[Command::Put {
                        node: Box::new(node),
                    }],
                )?;
                json!({"revision":revision,"node":id,"type":"sound","duration_seconds":probe.duration})
            }
            _ if crate::assets::svg::is_svg(&media(args)?) => {
                let path = media(args)?;
                let doc = self.document.snapshot();
                // Fitted to the composition, or to a frame of another size (as an Own
                // size branch has), filling `scale` of it.
                let side =
                    |key: &str, default: u32| -> Result<u32> {
                        match args.get(key) {
                            None => Ok(default),
                            Some(value) => Ok(u32::try_from(
                                value.as_u64().filter(|v| (1..=16384).contains(v)).ok_or(
                                    format!("{key} must be a whole number of pixels, 1 to 16384"),
                                )?,
                            )?),
                        }
                    };
                let scale = match args.get("scale") {
                    None => 0.8,
                    Some(value) => value
                        .as_f64()
                        .filter(|v| (0.05..=1.0).contains(v))
                        .ok_or("scale must be 0.05 to 1")?
                        as f32,
                };
                let (mut shapes, skipped) = crate::assets::svg::import(
                    &path,
                    side("width", doc.width)?,
                    side("height", doc.height)?,
                    scale,
                )?;
                if let Some(only) = args.get("shapes") {
                    let only: Vec<usize> = only
                        .as_array()
                        .ok_or("shapes must be a list of shape numbers")?
                        .iter()
                        .map(|n| n.as_u64().map(|n| n as usize))
                        .collect::<Option<_>>()
                        .ok_or("shapes must be a list of shape numbers")?;
                    shapes = crate::assets::svg::keep(shapes, &only);
                    if shapes.is_empty() {
                        return Err("none of those shape numbers is in the drawing".into());
                    }
                }
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Drawing".into());
                let (mut commands, mut report) = crate::assets::svg::commands(doc, &name, &shapes);
                if flag(args, "output") {
                    let id = report["result"].as_str().unwrap_or_default().parse()?;
                    commands.push(Command::Output { id });
                }
                report["revision"] =
                    json!(self.document.apply(self.document.revision(), &commands)?);
                report["left_out"] = json!(skipped);
                report
            }
            _ => {
                let path = media(args)?;
                let (node, info) = footage(&path)?;
                let id = node.id;
                let mut commands = Vec::new();
                if flag(args, "match_composition") {
                    let rate = info
                        .frame_rate
                        .unwrap_or(self.document.snapshot().frame_rate);
                    commands.push(Command::Resize {
                        width: info.width,
                        height: info.height,
                    });
                    commands.push(Command::FrameRate { frame_rate: rate });
                    if let Some(duration) = info.duration {
                        commands.push(Command::FrameCount {
                            frame_count: frames(duration, rate),
                        });
                    }
                }
                commands.push(Command::Put {
                    node: Box::new(node),
                });
                if flag(args, "output") {
                    commands.push(Command::Output { id });
                }
                let revision = self.document.apply(self.document.revision(), &commands)?;
                json!({"revision":revision,"node":id,"width":info.width,"height":info.height,"frame_rate":info.frame_rate,"duration":info.duration,"frames":info.frames,"media":info.description})
            }
        }))
    }
}
