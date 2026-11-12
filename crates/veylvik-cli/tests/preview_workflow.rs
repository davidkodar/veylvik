use base64::Engine;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use veylvik_document::*;
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn mcp_returns_inline_previews_without_mutating_or_writing_the_project() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("veylvik-preview-{}", NodeId::new_v4())));
    fs::create_dir(&scratch.0).unwrap();
    let mut doc = blank(32, 18, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation = Operation::Gradient {
        mode: GradientMode::Linear,
        start_x: 0.0.into(),
        start_y: 0.0.into(),
        end_x: 31.0.into(),
        end_y: 0.0.into(),
    };
    let color = Node::new(
        "Color",
        Operation::Colorize {
            low: [1.0, 0.0, 0.0, 0.5],
            high: [0.0, 0.0, 1.0, 0.25],
        },
        &[("image", source)],
    );
    doc.output = color.id;
    doc.nodes.insert(color.id, color);
    let project = scratch.0.join("project.veylvik");
    doc.save(&project).unwrap();
    let original = fs::read(&project).unwrap();
    let mut messages = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ];
    for (i,(name,args)) in [
        ("diagnostics", json!({})),
        ("preview_image", json!({"revision":0,"ticks":0,"rate":24,"max_edge":0})),
        ("preview_image", json!({"revision":1,"ticks":0,"rate":24})),
        ("diagnostics", json!({})),
        ("preview_image", json!({"revision":0,"ticks":0,"rate":24})),
        ("preview_image", json!({"revision":0,"ticks":0,"rate":24,"max_edge":5,"region":{"x":3,"y":2,"width":10,"height":6}})),
        ("preview_image", json!({"revision":0,"ticks":0,"rate":24,"max_edge":8,"node":source})),
        ("inspect", json!({})),
        ("render", json!({"revision":0,"ticks":0,"rate":24,"path":"full.png"})),
    ].into_iter().enumerate() {
        messages.push(json!({"jsonrpc":"2.0","id":i+2,"method":"tools/call","params":{"name":name,"arguments":args}}));
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["mcp", "project.veylvik"])
        .current_dir(&scratch.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let result = |id: u64| &replies.iter().find(|reply| reply["id"] == id).unwrap()["result"];
    for id in [2, 5] {
        assert_eq!(result(id)["structuredContent"]["initialized"], false);
    }
    for id in [3, 4] {
        assert_eq!(result(id)["isError"], true);
    }
    for (id, width, height) in [(6, 32, 18), (7, 5, 3), (8, 8, 5)] {
        let result = result(id);
        assert_eq!(result["isError"], false, "{result}");
        assert_eq!(result["content"].as_array().unwrap().len(), 2);
        assert_eq!(result["content"][1]["type"], "image");
        assert_eq!(result["content"][1]["mimeType"], "image/png");
        let metadata = &result["structuredContent"];
        assert_eq!(metadata["width"], width);
        assert_eq!(metadata["height"], height);
        assert_eq!(metadata["revision"], 0);
        assert_eq!(
            metadata["node"],
            json!(if id == 8 { source } else { doc.output })
        );
        assert_eq!(metadata["time"], json!({"ticks":0,"rate":24}));
        assert!(metadata.to_string().len() < 1024);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result["content"][1]["data"].as_str().unwrap())
            .unwrap();
        let reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .unwrap();
        assert_eq!(
            (reader.info().width, reader.info().height),
            (width as u32, height as u32)
        );
        if id == 6 {
            assert_eq!(bytes, fs::read(scratch.0.join("full.png")).unwrap());
        }
        if id == 7 {
            assert_eq!(
                metadata["source_region"],
                json!({"x":3,"y":2,"width":10,"height":6})
            );
        }
    }
    assert_eq!(
        result(9)["structuredContent"]["composition"],
        serde_json::to_value(&doc).unwrap()
    );
    assert_eq!(result(9)["structuredContent"]["history"]["undo_steps"], 0);
    assert_eq!(fs::read(&project).unwrap(), original);
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
}
