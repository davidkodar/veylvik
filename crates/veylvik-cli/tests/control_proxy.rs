#![cfg(unix)]
//! The live relay answers the handshake itself, forwards each call over its own
//! connection, and flushes fragmented replies before stdin closes. Every wait is
//! bounded so a regression fails instead of hanging the test run.
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
fn directory() -> PathBuf {
    // Socket paths must fit in sun_path; a build's scratch TMPDIR can be longer.
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let directory = base.join(format!("veylvik-proxy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir(&directory).unwrap();
    directory
}
#[test]
fn live_stdio_relay_flushes_fragmented_responses_before_input_eof() {
    let directory = directory();
    let listener = UnixListener::bind(directory.join(veylvik_control::ipc::SOCKET_NAME)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (finish, finished) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => return Err(format!("relay never connected: {error}")),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut input = BufReader::new(socket.try_clone().unwrap());
        let mut line = String::new();
        input.read_line(&mut line).map_err(|e| e.to_string())?;
        if !line.contains("initialize") {
            return Err(format!("expected the relay handshake, got {line}"));
        }
        socket
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"veylvik-relay\",\"result\":{}}\n")
            .unwrap();
        line.clear();
        input.read_line(&mut line).map_err(|e| e.to_string())?;
        if !line.contains("notifications/initialized") {
            return Err(format!("expected initialized, got {line}"));
        }
        line.clear();
        input.read_line(&mut line).map_err(|e| e.to_string())?;
        if !line.contains("tools/call") || !line.contains("\"id\":2") {
            return Err(format!("expected the forwarded call, got {line}"));
        }
        socket.write_all(b"{\"jsonrpc\":\"2.0\",").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        socket
            .write_all(b"\"id\":2,\"result\":{\"content\":[],\"isError\":false}}\n")
            .unwrap();
        let _ = finished.recv_timeout(Duration::from_secs(5));
        Ok(())
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_veylvik"))
        .args(["mcp", "--studio"])
        .arg(&directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect","arguments":{}}}"#,
    ] {
        writeln!(input, "{message}").unwrap();
    }
    input.flush().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let (sender, responses) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut line = String::new();
            let result = output.read_line(&mut line).map(|_| line);
            if sender.send(result).is_err() {
                return;
            }
        }
    });
    let handshake = responses.recv_timeout(Duration::from_secs(5));
    let call = responses.recv_timeout(Duration::from_secs(10));
    let _ = finish.send(());
    drop(input);
    if call.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    let served = server.join().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    served.unwrap();
    let parse = |line: String| serde_json::from_str::<serde_json::Value>(&line).unwrap();
    let handshake = parse(handshake.expect("relay answers initialize itself").unwrap());
    assert_eq!(handshake["id"], 1);
    assert_eq!(handshake["result"]["serverInfo"]["name"], "veylvik");
    let call = parse(
        call.expect("the reply must arrive while stdin and the server stay open")
            .unwrap(),
    );
    assert_eq!(call["id"], 2);
    assert_eq!(call["result"]["isError"], false);
    assert!(status.success());
}
