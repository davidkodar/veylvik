use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn outline_conversion_matches_mcp_and_protects_existing_files() {
    let root = Scratch(std::env::temp_dir().join(format!(
            "veylvik-text-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir(&root.0).unwrap();
    fs::write(root.0.join("font.ttf"), epaint_default_fonts::UBUNTU_LIGHT).unwrap();
    let cli = |output: &str, text: &str| {
        Command::new(env!("CARGO_BIN_EXE_veylvik"))
            .args([
                "outline-text",
                "font.ttf",
                text,
                output,
                "--canvas",
                "640",
                "128",
                "--origin",
                "12",
                "80",
            ])
            .current_dir(&root.0)
            .output()
            .unwrap()
    };
    let result = cli("title.veylvik", "Veylvik ffi Å");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(root.0.join("title.veylvik")).unwrap();
    let document = veylvik_document::Composition::load(&root.0.join("title.veylvik")).unwrap();
    assert!(
        veylvik_render::Plan::compile(&document)
            .unwrap()
            .required_assets()
            .is_empty()
    );
    let expected = serde_json::to_value(&document.nodes[&document.output].operation).unwrap();
    assert!(!cli("title.veylvik", "replace").status.success());
    assert_eq!(fs::read(root.0.join("title.veylvik")).unwrap(), original);
    assert!(!cli("font.ttf", "replace").status.success());
    assert_eq!(
        fs::read(root.0.join("font.ttf")).unwrap(),
        epaint_default_fonts::UBUNTU_LIGHT
    );
    assert!(!cli("blank.veylvik", "   ").status.success());
    assert!(!root.0.join("blank.veylvik").exists());
    assert!(!cli("missing.veylvik", "\u{10ffff}").status.success());
    assert!(!root.0.join("missing.veylvik").exists());
    let mut child = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["mcp", "title.veylvik"])
        .current_dir(&root.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"outline_text","arguments":{"revision":0,"font":"font.ttf","text":"Veylvik ffi Å","origin":[12,80]}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"inspect","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"outline_text","arguments":{"revision":1,"font":"font.ttf","text":"stale"}}}),
    ] {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[1]["result"]["isError"], false);
    assert_eq!(
        replies[1]["result"]["structuredContent"]["operation"],
        expected
    );
    assert_eq!(replies[2]["result"]["structuredContent"]["revision"], 0);
    assert_eq!(replies[3]["result"]["isError"], true);
    // The saved composition needs neither the original text nor font to render.
    fs::remove_file(root.0.join("font.ttf")).unwrap();
    veylvik_render::Plan::compile(
        &veylvik_document::Composition::load(&root.0.join("title.veylvik")).unwrap(),
    )
    .unwrap();
}
