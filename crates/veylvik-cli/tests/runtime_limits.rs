use std::io::Write;
use std::process::{Command, Stdio};
use veylvik_document::*;
#[test]
#[ignore = "requires Vulkan"]
fn cli_and_headless_mcp_honor_host_budget_and_cpu_inspection_stays_available() {
    let root = std::env::temp_dir().join(format!("veylvik-budget-{}", NodeId::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("source.veylvik");
    let mut doc = blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.25, 0.5, 1.0, 0.5],
        });
    let blur = Node::new(
        "Blur",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 1.0.into(),
            filter: Default::default(),
        }),
        &[("image", doc.output)],
    );
    doc.output = blur.id;
    doc.nodes.insert(blur.id, blur);
    doc.save(&path).unwrap();
    let original = std::fs::read(&path).unwrap();
    let binary = env!("CARGO_BIN_EXE_veylvik");
    for budget in ["not-a-number", "64"] {
        let output = root.join(format!("{budget}.png"));
        let result = Command::new(binary)
            .env("VEYLVIK_GPU_BUDGET_MIB", budget)
            .arg("render")
            .arg(&path)
            .arg(&output)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(if budget == "64" {
                "budget"
            } else {
                "VEYLVIK_GPU_BUDGET_MIB"
            })
        );
        assert!(!output.exists());
    }
    let check = Command::new(binary)
        .env("VEYLVIK_GPU_BUDGET_MIB", "invalid")
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    assert!(check.status.success());
    let mut large = doc.clone();
    large.width = 4096;
    large.height = 4096;
    let large_path = root.join("large.veylvik");
    large.save(&large_path).unwrap();
    let checked = Command::new(binary)
        .env("VEYLVIK_GPU_BUDGET_MIB", "64")
        .arg("check")
        .arg(&large_path)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert!(report["working_bytes"].as_u64().unwrap() > 512 * 1024 * 1024);
    assert_eq!(report["source_bytes"], 0);
    let mut rendered = None;
    for budget in ["128", "512"] {
        let output = root.join(format!("{budget}.png"));
        let result = Command::new(binary)
            .env("VEYLVIK_GPU_BUDGET_MIB", budget)
            .arg("render")
            .arg(&path)
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let bytes = std::fs::read(output).unwrap();
        if let Some(previous) = rendered {
            assert_eq!(bytes, previous);
        }
        rendered = Some(bytes);
    }
    let mut image_doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    image_doc
        .nodes
        .get_mut(&image_doc.output)
        .unwrap()
        .operation = Operation::Image(veylvik_document::nodes::Image {
        asset: "128.png".into(),
    });
    let image_path = root.join("image.veylvik");
    image_doc.save(&image_path).unwrap();
    let checked = Command::new(binary)
        .arg("check")
        .arg(&image_path)
        .output()
        .unwrap();
    assert!(checked.status.success());
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["source_bytes"], 1920 * 1080 * 16);
    std::fs::remove_file(root.join("128.png")).unwrap();
    assert!(
        !Command::new(binary)
            .arg("check")
            .arg(&image_path)
            .output()
            .unwrap()
            .status
            .success()
    );
    let mut process = Command::new(binary)
        .env("VEYLVIK_GPU_BUDGET_MIB", "128")
        .arg("mcp")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"preview_image","arguments":{"revision":0,"ticks":0,"rate":1,"max_edge":16}}}),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"diagnostics","arguments":{}}}),
    ];
    let mut input = process.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success());
    let replies: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[1]["result"]["isError"], false);
    let stats = &replies[2]["result"]["structuredContent"]["snapshot"];
    assert_eq!(stats["budget_bytes"], 128 * 1024 * 1024);
    assert_eq!(stats["frame_cache_limit_bytes"], 16 * 1024 * 1024);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_dir_all(root).unwrap();
}
