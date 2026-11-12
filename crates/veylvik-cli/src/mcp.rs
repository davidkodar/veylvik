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
fn tools() -> Value {
    let revision = json!({"type":"integer","minimum":0});
    json!({"tools":[
        tool("inspect","Read project state and revision. Set summary=true for compact composition metadata without node payloads; graph and get_node provide targeted inspection.",json!({"summary":{"type":"boolean"}}),&[]),
        tool("graph","Page node metadata in stable UUID order without parameter payloads. Labels over 256 characters are marked as truncated. Compare revisions between pages.",json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":128}}),&[]),
        tool("get_node","Read one exact authored node, or just a named parameter. Does not evaluate curves or change selection.",json!({"id":{"type":"string","format":"uuid"},"parameter":{"type":"string"}}),&["id"]),
        tool("diagnostics","Read existing renderer buffer and cache counters without initializing a GPU. Snapshot is null until rendering initializes the device. Declared buffer accounting excludes driver and rasterizer scratch memory.",json!({}),&[]),
        tool("nodes","List supported node operations, ports and example payloads.",json!({}),&[]),
        tool("outline_text","Read an explicit font and convert explicitly separated directional/script text lines into portable Path mask geometry. Does not modify the document. Apply the returned operation with a put command. No fallback, automatic wrapping or mixed-direction paragraph layout.",json!({"revision":revision,"font":{"type":"string"},"text":{"type":"string","maxLength":4096},"alignment":{"enum":["left","center","right"]},"line_height":{"type":"number","minimum":1,"maximum":4096},"font_index":{"type":"integer","minimum":0,"maximum":u32::MAX},"font_size":{"type":"number","minimum":1,"maximum":1024},"origin":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-32768,"maximum":32768}},"direction":{"enum":["left_to_right","right_to_left"]},"language":{"type":"string","minLength":1,"maxLength":64}}),&["revision","font","text"]),
        tool("apply","Validate or commit an atomic batch. dry_run returns the candidate without changing history. Use nodes for payload examples. A put command inserts or replaces a complete node; set_parameter changes one registered parameter, and set_key inserts or updates a scalar key.",json!({"revision":revision,"dry_run":{"type":"boolean"},"commands":{"type":"array","minItems":1,"maxItems":4096,"items":veylvik_document::schema::command()}}),&["revision","commands"]),
        tool("undo","Undo one transaction, with a revision precondition.",json!({"revision":revision}),&["revision"]),
        tool("redo","Redo one transaction, with a revision precondition.",json!({"revision":revision}),&["revision"]),
        tool("save","Atomically save the current composition to an explicit path.",json!({"path":{"type":"string"},"revision":revision}),&["path","revision"]),
        tool("render_sequence","Render an inclusive frame range at the composition frame rate into a new directory. Choose png (default SDR) or exr (linear HDR); frames use zero-based names; sequence.json records timing and completion. Existing directories are rejected; at most 10000 frames.",json!({"path":{"type":"string"},"revision":revision,"start":{"type":"integer"},"end":{"type":"integer"},"format":{"enum":["png","exr"]},"node":{"type":"string","format":"uuid"},"region":{"type":"object","properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1}},"required":["x","y","width","height"],"additionalProperties":false}}),&["path","revision","start","end"]),
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
    fn call(&mut self, name: &str, args: &Value) -> Result<Value> {
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
            "inspect" | "nodes" | "diagnostics" | "graph" | "get_node"
        ) && required_u64(args, "revision")? != self.document.revision()
        {
            return Err(format!("revision conflict: current {}", self.document.revision()).into());
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
                    self.renderer = Some(Renderer::new()?);
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
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()?;
                if let Some(region) = region {
                    region.validate(doc.width, doc.height)?;
                }
                let plan = Plan::compile_output(doc, output)?;
                if self.renderer.is_none() {
                    self.renderer = Some(Renderer::new()?);
                }
                let renderer = self.renderer.as_ref().unwrap();
                let inputs =
                    crate::assets::load_inputs(&plan, &self.asset_base, renderer.budget_bytes)?;
                crate::assets::protect_inputs(doc, &self.asset_base, path(args)?)?;
                let frame =
                    renderer.render_frame(&plan, RenderRequest { time, region }, &inputs)?;
                crate::save_frame(
                    path(args)?,
                    frame.region.width,
                    frame.region.height,
                    &frame.pixels,
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
    veylvik_control::serve(input, output, &tools(), |name, args| {
        session.call(name, args).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targeted_queries_validate_arguments_without_mutation_or_gpu_initialization() {
        let mut session = Session {
            document: Document::new(veylvik_document::motion_demo()).unwrap(),
            asset_base: std::env::temp_dir(),
            renderer: None,
        };
        let id = session.document.snapshot().output;
        let summary = session.call("inspect", &json!({"summary":true})).unwrap();
        assert!(summary.get("composition").is_none());
        assert_eq!(summary["summary"]["output"], json!(id));
        assert!(session.call("inspect", &json!({"summary":"yes"})).is_err());
        let page = session.call("graph", &json!({"limit":2})).unwrap();
        assert_eq!(page["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(page["revision"], 0);
        let full = session.call("get_node", &json!({"id":id})).unwrap();
        let parameter = session
            .call("get_node", &json!({"id":id,"parameter":"stops"}))
            .unwrap();
        assert_eq!(parameter["value"], full["node"]["operation"]["stops"]);
        for args in [
            json!({"offset":-1}),
            json!({"limit":0}),
            json!({"limit":"2"}),
            json!({"offset":100000}),
        ] {
            assert!(session.call("graph", &args).is_err());
        }
        for args in [
            json!({}),
            json!({"id":"bad"}),
            json!({"id":id,"parameter":3}),
            json!({"id":id,"parameter":"unknown"}),
        ] {
            assert!(session.call("get_node", &args).is_err());
        }
        assert_eq!(session.document.revision(), 0);
        assert!(session.renderer.is_none());
    }
    #[test]
    fn protocol_enforces_lifecycle_and_transaction_revision() {
        let messages = [
            json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"apply","arguments":{"revision":0,"commands":[{"command":"resize","width":100,"height":50}]}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"undo","arguments":{"revision":0}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"inspect"}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/list"}),
        ];
        let input = messages
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let mut output = Vec::new();
        serve(
            veylvik_document::demo(),
            std::env::temp_dir(),
            std::io::Cursor::new(input),
            &mut output,
        )
        .unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 6);
        assert_eq!(replies[0]["error"]["code"], -32002);
        assert_eq!(replies[2]["result"]["structuredContent"]["revision"], 1);
        assert_eq!(replies[3]["result"]["isError"], true);
        assert_eq!(
            replies[4]["result"]["structuredContent"]["composition"]["width"],
            100
        );
        let names: std::collections::BTreeSet<_> = replies[5]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "inspect",
                "graph",
                "get_node",
                "diagnostics",
                "nodes",
                "outline_text",
                "apply",
                "undo",
                "redo",
                "save",
                "render",
                "render_sequence"
            ]
            .into()
        );
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn apply_schema_uses_the_registry_and_image_edits_remain_undoable() {
        let catalog = tools();
        let apply = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "apply")
            .unwrap();
        assert_eq!(
            apply["inputSchema"]["properties"]["commands"]["items"]["oneOf"][0]["properties"]["node"]
                ["properties"]["operation"],
            veylvik_document::registry::operation_schema()
        );
        let composition = veylvik_document::demo();
        let output = composition.output;
        let mut session = Session {
            document: Document::new(composition).unwrap(),
            asset_base: std::env::temp_dir(),
            renderer: None,
        };
        let diagnostics = session.call("diagnostics", &json!({})).unwrap();
        assert_eq!(diagnostics["initialized"], false);
        assert!(diagnostics["snapshot"].is_null());
        assert!(session.renderer.is_none());
        assert_eq!(session.document.revision(), 0);
        let command = json!({"command":"put","node":{"id":output,"version":1,"label":"Plate","operation":{"type":"image","asset":"plate.png"},"inputs":{}}});
        session
            .call(
                "apply",
                &json!({"revision":0,"dry_run":true,"commands":[command]}),
            )
            .unwrap();
        assert_eq!(session.document.revision(), 0);
        session
            .call("apply", &json!({"revision":0,"commands":[command]}))
            .unwrap();
        assert_eq!(session.document.revision(), 1);
        session.call("undo", &json!({"revision":1})).unwrap();
        assert!(matches!(
            session.document.snapshot().nodes[&output].operation,
            veylvik_document::Operation::Merge {}
        ));
        let error=session.call("render",&json!({"revision":2,"path":"/tmp/unused.png","ticks":0,"rate":1,"region":{"x":0,"y":0,"width":0,"height":1}})).unwrap_err();
        assert!(error.to_string().contains("region"));
        assert!(session.renderer.is_none());
    }
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
