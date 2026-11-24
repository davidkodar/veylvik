//! Headless MCP tests: catalogue and revisions, images, branches, effects and presets.
use super::*;
#[test]
fn targeted_queries_validate_arguments_without_mutation_or_gpu_initialization() {
    let mut session = Session {
        document: Document::new(veylvik_document::motion_demo()).unwrap(),
        saved: 0,
        asset_base: std::env::temp_dir(),
        saved_dir: None,
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
            "materials",
            "apply_material",
            "get_preset",
            "insert_preset",
            "diagnostics",
            "nodes",
            "outline_text",
            "preview_image",
            "apply",
            "duplicate_branch",
            "add_effect",
            "undo",
            "redo",
            "save",
            "render",
            "render_sequence",
            "render_video",
            "replace_sound",
            "track_surface",
            "solve_camera",
            "segment_subject",
            "new_project",
            "open_project",
            "new_from_media",
            "import_media",
            "match_shot",
            "measure",
            "add_node",
            "gold_title",
            "particle_look",
            "stand_on_ground",
            "surround_lights"
        ]
        .into()
    );
}
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
            veylvik_document::registry::operation_reference()
        );
        let composition = veylvik_document::demo();
        let output = composition.output;
        let mut session = Session {
            document: Document::new(composition).unwrap(),
            saved: 0,
            asset_base: std::env::temp_dir(),
            saved_dir: None,
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
            veylvik_document::Operation::Merge(veylvik_document::nodes::Merge {})
        ));
        let error=session.call("render",&json!({"revision":2,"path":"/tmp/unused.png","ticks":0,"rate":1,"region":{"x":0,"y":0,"width":0,"height":1}})).unwrap_err();
        assert!(error.to_string().contains("region"));
        assert!(session.renderer.is_none());
    }
}
mod branch_tests {
    use super::*;
    #[test]
    fn branch_tool_dry_run_guards_and_history_do_not_initialize_renderer() {
        let composition = veylvik_document::demo();
        let root = composition.output;
        let before = serde_json::to_value(&composition).unwrap();
        let count = composition.nodes.len();
        let mut session = Session {
            document: Document::new(composition).unwrap(),
            saved: 0,
            asset_base: std::env::temp_dir(),
            saved_dir: None,
            renderer: None,
        };
        let dry = session
            .call(
                "duplicate_branch",
                &json!({"revision":0,"node":root,"dry_run":true}),
            )
            .unwrap();
        assert_eq!(dry["valid"], true);
        assert_eq!(dry["revision"], 0);
        assert_eq!(
            serde_json::to_value(session.document.snapshot()).unwrap(),
            before
        );
        assert_eq!(session.document.history().undo_steps, 0);
        for args in [
            json!({"revision":1,"node":root}),
            json!({"revision":0,"node":root,"dry_run":"true"}),
            json!({"revision":0,"node":"not-a-uuid"}),
            json!({"revision":0,"node":veylvik_document::NodeId::new_v4()}),
            json!({"revision":0,"node":root,"extra":true}),
        ] {
            assert!(session.call("duplicate_branch", &args).is_err());
        }
        let actual = session
            .call("duplicate_branch", &json!({"revision":0,"node":root}))
            .unwrap();
        assert_eq!(actual["revision"], 1);
        assert_eq!(actual["dry_run"], false);
        assert_ne!(actual["root"], dry["root"]);
        let copied: veylvik_document::NodeId = actual["root"].as_str().unwrap().parse().unwrap();
        assert!(session.document.snapshot().nodes.contains_key(&copied));
        assert_eq!(session.document.snapshot().output, root);
        assert_eq!(
            session.document.snapshot().nodes.len(),
            count + actual["copied_nodes"].as_u64().unwrap() as usize
        );
        assert_eq!(actual["mapping"][root.to_string()], actual["root"]);
        session.call("undo", &json!({"revision":1})).unwrap();
        assert_eq!(
            serde_json::to_value(session.document.snapshot()).unwrap(),
            before
        );
        session.call("redo", &json!({"revision":2})).unwrap();
        assert!(session.document.snapshot().nodes.contains_key(&copied));
        assert!(session.renderer.is_none());
    }
}
mod effect_tests {
    use super::*;
    #[test]
    fn effects_have_discoverable_roles_revision_guards_and_atomic_history() {
        for recipe in veylvik_document::recipes::Recipe::ALL {
            let composition = veylvik_document::demo();
            let input = composition.output;
            let before = serde_json::to_value(&composition).unwrap();
            let count = composition.nodes.len();
            let mut session = Session {
                document: Document::new(composition).unwrap(),
                saved: 0,
                asset_base: std::env::temp_dir(),
                saved_dir: None,
                renderer: None,
            };
            let dry = session
                .call(
                    "add_effect",
                    &json!({"revision":0,"input":input,"recipe":recipe,"dry_run":true}),
                )
                .unwrap();
            assert_eq!(dry["added_nodes"], recipe.node_count());
            assert_eq!(dry["revision"], 0);
            assert_eq!(
                serde_json::to_value(session.document.snapshot()).unwrap(),
                before
            );
            for args in [
                json!({"revision":1,"input":input,"recipe":recipe}),
                json!({"revision":0,"input":input,"recipe":"unknown"}),
                json!({"revision":0,"input":input,"recipe":recipe,"dry_run":1}),
            ] {
                assert!(session.call("add_effect", &args).is_err());
            }
            let actual = session
                .call(
                    "add_effect",
                    &json!({"revision":0,"input":input,"recipe":recipe}),
                )
                .unwrap();
            assert_ne!(actual["root"], dry["root"]);
            assert_eq!(actual["root"], actual["roles"]["composite"]);
            for id in actual["roles"].as_object().unwrap().values() {
                assert!(
                    session
                        .document
                        .snapshot()
                        .nodes
                        .contains_key(&id.as_str().unwrap().parse().unwrap())
                );
            }
            assert_eq!(session.document.snapshot().output, input);
            assert_eq!(
                session.document.snapshot().nodes.len(),
                count + recipe.node_count()
            );
            assert_eq!(session.document.history().undo_steps, 1);
            session.call("undo", &json!({"revision":1})).unwrap();
            assert_eq!(
                serde_json::to_value(session.document.snapshot()).unwrap(),
                before
            );
            session.call("redo", &json!({"revision":2})).unwrap();
            assert!(session.renderer.is_none());
        }
    }
}
mod preset_tests {
    use super::*;
    #[test]
    fn portable_tools_guard_revision_validate_bindings_and_share_history_without_gpu() {
        let composition = veylvik_document::demo();
        let input = composition.output;
        let mut session = Session {
            document: Document::new(composition).unwrap(),
            saved: 0,
            asset_base: std::env::temp_dir(),
            saved_dir: None,
            renderer: None,
        };
        let loaded = session
            .call("get_preset", &json!({"starter":"liquid_shimmer"}))
            .unwrap();
        assert!(
            session
                .call(
                    "get_preset",
                    &json!({"starter":"liquid_shimmer","group":input})
                )
                .is_err()
        );
        let preset = loaded["preset"].clone();
        let args = json!({"revision":0,"preset":preset,"inputs":{"input_1":input},"dry_run":true});
        let dry = session.call("insert_preset", &args).unwrap();
        assert_eq!(session.document.revision(), 0);
        let mut invalid = args.clone();
        invalid["inputs"] = json!({});
        assert!(session.call("insert_preset", &invalid).is_err());
        invalid = args.clone();
        invalid["revision"] = json!(9);
        assert!(session.call("insert_preset", &invalid).is_err());
        let mut args = args;
        args["dry_run"] = json!(false);
        let inserted = session.call("insert_preset", &args).unwrap();
        assert_ne!(inserted["group"], dry["group"]);
        let exported = session
            .call("get_preset", &json!({"group":inserted["group"]}))
            .unwrap();
        assert_eq!(exported["preset"]["group"]["label"], "Liquid Shimmer");
        assert_eq!(session.document.history().undo_steps, 1);
        assert_eq!(session.document.snapshot().output, input);
        session.call("undo", &json!({"revision":1})).unwrap();
        assert!(session.document.snapshot().groups.is_empty());
        assert!(session.renderer.is_none());
    }
}
#[test]
fn tracking_solving_and_cut_out_take_the_same_arguments_on_both_servers() {
    // The live catalogue lives in a light crate that cannot reach the request types, so
    // its copies are held to them here: the same arguments, plus the live session, with
    // back_to capped at 0 there (Studio tracks backward to the first frame).
    let live = veylvik_control::live::catalog()["tools"]
        .as_array()
        .unwrap();
    for (name, own, required) in [
        (
            "track_surface",
            veylvik_track::request::Request::schema(),
            veylvik_track::request::Request::REQUIRED,
        ),
        (
            "solve_camera",
            veylvik_track::camera::Request::schema(),
            veylvik_track::camera::Request::REQUIRED,
        ),
        (
            "segment_subject",
            veylvik_segment::request::Request::schema(),
            veylvik_segment::request::Request::REQUIRED,
        ),
    ] {
        let tool = live.iter().find(|t| t["name"] == name).unwrap();
        let mut properties = tool["inputSchema"]["properties"].clone();
        let map = properties.as_object_mut().unwrap();
        map.remove("session");
        map.remove("revision");
        if let Some(back) = map.get_mut("back_to") {
            assert_eq!(back["maximum"], json!(0), "{name}");
            back.as_object_mut().unwrap().remove("maximum");
        }
        assert_eq!(
            properties, own,
            "{name}: live arguments differ from the request's"
        );
        let wanted: Vec<&str> = tool["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .filter(|r| !matches!(*r, "session" | "revision"))
            .collect();
        assert_eq!(wanted, required, "{name}");
    }
}
