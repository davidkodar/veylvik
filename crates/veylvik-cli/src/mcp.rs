//! MCP stdio adapter. Editing uses the same document transactions as other clients.
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use veylvik_document::{Command, Composition, Document, Time};
use veylvik_render::{Plan, Region, RenderRequest, Renderer};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Session {
    document: Document,
    asset_base: std::path::PathBuf,
    renderer: Option<Renderer>,
}
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
fn tools() -> &'static Value {
    static TOOLS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    TOOLS.get_or_init(build_tools)
}
fn build_tools() -> Value {
    let revision = json!({"type":"integer","minimum":0});
    json!({"tools":[
        tool("inspect","Read project state and revision. Set summary=true for compact composition metadata without node payloads; graph and get_node provide targeted inspection.",json!({"summary":{"type":"boolean"}}),&[]),
        tool("graph","Page node metadata in stable UUID order without parameter payloads. Labels over 256 characters are marked as truncated. Compare revisions between pages.",json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":128}}),&[]),
        tool("get_node","Read one exact authored node, or just a named parameter. Does not evaluate curves or change selection.",json!({"id":{"type":"string","format":"uuid"},"parameter":{"type":"string"}}),&["id"]),
        tool("diagnostics","Read existing renderer buffer and cache counters without initializing a GPU. Snapshot is null until rendering initializes the device. Declared buffer accounting excludes driver and rasterizer scratch memory.",json!({}),&[]),
        tool("nodes","List supported node operations, ports and example payloads.",json!({}),&[]),
        tool("outline_text","Read an explicit font and convert explicitly separated directional/script text lines into portable Path mask geometry. Does not modify the document. Apply the returned operation with a put command. No fallback, automatic wrapping or mixed-direction paragraph layout.",json!({"revision":revision,"font":{"type":"string"},"text":{"type":"string","maxLength":4096},"alignment":{"enum":["left","center","right"]},"line_height":{"type":"number","minimum":1,"maximum":4096},"font_index":{"type":"integer","minimum":0,"maximum":u32::MAX},"font_size":{"type":"number","minimum":1,"maximum":1024},"origin":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-32768,"maximum":32768}},"direction":{"enum":["left_to_right","right_to_left"]},"language":{"type":"string","minLength":1,"maxLength":64}}),&["revision","font","text"]),
        tool("materials","Every material that apply_material accepts: family surfaces (source family: changes the surface kind and keeps the target's finish and lighting), catalogue looks and saved materials (whole looks), with label, category, description and whether the user starred it. Saved ids are saved:<name>.",json!({}),&[]),
        tool("apply_material","Put a material from materials on a Text node (adding or restyling its Text surface, exactly like the library) or restyle a Text surface node in place. One undo step; output and wiring stay. Dry-run validates only.",json!({"revision":revision,"target":{"type":"string","format":"uuid"},"material":{"type":"string","minLength":1,"maxLength":256},"dry_run":{"type":"boolean"}}),&["revision","target","material"]),
        tool("get_preset","Read a bundled starter, or capture a group with its current values and animation. Supply exactly one of starter or group. No filesystem access.",json!({"group":{"type":"string","format":"uuid"},"starter":{"type":"string","enum":veylvik_document::presets::builtin::ALL.map(|p|p.id).as_slice()}}),&[]),
        tool("insert_preset","Insert an independent instance with explicitly bound inputs. One undo step; output remains unchanged. Dry-run IDs are provisional.",json!({"revision":revision,"preset":veylvik_document::schema::preset(),"inputs":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}},"sizing":{"type":"string","enum":["fit","fill","original"],"default":"fit"},"position":veylvik_document::schema::position(),"dry_run":{"type":"boolean"}}),&["revision","preset","inputs"]),
        tool("apply","Validate or commit an atomic batch. dry_run returns the candidate without changing history. Use nodes for payload examples. A put command inserts or replaces a complete node; set_parameter changes one registered parameter, and set_key inserts or updates a scalar key.",json!({"revision":revision,"dry_run":{"type":"boolean"},"commands":{"type":"array","minItems":1,"maxItems":4096,"items":veylvik_document::schema::command()}}),&["revision","commands"]),
        tool("add_effect","Create an editable effect group with exposed controls and ordinary internal nodes as one transaction. Returns root and named role IDs; output is unchanged and layout is automatic. Discover recipes with nodes. Dry-run IDs are provisional.",json!({"revision":revision,"input":{"type":"string","format":"uuid"},"recipe":{"type":"string","enum":veylvik_document::recipes::Recipe::ALL},"dry_run":{"type":"boolean"}}),&["revision","input","recipe"]),
        tool("duplicate_branch","Copy a node and all upstream dependencies as one transaction. Shared ancestors are copied once; returns the copied root and old-to-new ID mapping. Output and assets stay unchanged. Stored layout positions are preserved. Dry-run IDs are provisional and are not reused on commit.",json!({"revision":revision,"node":{"type":"string","format":"uuid"},"dry_run":{"type":"boolean"}}),&["revision","node"]),
        tool("track_surface","Track one rigid planar surface on the GPU, exactly like Studio's Track surface, and add the result as a reusable Planar track node (one undo step; output unchanged). surface is four [x,y] corners in 0..1 composition coordinates, in perimeter order, on the start (reference) frame; end may precede start to track backward. Optional region is a separate, larger area of the same plane to analyze (like a tracking area separate from the corner pin), so the corners can be small, blank or leave the frame. area_follows=screen keeps that area fixed in the frame for floors, roads or sand that keep filling the same part of the shot, so re-anchoring keeps finding fresh texture during pans and walks. Optional occlusion is a node (e.g. an animated path mask) whose alpha marks foreground objects such as people crossing the surface; matches under it are ignored on every frame. motion constrains how the surface may move; auto uses perspective only when the matches clearly need it, which keeps distant or off-screen corners steady. refine (default true) aligns every pixel of the area after feature matching for sub-pixel stability. back_to also tracks backward from start to that earlier frame, so a clear middle frame can anchor a whole shot. Re-anchoring on the latest good frame is on by default so walking or zooming shots last longer. Connect the returned node to the track input of planar_warp nodes with apply. Returns frames tracked, first/last frame and why it stopped.",json!({"revision":revision,"source":{"type":"string","format":"uuid"},"surface":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}},"region":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"feature_radius":{"type":"integer","minimum":3,"maximum":16},"search_radius":{"type":"integer","minimum":1,"maximum":48},"tolerance":{"type":"number","minimum":0.1,"maximum":32},"reanchor":{"type":"boolean"},"area_follows":{"enum":["surface","screen"],"default":"surface"},"occlusion":{"type":"string","format":"uuid"},"motion":{"enum":["auto","perspective","affine","similarity","translation"],"default":"auto"},"refine":{"type":"boolean","default":true},"back_to":{"type":"integer","minimum":0}}),&["revision","source","surface","start","end"]),
        tool("solve_camera","Solve the real camera of a filmed shot on the GPU so 3D sits in it: hundreds of features are followed through the footage, then the camera's path, rotation and lens are solved (a tripod pan is recognised and solved as rotation only). Adds, as one undo step: a Solved camera keyed on every frame, a Ground (shadow catcher) plane levelled with the floor when one is found, a Solved scene (merge 3D objects into it), a Render solved scene, Footage grain (Match grain measured from the footage, so the CG gets its grain) and 3D over footage (that merged over the source). The output is unchanged. source is the footage node; start/end default to the whole composition; focal_length (mm) is found when omitted; sensor_width defaults to 36 mm. Units match 2D pixels at the footage's median depth. Returns frames solved, tracks, rms_pixels (under 0.5 is good), focal_length, lens_measured (false when a tripod shot barely turns and 36 mm-equivalent was assumed: pass focal_length then), tripod, the ground and the new node ids.",json!({"revision":revision,"source":{"type":"string","format":"uuid"},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"focal_length":{"type":"number","minimum":1,"maximum":2000},"sensor_width":{"type":"number","minimum":1,"maximum":100}}),&["revision","source"]),
        tool("segment_subject","Cut out a person or object and track it through the shot with a local AI model, like Studio's Select subject. points are clicks in 0..1 composition coordinates on frame (the prompted frame); positive=true marks the subject, false excludes an area; several points spread over a person (head, body, legs, anything carried) select them reliably. Tracks from frame to end (either direction; end defaults to the last composition frame), and with back_to also backward to that earlier frame. model picks the tracker: edgetam (fastest), sam2.1-base-plus or sam2.1-large (more precise and consistent); it must be installed with `veylvik models install ID`. detail (default true) adds a zoomed second pass for feet, hands and hair; matting (default: when ViTMatte is installed) refines hair and soft edges. Writes a 16-bit matte sequence beside the project and adds a Subject matte Footage node and a Subject cutout (Refine matte) node in one undo step; output unchanged. Merge the cutout over a new background, or the shot behind it. Returns the nodes, frame range, frames where the subject was absent, zoom and timing.",json!({"revision":revision,"source":{"type":"string","format":"uuid"},"points":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","additionalProperties":false,"properties":{"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1},"positive":{"type":"boolean"}},"required":["x","y","positive"]}},"frame":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"back_to":{"type":"integer","minimum":0},"model":{"enum":["edgetam","sam2.1-base-plus","sam2.1-large"],"default":"edgetam"},"detail":{"type":"boolean","default":true},"matting":{"type":"boolean"}}),&["revision","source","points","frame"]),
        tool("undo","Undo one transaction, with a revision precondition.",json!({"revision":revision}),&["revision"]),
        tool("redo","Redo one transaction, with a revision precondition.",json!({"revision":revision}),&["revision"]),
        tool("save","Atomically save the current composition to an explicit path.",json!({"path":{"type":"string"},"revision":revision}),&["path","revision"]),
        tool("render_video","Export composition output to a new MP4 using hardware-first encoding. Defaults to composition size/rate/range, H.264 high quality, black matte, hardware required. Optional settings replaces the entire settings object. Audio: the sound of every Footage node feeding the output is mixed in at its timeline position and trims (AAC 48 kHz); settings.audio may set mute, music (an extra file starting at the first exported frame) and music_gain_db (-60..12). The result lists the audio sources. Video is SDR Rec.709; Vulkan resizes and converts, compatible NVIDIA devices transfer planes directly on GPU to in-process NVENC; AMD/Intel can trial experimental DMA-BUF to VA-API; unsupported paths explicitly report RAM staging. Never overwrites an existing file.",json!({"path":{"type":"string"},"revision":revision,"settings":{"type":"object","additionalProperties":false,"properties":{"width":{"type":"integer","minimum":2,"maximum":8192},"height":{"type":"integer","minimum":2,"maximum":8192},"frame_rate":{"type":"object","properties":{"ticks":{"type":"integer","minimum":1},"rate":{"type":"integer","minimum":1}},"required":["ticks","rate"],"additionalProperties":false},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"codec":{"enum":["h264","hevc10"]},"quality":{"enum":["draft","high","master"]},"allow_software":{"type":"boolean"},"matte":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":0,"maximum":1}}},"required":["width","height","frame_rate","start","end"]}}),&["path","revision"]),
        tool("render_sequence","Render an inclusive frame range at the composition frame rate into a new directory. Choose png (default SDR) or exr (linear HDR); frames use zero-based names; sequence.json records timing and completion. Existing directories are rejected; at most 10000 frames.",json!({"path":{"type":"string"},"revision":revision,"start":{"type":"integer"},"end":{"type":"integer"},"format":{"enum":["png","exr"]},"node":{"type":"string","format":"uuid"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["path","revision","start","end"]),
        tool("preview_image","Render an inline PNG image for visual inspection without writing a file. max_edge defaults to 512 (1..1024); area filtering uses linear premultiplied color. This bounds the returned image, not GPU render resolution. HDR is clipped to SDR and PNG preserves alpha.",json!({"max_edge":{"type":"integer","minimum":1,"maximum":1024},"revision":revision,"ticks":{"type":"integer"},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX},"node":{"type":"string","format":"uuid"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["revision","ticks","rate"]),
        tool("render","Render the current revision using Vulkan. Choose .png for SDR or .exr for lossless linear premultiplied RGBA32F. Time is exact ticks/rate seconds.",json!({"path":{"type":"string"},"revision":revision,"ticks":{"type":"integer"},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX},"node":{"type":"string","format":"uuid"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["path","revision","ticks","rate"])
    ]})
}
fn required_u64(args: &Value, name: &str) -> Result<u64> {
    args[name]
        .as_u64()
        .ok_or_else(|| format!("{name} must be an unsigned integer").into())
}
fn path(args: &Value) -> Result<&std::path::Path> {
    Ok(std::path::Path::new(
        args["path"].as_str().ok_or("path must be a string")?,
    ))
}
impl Session {
    fn validate_call(&self, name: &str, args: &Value) -> Result<()> {
        let catalog = tools();
        let descriptor = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .ok_or("unknown tool")?;
        let object = args.as_object().ok_or("arguments must be an object")?;
        for key in object.keys() {
            if descriptor["inputSchema"]["properties"].get(key).is_none() {
                return Err(format!("unknown argument {key}").into());
            }
        }
        for key in descriptor["inputSchema"]["required"].as_array().unwrap() {
            if !object.contains_key(key.as_str().unwrap()) {
                return Err(format!("missing argument {key}").into());
            }
        }
        if !matches!(
            name,
            "inspect" | "nodes" | "diagnostics" | "graph" | "get_node" | "get_preset" | "materials"
        ) && required_u64(args, "revision")? != self.document.revision()
        {
            return Err(format!("revision conflict: current {}", self.document.revision()).into());
        }
        Ok(())
    }
    fn call_output(&mut self, name: &str, args: &Value) -> Result<veylvik_control::ToolOutput> {
        if name != "preview_image" {
            return self.call(name, args).map(Into::into);
        }
        self.validate_call(name, args)?;
        let max_edge = args
            .get("max_edge")
            .map(|value| value.as_u64().ok_or("max_edge must be an integer"))
            .transpose()?
            .unwrap_or(512);
        if !(1..=veylvik_io::preview::MAX_PREVIEW_EDGE as u64).contains(&max_edge) {
            return Err("max_edge must be within 1..1024".into());
        }
        let (node, time, frame) = self.frame(args, true)?;
        let pixels = veylvik_render::display::apply(
            if self.document.snapshot().ocio.view.is_some() {
                veylvik_document::media::DisplayTransform::Standard
            } else {
                self.document.snapshot().display
            },
            &frame.pixels,
        );
        let thumbnail = veylvik_io::preview::thumbnail(
            frame.region.width,
            frame.region.height,
            &pixels,
            max_edge as u32,
        )?;
        let metadata = json!({"revision":self.document.revision(),"node":node,"time":time,"width":thumbnail.width,"height":thumbnail.height,"source_region":frame.region,"encoding":"sRGB straight-alpha PNG","filter":"linear-premultiplied area"});
        Ok(veylvik_control::ToolOutput::png(metadata, &thumbnail.png))
    }
    fn frame(
        &mut self,
        args: &Value,
        preview: bool,
    ) -> Result<(
        veylvik_document::NodeId,
        Time,
        veylvik_render::RenderedFrame,
    )> {
        let time = Time::new(
            args["ticks"].as_i64().ok_or("ticks must be an integer")?,
            u32::try_from(required_u64(args, "rate")?)?,
        )?;
        let doc = self.document.snapshot();
        let output = match args.get("node") {
            None => doc.output,
            Some(value) => value
                .as_str()
                .ok_or("node must be a UUID string")?
                .parse()?,
        };
        let region: Option<Region> = args
            .get("region")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()?;
        if let Some(region) = region {
            region.validate(doc.width, doc.height)?;
        }
        let plan = Plan::compile_output(doc, output)?;
        if self.renderer.is_none() {
            self.renderer = Some(crate::assets::runtime::renderer()?);
        }
        let renderer = self.renderer.as_ref().unwrap();
        let inputs = crate::assets::load_inputs(&plan, &self.asset_base, renderer.budget_bytes)?;
        let frame = if preview {
            let frame = renderer.render_gpu_cached(
                &plan,
                RenderRequest {
                    time,
                    region,
                    scene_view: None,
                },
                &inputs,
            )?;
            renderer.read_frame(renderer.apply_ocio_view(&plan, &inputs, frame)?.as_ref())?
        } else {
            renderer.render_frame_export(
                &plan,
                RenderRequest {
                    time,
                    region,
                    scene_view: None,
                },
                &inputs,
            )?
        };
        Ok((output, time, frame))
    }
    fn call(&mut self, name: &str, args: &Value) -> Result<Value> {
        self.validate_call(name, args)?;
        match name {
            "materials" => veylvik_io::materials::library(&veylvik_io::materials::directory()?),
            "apply_material" => {
                let target = args["target"]
                    .as_str()
                    .ok_or("target must be a UUID")?
                    .parse()?;
                let commands = veylvik_io::materials::apply(
                    self.document.snapshot(),
                    target,
                    args["material"]
                        .as_str()
                        .ok_or("material must be a string")?,
                    &veylvik_io::materials::directory()?,
                )?;
                let dry_run = args
                    .get("dry_run")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if dry_run {
                    self.document.prepare(self.document.revision(), &commands)?;
                } else {
                    self.document.apply(self.document.revision(), &commands)?;
                }
                Ok(
                    json!({"revision":self.document.revision(),"dry_run":dry_run,"changed_nodes":commands.len()}),
                )
            }
            "get_preset" => {
                let preset = veylvik_document::presets::read(
                    self.document.snapshot(),
                    args.get("group"),
                    args.get("starter"),
                )?;
                Ok(json!({"revision":self.document.revision(),"preset":preset}))
            }
            "insert_preset" => {
                let preset: veylvik_document::presets::Preset =
                    serde_json::from_value(args["preset"].clone())?;
                let inputs = serde_json::from_value(args["inputs"].clone())?;
                let position = args
                    .get("position")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()?
                    .flatten();
                let dry_run = args
                    .get("dry_run")
                    .map(|v| v.as_bool().ok_or("dry_run must be boolean"))
                    .transpose()?
                    .unwrap_or(false);
                let instance = preset.instantiate_sized(
                    self.document.snapshot(),
                    &inputs,
                    position,
                    args.get("sizing")
                        .map(|v| serde_json::from_value(v.clone()))
                        .transpose()?
                        .unwrap_or_default(),
                )?;
                if !dry_run {
                    self.document
                        .apply(self.document.revision(), &instance.commands)?;
                }
                Ok(
                    json!({"revision":self.document.revision(),"group":instance.group,"root":instance.output,"dry_run":dry_run,"valid":true}),
                )
            }
            "inspect" => {
                let summary = args
                    .get("summary")
                    .map(|value| value.as_bool().ok_or("summary must be boolean"))
                    .transpose()?
                    .unwrap_or(false);
                let mut result =
                    json!({"revision":self.document.revision(),"history":self.document.history()});
                if summary {
                    result["summary"] =
                        veylvik_document::inspection::summary(self.document.snapshot());
                } else {
                    result["composition"] = serde_json::to_value(self.document.snapshot())?;
                }
                Ok(result)
            }
            "graph" | "get_node" => {
                let mut result = read_graph(self.document.snapshot(), name, args)?;
                result["revision"] = json!(self.document.revision());
                Ok(result)
            }
            "diagnostics" => match &self.renderer {
                Some(renderer) => Ok(
                    json!({"initialized":true,"adapter":renderer.adapter.name,"snapshot":renderer.try_diagnostics()?}),
                ),
                None => Ok(json!({"initialized":false,"adapter":null,"snapshot":null})),
            },
            "nodes" => Ok(veylvik_document::registry::catalog()),
            "outline_text" => {
                let mut payload = args.clone();
                payload.as_object_mut().unwrap().remove("revision");
                let request: crate::assets::text::OutlineRequest = serde_json::from_value(payload)?;
                let outlined = crate::assets::text::outline(&self.asset_base, &request)?;
                Ok(
                    json!({"revision":self.document.revision(),"operation":{"type":"path","geometry":outlined.geometry},"glyph_count":outlined.glyph_count,"advance":outlined.advance,"line_count":outlined.line_count}),
                )
            }
            "add_effect" => {
                let input = args["input"]
                    .as_str()
                    .ok_or("input must be a UUID")?
                    .parse()?;
                let recipe: veylvik_document::recipes::Recipe =
                    serde_json::from_value(args["recipe"].clone())?;
                let dry_run = args
                    .get("dry_run")
                    .map(|value| value.as_bool().ok_or("dry_run must be boolean"))
                    .transpose()?
                    .unwrap_or(false);
                let effect = recipe.build(self.document.snapshot(), input)?;
                let mut result = json!({"root":effect.root,"roles":effect.roles,"added_nodes":effect.nodes.len(),"recipe":recipe,"dry_run":dry_run,"valid":true});
                let commands = effect.into_grouped_commands(self.document.snapshot())?;
                if dry_run {
                    self.document.prepare(self.document.revision(), &commands)?;
                } else {
                    self.document.apply(self.document.revision(), &commands)?;
                }
                result["revision"] = json!(self.document.revision());
                Ok(result)
            }
            "track_surface" => {
                let request = crate::track::Request::from_json(args)?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                let (node, commands, mut report) = crate::track::run(
                    self.renderer.as_ref().unwrap(),
                    self.document.snapshot(),
                    &self.asset_base,
                    &request,
                )?;
                self.document.apply(self.document.revision(), &commands)?;
                report["node"] = json!(node);
                report["revision"] = json!(self.document.revision());
                Ok(report)
            }
            "solve_camera" => {
                let request =
                    crate::solve::Request::from_json(args, self.document.snapshot().frame_count)?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                let (commands, mut report) = crate::solve::run(
                    self.renderer.as_ref().unwrap(),
                    self.document.snapshot(),
                    &self.asset_base,
                    &request,
                )?;
                self.document.apply(self.document.revision(), &commands)?;
                report["revision"] = json!(self.document.revision());
                Ok(report)
            }
            "segment_subject" => {
                let request =
                    crate::segment::Request::from_json(args, self.document.snapshot().frame_count)?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                let (node, commands, mut report) = crate::segment::run(
                    self.renderer.as_ref().unwrap(),
                    self.document.snapshot(),
                    &self.asset_base,
                    &self.asset_base,
                    &request,
                )?;
                self.document.apply(self.document.revision(), &commands)?;
                report["node"] = json!(node);
                report["revision"] = json!(self.document.revision());
                Ok(report)
            }
            "duplicate_branch" => {
                let root = args["node"]
                    .as_str()
                    .ok_or("node must be a UUID")?
                    .parse()?;
                let dry_run = args
                    .get("dry_run")
                    .map(|value| value.as_bool().ok_or("dry_run must be boolean"))
                    .transpose()?
                    .unwrap_or(false);
                let copy = veylvik_document::editing::copy_branch(self.document.snapshot(), root)?;
                let mut result = json!({"root":copy.root,"mapping":copy.mapping,"copied_nodes":copy.nodes.len(),"dry_run":dry_run});
                let commands = copy.into_commands();
                if dry_run {
                    self.document.prepare(self.document.revision(), &commands)?;
                } else {
                    self.document.apply(self.document.revision(), &commands)?;
                }
                result["revision"] = json!(self.document.revision());
                result["valid"] = json!(true);
                Ok(result)
            }
            "apply" => {
                let commands: Vec<Command> = serde_json::from_value(args["commands"].clone())?;
                let dry_run = match args.get("dry_run") {
                    None => false,
                    Some(v) => v.as_bool().ok_or("dry_run must be boolean")?,
                };
                if dry_run {
                    let candidate = self.document.prepare(self.document.revision(), &commands)?;
                    Ok(
                        json!({"valid":true,"revision":self.document.revision(),"candidate":candidate}),
                    )
                } else {
                    Ok(json!({"revision":self.document.apply(self.document.revision(),&commands)?}))
                }
            }
            "undo" => Ok(json!({"revision":self.document.undo(self.document.revision())?})),
            "redo" => Ok(json!({"revision":self.document.redo(self.document.revision())?})),
            "save" => {
                crate::assets::save(self.document.snapshot(), &self.asset_base, path(args)?)?;
                Ok(json!({"saved":args["path"],"revision":self.document.revision()}))
            }
            "render_video" => {
                let doc = self.document.snapshot();
                let mut settings: crate::assets::video_export::Settings = match args.get("settings")
                {
                    Some(value) => serde_json::from_value(value.clone())?,
                    None => crate::assets::video_export::Settings::for_document(doc),
                };
                settings.audio.base = Some(self.asset_base.clone());
                settings.times(doc)?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                let renderer = self.renderer.as_ref().unwrap();
                let plan = Plan::compile(doc)?;
                let inputs =
                    crate::assets::load_inputs(&plan, &self.asset_base, renderer.budget_bytes)?;
                let outcome = crate::assets::video_export::render(
                    renderer,
                    doc,
                    &inputs,
                    path(args)?,
                    &settings,
                    |_| true,
                )?;
                let mut result = serde_json::to_value(outcome)?;
                result["revision"] = json!(self.document.revision());
                Ok(result)
            }
            "render_sequence" => {
                let format = serde_json::from_value::<crate::assets::FrameFormat>(
                    args.get("format").cloned().unwrap_or(json!("png")),
                )?;
                let doc = self.document.snapshot();
                let frames = veylvik_document::sequence::FrameRange::new(
                    args["start"].as_i64().ok_or("start must be an integer")?,
                    args["end"].as_i64().ok_or("end must be an integer")?,
                    doc.frame_rate,
                )?;
                let output = args
                    .get("node")
                    .map(|value| value.as_str().ok_or("node must be a UUID string"))
                    .transpose()?
                    .map(str::parse)
                    .transpose()?
                    .unwrap_or(doc.output);
                let region: Option<Region> = args
                    .get("region")
                    .map(|value| serde_json::from_value(value.clone()))
                    .transpose()?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                let mut result = crate::assets::sequence::render_sequence(
                    self.renderer.as_ref().unwrap(),
                    doc,
                    &self.asset_base,
                    path(args)?,
                    crate::assets::sequence::SequenceRequest {
                        format,
                        frames,
                        output,
                        region,
                    },
                    |_, _| true,
                )?;
                result["revision"] = json!(self.document.revision());
                Ok(result)
            }
            "render" => {
                crate::assets::FrameFormat::from_path(path(args)?)?;
                crate::assets::protect_inputs(
                    self.document.snapshot(),
                    &self.asset_base,
                    path(args)?,
                )?;
                let (_, _, frame) = self.frame(args, false)?;
                let display = self.document.snapshot().output_display();
                let pixels = crate::assets::export::sdr_pixels(path(args)?, &frame.pixels, display);
                crate::save_frame(
                    path(args)?,
                    frame.region.width,
                    frame.region.height,
                    &pixels,
                )?;
                Ok(
                    json!({"output":args["path"],"revision":self.document.revision(),"region":frame.region}),
                )
            }
            _ => Err("unknown tool".into()),
        }
    }
}

pub fn serve(
    composition: Composition,
    asset_base: std::path::PathBuf,
    input: impl BufRead,
    output: impl Write,
) -> Result<()> {
    let mut session = Session {
        document: Document::new(composition)?,
        asset_base,
        renderer: None,
    };
    veylvik_control::serve_with_output(input, output, tools(), |name, args| {
        session.call_output(name, args).map_err(|e| e.to_string())
    })
}

fn read_graph(document: &veylvik_document::Composition, name: &str, args: &Value) -> Result<Value> {
    if name == "graph" {
        let offset = args
            .get("offset")
            .map(|value| value.as_u64().ok_or("offset must be an unsigned integer"))
            .transpose()?
            .unwrap_or(0);
        let limit = args
            .get("limit")
            .map(|value| value.as_u64().ok_or("limit must be an unsigned integer"))
            .transpose()?
            .unwrap_or(32);
        Ok(veylvik_document::inspection::page(document, offset, limit)?)
    } else {
        let id = args["id"].as_str().ok_or("id must be a UUID")?.parse()?;
        let parameter = args
            .get("parameter")
            .map(|value| value.as_str().ok_or("parameter must be a string"))
            .transpose()?;
        Ok(veylvik_document::inspection::node(document, id, parameter)?)
    }
}
#[cfg(test)]
mod tests;
