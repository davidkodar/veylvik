//! Exercise the installed protocol boundary rather than calling adapter internals.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(root: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
#[ignore = "requires Vulkan; run cargo test --workspace -- --ignored"]
fn image_mcp_save_as_and_cli_crop_agree() {
    for exr in [false, true] {
        image_workflow(exr);
    }
}
fn image_workflow(exr: bool) {
    let root =
        Temp(std::env::temp_dir().join(format!("veylvik-workflow-{}-{exr}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("project")).unwrap();
    fs::create_dir(root.0.join("moved")).unwrap();
    let source = root.0.join("source.png");
    let mut encoder = png::Encoder::new(fs::File::create(&source).unwrap(), 16, 12);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let pixels: Vec<u8> = (0..12u8)
        .flat_map(|y| (0..16u8).flat_map(move |x| [x * 16, y * 20, 180, 128]))
        .collect();
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
    let source_name = if exr { "source.exr" } else { "source.png" };
    if exr {
        let image = veylvik_io::load_png(&source, 128 * 1024 * 1024).unwrap();
        let pixels: Vec<_> = image
            .pixels()
            .iter()
            .map(|p| [p[0] * 8.0, -p[1], p[2], p[3]])
            .collect();
        veylvik_io::save_frame(
            &root.0.join(source_name),
            image.width(),
            image.height(),
            &pixels,
        )
        .unwrap();
    }
    let source = root.0.join(source_name);
    let original = fs::read(&source).unwrap();
    run(&root.0, &["import", source_name, "project/image.veylvik"]);
    let doc: Value =
        serde_json::from_slice(&fs::read(root.0.join("project/image.veylvik")).unwrap()).unwrap();
    let image = doc["output"].as_str().unwrap();
    assert_eq!(
        doc["nodes"][image]["operation"]["asset"],
        format!("../{source_name}")
    );
    let blur = "39ea450e-4aad-42a3-9138-05934aa9035e";
    let commands = json!([
        {"command":"put","node":{"id":blur,"version":1,"label":"Blur","operation":{"type":"blur","radius":{"kind":"constant","value":2}},"inputs":{"image":image}}},
        {"command":"output","id":blur}
    ]);
    let mut messages = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ];
    for (i,(name,args)) in [
        ("apply",json!({"revision":0,"commands":commands})),
        ("save",json!({"revision":1,"path":"moved/effect.veylvik"})),
        ("render",json!({"revision":1,"path":"mcp.png","ticks":0,"rate":1,"region":{"x":1,"y":1,"width":8,"height":6}})),
        ("render",json!({"revision":1,"path":"mcp.exr","ticks":0,"rate":1,"region":{"x":1,"y":1,"width":8,"height":6}})),
    ].into_iter().enumerate() { messages.push(json!({"jsonrpc":"2.0","id":i+2,"method":"tools/call","params":{"name":name,"arguments":args}})); }
    let mut child = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["mcp", "project/image.veylvik"])
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
    assert_eq!(replies.len(), 5);
    for reply in &replies {
        assert!(
            reply.get("error").is_none() && reply["result"]["isError"] != true,
            "{reply}"
        );
    }
    run(&root.0, &["check", "moved/effect.veylvik"]);
    run(
        &root.0,
        &[
            "render",
            "moved/effect.veylvik",
            "cli.png",
            "--crop",
            "1",
            "1",
            "8",
            "6",
        ],
    );
    assert_eq!(
        fs::read(root.0.join("mcp.png")).unwrap(),
        fs::read(root.0.join("cli.png")).unwrap()
    );
    run(
        &root.0,
        &[
            "render",
            "moved/effect.veylvik",
            "preview.png",
            "--node",
            image,
            "--crop",
            "1",
            "1",
            "8",
            "6",
        ],
    );
    assert_ne!(
        fs::read(root.0.join("preview.png")).unwrap(),
        fs::read(root.0.join("cli.png")).unwrap()
    );
    run(
        &root.0,
        &[
            "render",
            "moved/effect.veylvik",
            "cli.exr",
            "--crop",
            "1",
            "1",
            "8",
            "6",
        ],
    );
    let exr = fs::read(root.0.join("cli.exr")).unwrap();
    assert_eq!(&exr[..4], &20000630_u32.to_le_bytes());
    assert_eq!(exr, fs::read(root.0.join("mcp.exr")).unwrap());
    let refused = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["render", "moved/effect.veylvik", source_name])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("input image"));
    assert_eq!(fs::read(&source).unwrap(), original);
    let collected = run(&root.0, &["collect", "moved/effect.veylvik", "collection"]);
    assert_eq!(collected["assets"], 1);
    assert_eq!(collected["bytes"], original.len());
    let existing = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["collect", "moved/effect.veylvik", "collection"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(!existing.status.success());
    fs::rename(root.0.join("collection"), root.0.join("portable")).unwrap();
    fs::remove_file(&source).unwrap();
    fs::remove_dir_all(root.0.join("moved")).unwrap();
    run(&root.0, &["check", "portable/project.veylvik"]);
    run(
        &root.0,
        &[
            "render",
            "portable/project.veylvik",
            "portable.exr",
            "--crop",
            "1",
            "1",
            "8",
            "6",
        ],
    );
    assert_eq!(fs::read(root.0.join("portable.exr")).unwrap(), exr);
}
