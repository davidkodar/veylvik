//! MCP stdio adapter. Editing uses the same document transactions as other clients.
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use veylvik_document::{Command, Composition, Document, Time};
use veylvik_render::{Plan, Region, RenderRequest, Renderer};
mod project;
mod shot;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Session {
    document: Document,
    /// The revision last opened or saved: anything else is unsaved work.
    saved: u64,
    asset_base: std::path::PathBuf,
    /// The folder the project was last saved in, when that is not `asset_base`: files
    /// made for the project (subject mattes) belong beside it.
    saved_dir: Option<std::path::PathBuf>,
    renderer: Option<Renderer>,
}
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
fn tools() -> &'static Value {
    static TOOLS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    TOOLS.get_or_init(build_tools)
}
/// A request's own argument schema, with the document's `revision` beside it.
fn with_revision(mut properties: Value, revision: &Value) -> Value {
    properties["revision"] = revision.clone();
    properties
}
fn build_tools() -> Value {
    let revision = json!({"type":"integer","minimum":0});
    let mut list = json!({"tools":[
        tool("inspect","Read project state and revision. Set summary=true for compact composition metadata without node payloads; graph and get_node provide targeted inspection.",json!({"summary":{"type":"boolean"}}),&[]),
        tool("diagnostics","Read existing renderer buffer and cache counters without initializing a GPU. Snapshot is null until rendering initializes the device. Declared buffer accounting excludes driver and rasterizer scratch memory.",json!({}),&[]),
        tool("outline_text","Read an explicit font and convert explicitly separated directional/script text lines into portable Path mask geometry. Does not modify the document. Apply the returned operation with a put command. No fallback, automatic wrapping or mixed-direction paragraph layout.",json!({"revision":revision,"font":{"type":"string"},"text":{"type":"string","maxLength":4096},"alignment":{"enum":["left","center","right"]},"line_height":{"type":"number","minimum":1,"maximum":4096},"font_index":{"type":"integer","minimum":0,"maximum":u32::MAX},"font_size":{"type":"number","minimum":1,"maximum":1024},"origin":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-32768,"maximum":32768}},"direction":{"enum":["left_to_right","right_to_left"]},"language":{"type":"string","minLength":1,"maxLength":64}}),&["revision","font","text"]),
        tool("track_surface","Track one rigid planar surface on the GPU, exactly like Studio's Track surface, and add the result as a reusable Planar track node (one undo step; output unchanged). surface is four [x,y] corners in 0..1 composition coordinates, in perimeter order, on the start (reference) frame; end may precede start to track backward. Optional region is a separate, larger area of the same plane to analyze (like a tracking area separate from the corner pin), so the corners can be small, blank or leave the frame. area_follows=screen keeps that area fixed in the frame for floors, roads or sand that keep filling the same part of the shot, so re-anchoring keeps finding fresh texture during pans and walks. Optional occlusion is a node (e.g. an animated path mask) whose alpha marks foreground objects such as people crossing the surface; matches under it are ignored on every frame. motion constrains how the surface may move; auto uses perspective only when the matches clearly need it, which keeps distant or off-screen corners steady. refine (default true) aligns every pixel of the area after feature matching for sub-pixel stability. back_to also tracks backward from start to that earlier frame, so a clear middle frame can anchor a whole shot. Re-anchoring on the latest good frame is on by default so walking or zooming shots last longer. Connect the returned node to the track input of planar_warp nodes with apply. Returns frames tracked, first/last frame and why it stopped.",with_revision(veylvik_track::request::Request::schema(), &revision),&[&["revision"][..], veylvik_track::request::Request::REQUIRED].concat()),
        tool("solve_camera","Solve the real camera of a filmed shot on the GPU so 3D sits in it: hundreds of features are followed through the footage, then the camera's path, rotation, lens and its radial distortion are solved (a tripod pan is recognised and solved as rotation only). Adds, as one undo step: a Solved camera keyed on every frame, a Ground (shadow catcher) plane levelled with the floor when one is found, a Solved scene (merge 3D objects into it), a Render solved scene, Footage lens (a lens_distortion node bending the render like the footage, only when the lens distorts), Footage grain (Match grain measured from the footage, so the CG gets its grain) and 3D over footage (that merged over the source). The output is unchanged. source is the footage node; start/end default to the whole composition; focal_length (mm) is found when omitted; sensor_width defaults to 36 mm. Units match 2D pixels at the footage's median depth. Returns frames solved, tracks, rms_pixels (under 0.5 is good), focal_length, lens_measured (false when a tripod shot barely turns and 36 mm-equivalent was assumed: pass focal_length then), tripod, lens_distortion ({k1, k2} or null), the ground and the new node ids.",with_revision(veylvik_track::camera::Request::schema(), &revision),&[&["revision"][..], veylvik_track::camera::Request::REQUIRED].concat()),
        tool("segment_subject","Cut out a person or object and track it through the shot with a local AI model, like Studio's Select subject. points are clicks in 0..1 composition coordinates on frame (the prompted frame); positive=true marks the subject, false excludes an area; several points spread over a person (head, body, legs, anything carried) select them reliably. Tracks from frame to end (either direction; end defaults to the last composition frame), and with back_to also backward to that earlier frame. model picks the tracker: edgetam (fastest), sam2.1-base-plus or sam2.1-large (more precise and consistent); it must be installed with `veylvik models install ID`. detail (default true) adds a zoomed second pass for feet, hands and hair; matting (default: when ViTMatte is installed) refines hair and soft edges. Writes a 16-bit matte sequence beside the project and adds a Subject matte Footage node and a Subject cutout (Refine matte) node in one undo step; output unchanged. Merge the cutout over a new background, or the shot behind it. Returns the nodes, frame range, frames where the subject was absent, zoom and timing.",with_revision(veylvik_segment::request::Request::schema(), &revision),&[&["revision"][..], veylvik_segment::request::Request::REQUIRED].concat()),
        tool("save","Atomically save the current composition to an explicit path.",json!({"path":{"type":"string"},"revision":revision}),&["path","revision"]),
        tool("replace_sound","Give a finished video the composition's sound as it is now, without rendering the picture again: video's picture is copied as it stands into the new MP4 at path, with the sound of every Sound node and of the footage feeding the output mixed for the composition's length (AAC 48 kHz). Use it after changing or adding sound once the picture is right. Both paths are absolute; path must not exist. The result lists the sound sources.",json!({"video":{"type":"string"},"path":{"type":"string"},"revision":revision}),&["video","path","revision"]),
        tool("render_video","Export composition output to a new MP4 (or, with codec prores, a MOV) using hardware-first encoding. Defaults to composition size/rate/range, H.264 high quality, black matte, hardware required. settings names only what differs from those defaults. Audio: the sound of every Footage node feeding the output is mixed in at its timeline position and trims (AAC 48 kHz); settings.audio may set mute, music (an extra file starting at the first exported frame) and music_gain_db (-60..12). The result lists the audio sources. Video is SDR Rec.709; Vulkan resizes and converts, compatible NVIDIA devices transfer planes directly on GPU to in-process NVENC; AMD/Intel can trial experimental DMA-BUF to VA-API; unsupported paths explicitly report RAM staging. An existing file is refused unless settings.replace is true.",json!({"path":{"type":"string"},"revision":revision,"settings":{"type":"object","additionalProperties":false,"properties":{"width":{"type":"integer","minimum":2,"maximum":8192},"height":{"type":"integer","minimum":2,"maximum":8192},"frame_rate":{"type":"object","properties":{"ticks":{"type":"integer","minimum":1},"rate":{"type":"integer","minimum":1}},"required":["ticks","rate"],"additionalProperties":false},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"codec":{"enum":["h264","hevc10","prores","prores4444"],"description":"prores is ProRes 422 HQ in a .mov, for handing the shot to an editor; prores4444 is ProRes 4444 in a .mov with straight alpha, for overlays (slower: frames are read back whole)"},"quality":{"enum":["draft","high","master"]},"allow_software":{"type":"boolean"},"matte":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":0,"maximum":1}},"timecode":{"type":"string","description":"start timecode written into the file, HH:MM:SS:FF"},"replace":{"type":"boolean"},"audio":{"type":"object","additionalProperties":false,"properties":{"mute":{"type":"boolean"},"music":{"type":"string"},"music_gain_db":{"type":"number","minimum":-60,"maximum":12}}}}}}),&["path","revision"]),
        tool("render_sequence","Render an inclusive frame range at the composition frame rate into a new directory. Choose png (default SDR) or exr (linear HDR); frames use zero-based names; sequence.json records timing and completion. Existing directories are rejected; at most 10000 frames.",json!({"path":{"type":"string"},"revision":revision,"start":{"type":"integer"},"end":{"type":"integer"},"format":{"enum":["png","exr"]},"node":{"type":"string","format":"uuid"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["path","revision","start","end"]),
        tool("preview_image","Render an inline PNG image for visual inspection without writing a file. max_edge defaults to 512 (1..2048); region renders part of the frame, so details can be seen at full size; area filtering uses linear premultiplied color. This bounds the returned image, not GPU render resolution. HDR is clipped to SDR and PNG preserves alpha. alpha true shows the picture's alpha as grey instead (a matte check for any node).",json!({"max_edge":{"type":"integer","minimum":1,"maximum":2048},"revision":revision,"frame":{"type":"integer","minimum":0,"description":"a frame number at the composition's rate, instead of ticks and rate"},"ticks":{"type":"integer"},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX},"node":{"type":"string","format":"uuid"},"alpha":{"type":"boolean","description":"show the alpha alone as grey: white is opaque, black transparent"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["revision"]),
        tool("render","Render the current revision using Vulkan. Choose .png for SDR or .exr for lossless linear premultiplied RGBA32F. Time is exact ticks/rate seconds. alpha true writes the picture's alpha as grey instead (a matte check for any node).",json!({"path":{"type":"string"},"revision":revision,"frame":{"type":"integer","minimum":0,"description":"a frame number at the composition's rate, instead of ticks and rate"},"ticks":{"type":"integer"},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX},"node":{"type":"string","format":"uuid"},"alpha":{"type":"boolean","description":"show the alpha alone as grey: white is opaque, black transparent"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["path","revision"])
    ]});
    list["tools"]
        .as_array_mut()
        .unwrap()
        .extend(project::tools(&revision));
    list["tools"]
        .as_array_mut()
        .unwrap()
        .push(shot::descriptor(&revision));
    list["tools"].as_array_mut().unwrap().push(tool(
        "measure",
        "Read numbers back from a rendered frame instead of a picture: displayed brightness percentiles (0..1 after the display transform and the sRGB curve), scene-linear mean, peak and where it is, the share of pixels above 1.0, how much of the frame is covered and the box round everything that is not transparent (in composition pixels). Use it to check exposure, find where a node's picture lands in the frame, or compare two versions. node measures that node's own picture; region measures part of the frame.",
        json!({"revision":revision,"frame":{"type":"integer","minimum":0,"description":"a frame number at the composition's rate, instead of ticks and rate"},"ticks":{"type":"integer"},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX},"node":{"type":"string","format":"uuid"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),
        &["revision"],
    ));
    list["tools"]
        .as_array_mut()
        .unwrap()
        .extend(veylvik_presets::tools::catalogue(
            veylvik_presets::tools::Server::Headless,
        ));
    list
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
        veylvik_presets::tools::check(tools()["tools"].as_array().unwrap(), name, args, &|_| {
            String::new()
        })?;
        if !matches!(
            name,
            "inspect"
                | "nodes"
                | "diagnostics"
                | "graph"
                | "get_node"
                | "get_preset"
                | "materials"
                | "new_project"
                | "open_project"
                | "new_from_media"
        ) && required_u64(args, "revision")? != self.document.revision()
        {
            return Err(format!("revision conflict: current {}", self.document.revision()).into());
        }
        Ok(())
    }
    fn call_output(
        &mut self,
        name: &str,
        args: &Value,
        progress: veylvik_control::Progress<'_>,
    ) -> Result<veylvik_control::ToolOutput> {
        if name != "preview_image" {
            return self.call_reporting(name, args, progress).map(Into::into);
        }
        self.validate_call(name, args)?;
        let max_edge = args
            .get("max_edge")
            .map(|value| value.as_u64().ok_or("max_edge must be an integer"))
            .transpose()?
            .unwrap_or(512);
        if !(1..=veylvik_io::preview::MAX_PREVIEW_EDGE as u64).contains(&max_edge) {
            return Err("max_edge must be within 1..2048".into());
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
        // A time as ticks / rate seconds, or a frame number at the composition's rate (as
        // the live server takes it).
        let time = match args.get("frame") {
            Some(frame) => Time::at_frame(
                frame.as_i64().ok_or("frame must be an integer")?,
                self.document.snapshot().frame_rate,
            )?,
            None => Time::new(
                args["ticks"]
                    .as_i64()
                    .ok_or("give a time: frame, or ticks and rate")?,
                u32::try_from(required_u64(args, "rate")?)?,
            )?,
        };
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
                    scene_reflections: false,
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
                    scene_reflections: false,
                },
                &inputs,
            )?
        };
        // The alpha alone, as opaque grey: what a node keeps and what it lets through.
        let mut frame = frame;
        if args.get("alpha").and_then(Value::as_bool) == Some(true) {
            for pixel in &mut frame.pixels {
                let a = pixel[3].clamp(0.0, 1.0);
                *pixel = [a, a, a, 1.0];
            }
        }
        Ok((output, time, frame))
    }
    #[cfg(test)]
    fn call(&mut self, name: &str, args: &Value) -> Result<Value> {
        self.call_reporting(name, args, &mut |_, _, _| {})
    }
    fn call_reporting(
        &mut self,
        name: &str,
        args: &Value,
        progress: veylvik_control::Progress<'_>,
    ) -> Result<Value> {
        self.validate_call(name, args)?;
        if let Some(reply) = self.project(name, args)? {
            return Ok(reply);
        }
        if name == "match_shot" {
            return self.match_shot(args);
        }
        // The shared document tools: reads, then edits as one undo step each.
        if let Some(mut result) =
            veylvik_presets::tools::read(self.document.snapshot(), name, args)?
        {
            result["revision"] = json!(self.document.revision());
            return Ok(result);
        }
        if name == "materials" {
            return Ok(veylvik_presets::tools::Host::materials(&Files)?);
        }
        let revision = self.document.revision();
        if let Some(edit) = veylvik_presets::tools::edit(
            self.document.snapshot(),
            revision,
            name,
            args,
            &mut Files,
        )? {
            let dry_run = veylvik_presets::tools::dry_run(args)?;
            return Ok(veylvik_presets::tools::commit(
                &mut Opened(&mut self.document),
                edit,
                dry_run,
            )?);
        }
        if name == "measure" {
            let (node, time, frame) = self.frame(args, true)?;
            let doc = self.document.snapshot();
            let display = if doc.ocio.view.is_some() {
                veylvik_document::media::DisplayTransform::Standard
            } else {
                doc.display
            };
            let shown = veylvik_render::display::apply(display, &frame.pixels);
            let region = frame.region;
            let mut result = veylvik_io::measure::measure(
                region.x,
                region.y,
                region.width,
                region.height,
                &frame.pixels,
                &shown,
            )?;
            result["revision"] = json!(self.document.revision());
            result["node"] = json!(node);
            result["time"] = json!(time);
            return Ok(result);
        }
        match name {
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
            "diagnostics" => match &self.renderer {
                Some(renderer) => Ok(
                    json!({"initialized":true,"adapter":renderer.adapter.name,"snapshot":renderer.try_diagnostics()?}),
                ),
                None => Ok(json!({"initialized":false,"adapter":null,"snapshot":null})),
            },
            "outline_text" => {
                let mut payload = args.clone();
                payload.as_object_mut().unwrap().remove("revision");
                let request: crate::assets::text::OutlineRequest = serde_json::from_value(payload)?;
                let outlined = crate::assets::text::outline(&self.asset_base, &request)?;
                Ok(
                    json!({"revision":self.document.revision(),"operation":{"type":"path","geometry":outlined.geometry},"glyph_count":outlined.glyph_count,"advance":outlined.advance,"line_count":outlined.line_count}),
                )
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
                    &mut |done, total| progress(done as u64, total as u64, "tracking frames"),
                )?;
                self.document.apply(self.document.revision(), &commands)?;
                report["node"] = json!(node);
                report["revision"] = json!(self.document.revision());
                Ok(report)
            }
            "solve_camera" => {
                let request =
                    crate::solve::Request::from_json(args, self.document.snapshot().frame_count)?;
                request.check(self.document.snapshot())?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                let (commands, mut report) = crate::solve::run(
                    self.renderer.as_ref().unwrap(),
                    self.document.snapshot(),
                    &self.asset_base,
                    &request,
                    &mut |done, total| progress(done as u64, total as u64, "following features"),
                )?;
                self.document.apply(self.document.revision(), &commands)?;
                report["revision"] = json!(self.document.revision());
                Ok(report)
            }
            "segment_subject" => {
                let request =
                    crate::segment::Request::from_json(args, self.document.snapshot().frame_count)?;
                request.check(self.document.snapshot())?;
                if self.renderer.is_none() {
                    self.renderer = Some(crate::assets::runtime::renderer()?);
                }
                // The mattes go beside the saved project; the node names them from the
                // session's own base, which is what every other path here is relative to.
                let home = self
                    .saved_dir
                    .clone()
                    .unwrap_or_else(|| self.asset_base.clone());
                let (node, mut commands, mut report) = crate::segment::run(
                    self.renderer.as_ref().unwrap(),
                    self.document.snapshot(),
                    &self.asset_base,
                    &home,
                    &request,
                    &mut |done, total| progress(done as u64, total as u64, "cutting out frames"),
                )?;
                if home != self.asset_base {
                    for command in &mut commands {
                        if let Command::Put { node } = command
                            && let Some(asset) = node.operation.asset_mut()
                            && std::path::Path::new(asset.as_str()).is_relative()
                        {
                            *asset = home.join(asset.as_str()).to_string_lossy().into_owned();
                        }
                    }
                }
                self.document.apply(self.document.revision(), &commands)?;
                report["node"] = json!(node);
                report["revision"] = json!(self.document.revision());
                Ok(report)
            }
            "undo" => Ok(json!({"revision":self.document.undo(self.document.revision())?})),
            "redo" => Ok(json!({"revision":self.document.redo(self.document.revision())?})),
            "save" => {
                crate::assets::save(self.document.snapshot(), &self.asset_base, path(args)?)?;
                self.saved = self.document.revision();
                self.saved_dir = path(args)?.parent().and_then(|dir| dir.canonicalize().ok());
                Ok(json!({"saved":args["path"],"revision":self.document.revision()}))
            }
            "replace_sound" => {
                let doc = self.document.snapshot();
                let video =
                    std::path::Path::new(args["video"].as_str().ok_or("video must be a string")?);
                if !video.is_absolute() || !path(args)?.is_absolute() {
                    return Err("video and path must be absolute paths".into());
                }
                crate::assets::protect_inputs(doc, &self.asset_base, path(args)?)?;
                let seconds = f64::from(doc.frame_count) * doc.frame_rate.rate as f64
                    / doc.frame_rate.ticks as f64;
                let sources = crate::assets::export_audio::resound(
                    doc,
                    &self.asset_base,
                    video,
                    path(args)?,
                    seconds,
                )?;
                Ok(
                    json!({"output":args["path"],"revision":self.document.revision(),"audio":sources,"seconds":seconds}),
                )
            }
            "render_video" => {
                let doc = self.document.snapshot();
                let mut settings: crate::assets::video_export::Settings = match args.get("settings")
                {
                    Some(value) => {
                        let mut merged = serde_json::to_value(
                            crate::assets::video_export::Settings::for_document(doc),
                        )?;
                        let given = value.as_object().ok_or("settings must be an object")?;
                        for (key, value) in given {
                            merged[key.as_str()] = value.clone();
                        }
                        serde_json::from_value(merged)?
                    }
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
                    |p| {
                        progress(u64::from(p.completed), u64::from(p.total), &p.phase);
                        true
                    },
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
                    |done, total| {
                        progress(u64::from(done), u64::from(total), "rendering frames");
                        true
                    },
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
        saved: 0,
        asset_base,
        saved_dir: None,
        renderer: None,
    };
    veylvik_control::serve_reporting(input, output, tools(), |name, args, progress| {
        session
            .call_output(name, args, progress)
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests;

/// The headless server's own parts of the shared tools: the user's material library and
/// ground placement from files, the recipe's and copy's own layout.
struct Files;
impl veylvik_presets::tools::Host for Files {
    fn materials(&self) -> veylvik_document::Result<Value> {
        veylvik_io::materials::directory()
            .and_then(|dir| veylvik_io::materials::library(&dir))
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
    fn material(
        &self,
        document: &Composition,
        target: veylvik_document::NodeId,
        material: &str,
    ) -> veylvik_document::Result<Vec<Command>> {
        veylvik_io::materials::directory()
            .and_then(|dir| veylvik_io::materials::apply(document, target, material, &dir))
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
    fn stand(
        &self,
        document: &Composition,
        args: &Value,
    ) -> veylvik_document::Result<(Vec<Command>, Value)> {
        crate::assets::placing::stand(document, args)
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
}
/// The open document as the shared tools' target.
struct Opened<'a>(&'a mut Document);
impl veylvik_presets::tools::Target for Opened<'_> {
    fn revision(&self) -> u64 {
        self.0.revision()
    }
    fn prepare(&self, commands: &[Command]) -> veylvik_document::Result<Composition> {
        self.0.prepare(self.0.revision(), commands)
    }
    fn apply(&mut self, commands: &[Command]) -> veylvik_document::Result<()> {
        self.0.apply(self.0.revision(), commands).map(|_| ())
    }
}
