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
    let mut document = veylvik_document::motion_demo();
    // Exercise persisted vector masks through both command transports as well
    // as the animated procedural masks already present in the demo.
    let path = veylvik_document::Node::new(
        "vector mask",
        veylvik_document::Operation::Path(veylvik_document::nodes::Path {
            geometry: veylvik_document::vector::PathMask::default(),
        }),
        &[],
    );
    let output = veylvik_document::Node::new(
        "vector composite",
        veylvik_document::Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", document.output), ("foreground", path.id)],
    );
    document.nodes.insert(path.id, path);
    document.output = output.id;
    document.nodes.insert(output.id, output);
    let map = veylvik_document::Node::new(
        "distortion map",
        veylvik_document::Operation::FractalNoise(veylvik_document::nodes::FractalNoise {
            seed: 42,
            scale: 32.0.into(),
            octaves: 3.0.into(),
            roughness: 0.5.into(),
            evolution: 0.25.into(),
        }),
        &[],
    );
    let distortion = veylvik_document::Node::new(
        "distortion",
        veylvik_document::Operation::Displace(veylvik_document::nodes::Displace {
            x: 8.5.into(),
            y: (-4.25).into(),
        }),
        &[("image", document.output), ("map", map.id)],
    );
    document.nodes.insert(map.id, map);
    document.output = distortion.id;
    document.nodes.insert(distortion.id, distortion);
    let mask = veylvik_document::Node::new(
        "procedural mask",
        veylvik_document::Operation::ChannelMask(veylvik_document::nodes::ChannelMask {
            channel: veylvik_document::MaskChannel::Luminance,
        }),
        &[(
            "image",
            document
                .nodes
                .values()
                .find(|n| {
                    matches!(
                        n.operation,
                        veylvik_document::Operation::FractalNoise(
                            veylvik_document::nodes::FractalNoise { .. }
                        )
                    )
                })
                .unwrap()
                .id,
        )],
    );
    let masked = veylvik_document::Node::new(
        "masked distortion",
        veylvik_document::Operation::Mask(veylvik_document::nodes::Mask {}),
        &[("image", document.output), ("mask", mask.id)],
    );
    document.nodes.insert(mask.id, mask);
    document.output = masked.id;
    document.nodes.insert(masked.id, masked);
    let colorized = veylvik_document::Node::new(
        "Color mapping",
        veylvik_document::Operation::Colorize(veylvik_document::nodes::Colorize {
            low: [0.01, 0.02, 0.1, 0.2],
            high: [1.5, 0.3, 0.02, 0.9],
        }),
        &[("image", document.output)],
    );
    document.output = colorized.id;
    document.nodes.insert(colorized.id, colorized);
    document.save(&root.0.join("motion.veylvik")).unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nodes","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"render","arguments":{"revision":0,"path":"first.png","ticks":0,"rate":1}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"render","arguments":{"revision":0,"path":"second.png","ticks":1,"rate":1}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"render_sequence","arguments":{"revision":0,"path":"mcp-sequence","start":0,"end":2}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"render_sequence","arguments":{"revision":0,"path":"mcp-exr","start":0,"end":2,"format":"exr"}}}),
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
    assert_eq!(replies.len(), 6);
    for reply in &replies {
        assert!(
            reply.get("error").is_none() && reply["result"]["isError"] != true,
            "{reply}"
        );
    }
    let catalog = replies[1]["result"]["structuredContent"]["nodes"]
        .as_array()
        .unwrap();
    for kind in [
        "affine",
        "ellipse",
        "mask",
        "path",
        "displace",
        "fractal_noise",
        "channel_mask",
    ] {
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
    let result = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args([
            "render-sequence",
            "motion.veylvik",
            "cli-sequence",
            "0",
            "2",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["manifest"]["status"], "complete");
    for index in 0..3 {
        let name = format!("frame-{index:06}.png");
        assert_eq!(
            fs::read(root.0.join("cli-sequence").join(&name)).unwrap(),
            fs::read(root.0.join("mcp-sequence").join(&name)).unwrap()
        );
    }
    assert_eq!(
        fs::read(root.0.join("first.png")).unwrap(),
        fs::read(root.0.join("cli-sequence/frame-000000.png")).unwrap()
    );
    let result = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args([
            "render-sequence",
            "motion.veylvik",
            "cli-exr",
            "0",
            "2",
            "--format",
            "exr",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["manifest"]["format"], "exr");
    assert_eq!(report["manifest"]["color_space"], "linear_rec709");
    assert_eq!(report["manifest"]["alpha"], "premultiplied");
    for index in 0..3 {
        let name = format!("frame-{index:06}.exr");
        assert_eq!(
            fs::read(root.0.join("cli-exr").join(&name)).unwrap(),
            fs::read(root.0.join("mcp-exr").join(&name)).unwrap()
        );
    }
}
