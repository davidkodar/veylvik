//! The tools both MCP servers offer on a document, defined once: their descriptions and argument schemas, the reads, and for each editing tool the step from arguments to one undoable edit. A server supplies only what is its own through `Host` (the material library, the ground under a 3D object, where new nodes go in its graph) and `Target` (trying and applying commands to its document), so the headless server and the live editor cannot drift apart on these.
use crate::authoring::{self, Authored};
use serde_json::{Value, json};
use veylvik_document::{Command, Composition, Error, NodeId, Result};

/// Which server a catalogue is for: the live editor's tools also take its `session`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    Headless,
    Live,
}
struct Spec {
    name: &'static str,
    headless: &'static str,
    /// The live editor's own wording, where it differs (it places, selects and previews).
    live: Option<&'static str>,
    properties: Value,
    required: &'static [&'static str],
}
fn specs(revision: &Value) -> Vec<Spec> {
    let revision = revision.clone();
    vec![
        Spec {
            name: "graph",
            headless: "Page node metadata in stable UUID order without parameter payloads. Labels over 256 characters are marked as truncated. Compare revisions between pages.",
            live: None,
            properties: json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":128}}),
            required: &[],
        },
        Spec {
            name: "get_node",
            headless: "Read one exact authored node, or just a named parameter. Does not evaluate curves or change selection.",
            live: None,
            properties: json!({"id":{"type":"string","format":"uuid"},"parameter":{"type":"string"}}),
            required: &["id"],
        },
        Spec {
            name: "nodes",
            headless: "List supported node operations, ports and example payloads.",
            live: Some("Read built-in operations, ports, defaults and parameter schemas."),
            properties: json!({}),
            required: &[],
        },
        Spec {
            name: "materials",
            headless: "Every material that apply_material accepts: family surfaces (source family: changes the surface kind and keeps the target's finish and lighting), catalogue looks and saved materials (whole looks), with label, category, description and whether the user starred it. Saved ids are saved:<name>.",
            live: None,
            properties: json!({}),
            required: &[],
        },
        Spec {
            name: "apply_material",
            headless: "Put a material from materials on a Text node (adding or restyling its Text surface, exactly like the library) or restyle a Text surface node in place. One undo step; output and wiring stay. Dry-run validates only.",
            live: Some(
                "Put a material from materials on a Text node (adding or restyling its Text surface, exactly like the library) or restyle a Text surface node in place. One undo step; output and wiring stay. Selects and previews the target.",
            ),
            properties: json!({"revision":revision,"target":{"type":"string","format":"uuid"},"material":{"type":"string","minLength":1,"maxLength":256},"dry_run":{"type":"boolean"}}),
            required: &["revision", "target", "material"],
        },
        Spec {
            name: "get_preset",
            headless: "Read a bundled starter, or capture a group with its current values and animation. Supply exactly one of starter or group. No filesystem access.",
            live: None,
            properties: json!({"group":{"type":"string","format":"uuid"},"starter":{"type":"string","enum":veylvik_document::presets::builtin::ALL.map(|p|p.id).as_slice()}}),
            required: &[],
        },
        Spec {
            name: "insert_preset",
            headless: "Insert an independent instance with explicitly bound inputs. One undo step; output remains unchanged. Dry-run IDs are provisional.",
            live: None,
            properties: json!({"revision":revision,"preset":veylvik_document::schema::preset(),"inputs":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}},"sizing":{"type":"string","enum":["fit","fill","original"],"default":"fit"},"position":veylvik_document::schema::position(),"dry_run":{"type":"boolean"}}),
            required: &["revision", "preset", "inputs"],
        },
        Spec {
            name: "apply",
            headless: "Validate or commit an atomic batch. dry_run returns the candidate without changing history. Use nodes for payload examples. A put command inserts or replaces a complete node; set_parameter changes one registered parameter, and set_key inserts or updates a scalar key.",
            live: Some(
                "Validate or atomically apply commands to the open editor. Requires the session and revision from inspect. set_parameter and set_key preserve node identity and wiring. Image references must already be known to the editor; new paths require import. Unapplied inspector changes or file jobs block mutation.",
            ),
            properties: json!({"revision":revision,"dry_run":{"type":"boolean"},"commands":{"type":"array","minItems":1,"maxItems":4096,"items":veylvik_document::schema::command()}}),
            required: &["revision", "commands"],
        },
        Spec {
            name: "add_effect",
            headless: "Create an editable effect group with exposed controls and ordinary internal nodes as one transaction. Returns root and named role IDs; output is unchanged and layout is automatic. Discover recipes with nodes. Dry-run IDs are provisional.",
            live: Some(
                "Create an editable effect group with exposed controls and ordinary internal nodes as one undoable transaction. Places the effect without overlapping existing nodes, selects/previews its root, and preserves composition output. Returns named role IDs for editing controls. Discover recipes with nodes; dry-run IDs are provisional.",
            ),
            properties: json!({"revision":revision,"input":{"type":"string","format":"uuid"},"recipe":{"type":"string","enum":veylvik_document::recipes::Recipe::ALL},"dry_run":{"type":"boolean"}}),
            required: &["revision", "input", "recipe"],
        },
        Spec {
            name: "duplicate_branch",
            headless: "Copy a node and all upstream dependencies as one transaction. Shared ancestors are copied once; returns the copied root and old-to-new ID mapping. Output and assets stay unchanged. Stored layout positions are preserved. Dry-run IDs are provisional and are not reused on commit.",
            live: Some(
                "Copy a node and all upstream dependencies as one undoable transaction, preserving shared ancestors and asset references. Places the copy outside existing graph bounds, selects/previews its root, and leaves composition output unchanged. Returns root and old-to-new ID mapping. Dry-run IDs are provisional and are not reused on commit.",
            ),
            properties: json!({"revision":revision,"node":{"type":"string","format":"uuid"},"dry_run":{"type":"boolean"}}),
            required: &["revision", "node"],
        },
        Spec {
            name: "undo",
            headless: "Undo one transaction, with a revision precondition.",
            live: Some("Undo one editor transaction."),
            properties: json!({"revision":revision}),
            required: &["revision"],
        },
        Spec {
            name: "redo",
            headless: "Redo one transaction, with a revision precondition.",
            live: Some("Redo one editor transaction."),
            properties: json!({"revision":revision}),
            required: &["revision"],
        },
    ]
}
/// A tool descriptor as MCP lists it.
pub fn descriptor(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
/// The shared tools' descriptors for `server`: the document tools and the authoring
/// tools. A tool that edits takes the document's `revision` (and, live, the `session`).
pub fn catalogue(server: Server) -> Vec<Value> {
    let revision = json!({"type":"integer","minimum":0});
    let session = json!({"type":"string","format":"uuid"});
    let live = server == Server::Live;
    let mut out = Vec::new();
    for spec in specs(&revision) {
        let mut properties = spec.properties;
        let mut required: Vec<&str> = spec.required.to_vec();
        if live && properties.get("revision").is_some() {
            properties["session"] = session.clone();
            required.insert(0, "session");
        }
        let description = if live {
            spec.live.unwrap_or(spec.headless)
        } else {
            spec.headless
        };
        out.push(descriptor(spec.name, description, properties, &required));
    }
    for (name, description, mut properties, required) in authoring::tools() {
        if live {
            properties["session"] = session.clone();
        }
        properties["revision"] = revision.clone();
        properties["dry_run"] = json!({"type":"boolean"});
        let extra: &[&str] = if live {
            &["session", "revision"]
        } else {
            &["revision"]
        };
        let required: Vec<&str> = required
            .iter()
            .copied()
            .chain(extra.iter().copied())
            .collect();
        out.push(descriptor(name, description, properties, &required));
    }
    out
}
/// Checks `args` against `name`'s descriptor in `tools`: no argument it does not take,
/// every one it needs. `hint` may add a server's own advice for an unknown argument.
pub fn check(
    tools: &[Value],
    name: &str,
    args: &Value,
    hint: &dyn Fn(&str) -> String,
) -> Result<()> {
    let descriptor = tools
        .iter()
        .find(|t| t["name"] == name)
        .ok_or_else(|| Error(format!("unknown tool {name}")))?;
    let object = args
        .as_object()
        .ok_or_else(|| Error("arguments must be an object".into()))?;
    let properties = &descriptor["inputSchema"]["properties"];
    for key in object.keys() {
        if properties.get(key).is_none() {
            let takes: Vec<&str> = properties
                .as_object()
                .map(|p| p.keys().map(String::as_str).collect())
                .unwrap_or_default();
            return Err(Error(format!(
                "{name} has no argument {key}; it takes {}{}",
                takes.join(", "),
                hint(key)
            )));
        }
    }
    for key in descriptor["inputSchema"]["required"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !object.contains_key(key.as_str().unwrap_or_default()) {
            return Err(Error(format!("missing argument {key}")));
        }
    }
    Ok(())
}
/// Whether the call asks to try the edit without keeping it.
pub fn dry_run(args: &Value) -> Result<bool> {
    match args.get("dry_run") {
        None => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| Error("dry_run must be boolean".into())),
    }
}
fn node(args: &Value, name: &str) -> Result<NodeId> {
    args[name]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| Error(format!("{name} must be a UUID")))
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value, what: &str) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|e| Error(format!("{what}: {e}")))
}
/// The read tools that need only the document: `graph`, `get_node`, `nodes` and
/// `get_preset`. None for any other tool.
pub fn read(document: &Composition, name: &str, args: &Value) -> Result<Option<Value>> {
    Ok(Some(match name {
        "graph" => {
            let number = |key: &str, default: u64| -> Result<u64> {
                args.get(key)
                    .map(|v| {
                        v.as_u64()
                            .ok_or_else(|| Error(format!("{key} must be an unsigned integer")))
                    })
                    .transpose()
                    .map(|v| v.unwrap_or(default))
            };
            veylvik_document::inspection::page(
                document,
                number("offset", 0)?,
                number("limit", 32)?,
            )?
        }
        "get_node" => {
            let parameter = args
                .get("parameter")
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| Error("parameter must be a string".into()))
                })
                .transpose()?;
            veylvik_document::inspection::node(document, node(args, "id")?, parameter)?
        }
        "nodes" => veylvik_document::registry::catalog(),
        "get_preset" => {
            json!({"preset": veylvik_document::presets::read(document, args.get("group"), args.get("starter"))?})
        }
        _ => return Ok(None),
    }))
}
/// What only a server can do for the shared tools.
pub trait Host {
    /// Every material `apply_material` accepts (the server's material library).
    fn materials(&self) -> Result<Value>;
    /// The commands that put `material` on `target`.
    fn material(
        &self,
        document: &Composition,
        target: NodeId,
        material: &str,
    ) -> Result<Vec<Command>>;
    /// The commands and reply that stand a 3D object on the shot's ground.
    fn stand(&self, document: &Composition, args: &Value) -> Result<(Vec<Command>, Value)>;
    /// Lays out a new effect in the server's graph (a headless server keeps the recipe's layout).
    fn place_effect(
        &mut self,
        _document: &Composition,
        _revision: u64,
        _input: NodeId,
        _effect: &mut veylvik_document::recipes::Effect,
    ) -> Result<()> {
        Ok(())
    }
    /// Lays out a copied branch (a headless server keeps the stored positions).
    fn place_copy(
        &mut self,
        _document: &Composition,
        _revision: u64,
        _copy: &mut veylvik_document::editing::BranchCopy,
    ) -> Result<()> {
        Ok(())
    }
}
/// One editing call worked out: the commands for one undo step, the reply's own
/// fields, and the node a live editor selects and previews afterwards.
pub struct Edit {
    pub commands: Vec<Command>,
    pub result: Value,
    pub focus: Option<NodeId>,
    /// A dry run replies with the whole candidate composition (`apply`).
    pub candidate: bool,
}
impl From<Authored> for Edit {
    fn from(a: Authored) -> Self {
        Edit {
            commands: a.commands,
            result: a.result,
            focus: None,
            candidate: false,
        }
    }
}
/// Works out the editing tool `name`: `apply`, `insert_preset`, `apply_material`,
/// `add_effect`, `duplicate_branch`, `stand_on_ground` and the authoring tools. None
/// for any other tool.
pub fn edit(
    document: &Composition,
    revision: u64,
    name: &str,
    args: &Value,
    host: &mut dyn Host,
) -> Result<Option<Edit>> {
    if let Some(authored) = authoring::build(document, name, args)? {
        return Ok(Some(authored.into()));
    }
    Ok(Some(match name {
        "apply" => Edit {
            commands: parse(&args["commands"], "commands")?,
            result: json!({}),
            focus: None,
            candidate: true,
        },
        "insert_preset" => {
            let preset: veylvik_document::presets::Preset = parse(&args["preset"], "preset")?;
            let inputs = parse(&args["inputs"], "inputs")?;
            let position = args
                .get("position")
                .map(|v| parse(v, "position"))
                .transpose()?
                .flatten();
            let sizing = args
                .get("sizing")
                .map(|v| parse(v, "sizing"))
                .transpose()?
                .unwrap_or_default();
            let instance = preset.instantiate_sized(document, &inputs, position, sizing)?;
            Edit {
                result: json!({"group":instance.group,"root":instance.output}),
                focus: Some(instance.output),
                commands: instance.commands,
                candidate: false,
            }
        }
        "apply_material" => {
            let target = node(args, "target")?;
            let material = args["material"]
                .as_str()
                .ok_or_else(|| Error("material must be a string".into()))?;
            let commands = host.material(document, target, material)?;
            Edit {
                result: json!({"changed_nodes":commands.len()}),
                commands,
                focus: Some(target),
                candidate: false,
            }
        }
        "add_effect" => {
            let input = node(args, "input")?;
            let recipe: veylvik_document::recipes::Recipe = parse(&args["recipe"], "recipe")?;
            let mut effect = recipe.build(document, input)?;
            host.place_effect(document, revision, input, &mut effect)?;
            let result = json!({"root":effect.root,"roles":effect.roles,"added_nodes":effect.nodes.len(),"recipe":recipe});
            let focus = Some(effect.root);
            Edit {
                commands: effect.into_grouped_commands(document)?,
                result,
                focus,
                candidate: false,
            }
        }
        "duplicate_branch" => {
            let mut copy = veylvik_document::editing::copy_branch(document, node(args, "node")?)?;
            host.place_copy(document, revision, &mut copy)?;
            let result =
                json!({"root":copy.root,"mapping":copy.mapping,"copied_nodes":copy.nodes.len()});
            let focus = Some(copy.root);
            Edit {
                commands: copy.into_commands(),
                result,
                focus,
                candidate: false,
            }
        }
        "stand_on_ground" => {
            let (commands, result) = host.stand(document, args)?;
            Edit {
                commands,
                result,
                focus: None,
                candidate: false,
            }
        }
        _ => return Ok(None),
    }))
}
/// Where an edit lands: a server's document.
pub trait Target {
    fn revision(&self) -> u64;
    /// The composition the commands would make, changing nothing.
    fn prepare(&self, commands: &[Command]) -> Result<Composition>;
    /// Applies the commands as one undo step.
    fn apply(&mut self, commands: &[Command]) -> Result<()>;
}
/// Tries (`dry_run`) or applies an edit and gives its reply: the tool's own fields with
/// `dry_run`, `valid` and the revision after (before, for a dry run).
pub fn commit(target: &mut dyn Target, edit: Edit, dry_run: bool) -> Result<Value> {
    let mut result = edit.result;
    if dry_run {
        let candidate = target.prepare(&edit.commands)?;
        if edit.candidate {
            result["candidate"] =
                serde_json::to_value(candidate).map_err(|e| Error(e.to_string()))?;
        }
    } else {
        target.apply(&edit.commands)?;
    }
    result["dry_run"] = json!(dry_run);
    result["valid"] = json!(true);
    result["revision"] = json!(target.revision());
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Plain;
    impl Host for Plain {
        fn materials(&self) -> Result<Value> {
            Ok(json!([]))
        }
        fn material(&self, _: &Composition, _: NodeId, _: &str) -> Result<Vec<Command>> {
            Err(Error("no library".into()))
        }
        fn stand(&self, _: &Composition, _: &Value) -> Result<(Vec<Command>, Value)> {
            Err(Error("no ground".into()))
        }
    }
    struct Doc(veylvik_document::Document);
    impl Target for Doc {
        fn revision(&self) -> u64 {
            self.0.revision()
        }
        fn prepare(&self, commands: &[Command]) -> Result<Composition> {
            self.0.prepare(self.0.revision(), commands)
        }
        fn apply(&mut self, commands: &[Command]) -> Result<()> {
            self.0.apply(self.0.revision(), commands).map(|_| ())
        }
    }
    #[test]
    fn both_servers_list_the_same_shared_tools_with_the_same_arguments() {
        let headless = catalogue(Server::Headless);
        let live = catalogue(Server::Live);
        assert_eq!(headless.len(), live.len());
        for (h, l) in headless.iter().zip(&live) {
            assert_eq!(h["name"], l["name"]);
            let mut lp = l["inputSchema"]["properties"].clone();
            lp.as_object_mut().unwrap().remove("session");
            assert_eq!(h["inputSchema"]["properties"], lp, "{}", h["name"]);
        }
    }
    #[test]
    fn an_edit_is_tried_without_change_and_then_applied_as_one_step() {
        let mut doc = Doc(veylvik_document::Document::new(veylvik_document::demo()).unwrap());
        let source = doc.0.snapshot().output;
        let args = json!({"node": source.to_string()});
        let before = doc.revision();
        let tried = edit(
            doc.0.snapshot(),
            before,
            "duplicate_branch",
            &args,
            &mut Plain,
        )
        .unwrap()
        .unwrap();
        let reply = commit(&mut doc, tried, true).unwrap();
        assert_eq!(
            (reply["dry_run"].clone(), reply["revision"].clone()),
            (json!(true), json!(before))
        );
        let made = edit(
            doc.0.snapshot(),
            before,
            "duplicate_branch",
            &args,
            &mut Plain,
        )
        .unwrap()
        .unwrap();
        assert!(made.focus.is_some());
        let reply = commit(&mut doc, made, false).unwrap();
        assert_eq!(reply["revision"], json!(before + 1));
        assert!(reply["mapping"].is_object() && reply["valid"] == json!(true));
        // The host's own parts stay the host's: a material is refused by this one.
        let material = json!({"target": source.to_string(), "material": "gold"});
        assert!(edit(doc.0.snapshot(), 0, "apply_material", &material, &mut Plain).is_err());
        assert!(
            read(doc.0.snapshot(), "graph", &json!({}))
                .unwrap()
                .is_some()
        );
        assert!(
            edit(doc.0.snapshot(), 0, "render", &json!({}), &mut Plain)
                .unwrap()
                .is_none()
        );
        // Unknown and missing arguments are named.
        let tools = catalogue(Server::Headless);
        let wrong = check(&tools, "graph", &json!({"page": 1}), &|_| String::new()).unwrap_err();
        assert!(
            wrong.0.contains("graph has no argument page"),
            "{}",
            wrong.0
        );
    }
}
