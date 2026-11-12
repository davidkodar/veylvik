#![cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[test]
fn live_stdio_proxy_flushes_fragmented_responses_before_input_eof() {
    let directory = std::env::temp_dir().join(format!(
        "veylvik-proxy-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let listener = UnixListener::bind(directory.join(veylvik_control::ipc::SOCKET_NAME)).unwrap();
    let (finish, finished) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut input = BufReader::new(socket.try_clone().unwrap());
        let mut request = String::new();
        input.read_line(&mut request).unwrap();
        assert!(request.contains("initialize"));
        socket.write_all(b"{\"jsonrpc\":\"2.0\",").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        socket.write_all(b"\"id\":1,\"result\":{}}\n").unwrap();
        let _ = finished.recv_timeout(Duration::from_secs(5));
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
    writeln!(
        input,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}}"
    )
    .unwrap();
    input.flush().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let (sender, response) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = output.read_line(&mut line);
        let _ = sender.send(result.map(|_| line));
    });
    let result = response.recv_timeout(Duration::from_secs(3));
    let _ = finish.send(());
    drop(input);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    server.join().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    let line = result
        .expect("response must arrive while stdin and server stay open")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"],
        1
    );
    assert!(status.success());
}
