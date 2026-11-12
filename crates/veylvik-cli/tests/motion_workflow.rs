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
#[ignore = "requires Vulkan"]
fn animated_mask_composition_matches_cli_and_mcp() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("veylvik-motion-{}-{unique}", std::process::id());
    let root = Scratch(std::env::temp_dir().join(name));
    fs::create_dir(&root.0).unwrap();
    veylvik_document::motion_demo()
        .save(&root.0.join("motion.veylvik"))
        .unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nodes","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"render","arguments":{"revision":0,"path":"first.png","ticks":0,"rate":1}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"render","arguments":{"revision":0,"path":"second.png","ticks":1,"rate":1}}}),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["mcp", "motion.veylvik"])
        .current_dir(&root.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(replies.len(), 4);
    for reply in &replies {
        assert!(
            reply.get("error").is_none() && reply["result"]["isError"] != true,
            "{reply}"
        );
    }
    let catalog = replies[1]["result"]["structuredContent"]["nodes"]
        .as_array()
        .unwrap();
    for kind in ["affine", "ellipse", "mask"] {
        assert!(
            catalog
                .iter()
                .any(|node| node["descriptor"]["type_id"] == kind)
        );
    }
    let result = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["render", "motion.veylvik", "cli.png", "24", "24"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(root.0.join("cli.png")).unwrap(),
        fs::read(root.0.join("second.png")).unwrap()
    );
    assert_ne!(
        fs::read(root.0.join("first.png")).unwrap(),
        fs::read(root.0.join("second.png")).unwrap()
    );
}
