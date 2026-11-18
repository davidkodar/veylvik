//! Local transport: one private Unix socket per editor, owned only by this user.
//! A stale socket is reclaimed, a live one is never replaced or adopted.
use crate::{MAX_MESSAGE_BYTES, Protocol, ToolOutput};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
pub const SOCKET_NAME: &str = "control.sock";
/// `sun_path` holds 108 bytes including the terminating NUL.
pub const MAX_SOCKET_PATH_BYTES: usize = 107;
/// Overrides where Studio listens and where the relay connects.
pub const SOCKET_ENV: &str = "VEYLVIK_CONTROL_SOCKET";
/// `$VEYLVIK_CONTROL_SOCKET`, else `$XDG_RUNTIME_DIR/veylvik/control.sock`.
/// There is no shared-temp fallback: without a runtime directory live control needs an explicit path.
pub fn default_socket() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(SOCKET_ENV).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.is_dir())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("XDG_RUNTIME_DIR is not set; set {SOCKET_ENV} to a socket path"),
            )
        })?;
    Ok(runtime.join("veylvik").join(SOCKET_NAME))
}
/// Accepts a socket path, or a directory holding `control.sock`.
pub fn resolve_socket(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(SOCKET_NAME)
    } else {
        path.to_path_buf()
    }
}
fn check_length(path: &Path) -> io::Result<()> {
    let length = path.as_os_str().len();
    if length > MAX_SOCKET_PATH_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "control socket path {} is {length} bytes; Unix sockets allow at most {MAX_SOCKET_PATH_BYTES}. Use a shorter directory, such as one under $XDG_RUNTIME_DIR",
                path.display()
            ),
        ));
    }
    Ok(())
}
fn current_uid() -> io::Result<u32> {
    Ok(fs::metadata("/proc/self")?.uid())
}
/// Create the directory 0700, or accept an existing one only if this user owns it privately.
/// Returns whether it was created here.
fn private_directory(directory: &Path) -> io::Result<bool> {
    match fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => return Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != current_uid()? || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a directory owned by you with mode 0700",
                directory.display()
            ),
        ));
    }
    Ok(false)
}
fn listening(path: &Path) -> bool {
    UnixStream::connect(path).is_ok()
}
/// Bind `path`, replacing only a socket nobody answers on. If another editor is
/// listening there, bind `control-<pid>.sock` beside it instead.
fn claim(path: &Path) -> io::Result<(UnixListener, PathBuf)> {
    check_length(path)?;
    let path = if listening(path) {
        let own = path.with_file_name(format!("control-{}.sock", std::process::id()));
        check_length(&own)?;
        if listening(&own) {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("{} is already in use", own.display()),
            ));
        }
        own
    } else {
        path.to_path_buf()
    };
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(&path)?,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{} exists and is not a socket; refusing to replace it",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // The private directory guards the moment before the socket's own mode is narrowed.
    let listener = UnixListener::bind(&path)?;
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok((listener, path))
}
pub struct Request {
    pub name: String,
    pub arguments: Value,
    reply: mpsc::SyncSender<Result<ToolOutput, String>>,
}
impl Request {
    pub fn respond(self, result: Result<Value, String>) {
        self.respond_output(result.map(ToolOutput::from));
    }
    pub fn respond_output(self, result: Result<ToolOutput, String>) {
        let _ = self.reply.send(result);
    }
}
pub struct Server {
    requests: mpsc::Receiver<Request>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    endpoint: Endpoint,
}
struct Endpoint {
    socket: PathBuf,
    socket_id: (u64, u64),
    /// Set only when this server created the directory; it is removed again if left empty.
    directory: Option<(PathBuf, (u64, u64))>,
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        if identity(&self.socket).ok() == Some(self.socket_id) {
            let _ = fs::remove_file(&self.socket);
        }
        if let Some((directory, id)) = &self.directory
            && identity(directory).ok() == Some(*id)
        {
            let _ = fs::remove_dir(directory);
        }
    }
}
fn identity(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}
impl Server {
    /// Listen on `socket` (see [`claim`]), creating its parent directory 0700 if needed.
    /// One client is served at a time; requests are dispatched to the host thread.
    pub fn bind(
        socket: &Path,
        catalog: Value,
        wake: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let directory = socket
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "control socket needs a directory",
                )
            })?;
        check_length(socket)?;
        let created = private_directory(directory)?;
        let (listener, socket) = match claim(socket) {
            // Another editor may remove an empty directory it created between the check and the bind.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                private_directory(directory)?;
                claim(socket)?
            }
            result => result?,
        };
        let endpoint = Endpoint {
            socket_id: identity(&socket)?,
            directory: created
                .then(|| identity(directory).map(|id| (directory.to_path_buf(), id)))
                .transpose()?,
            socket,
        };
        listener.set_nonblocking(true)?;
        let (sender, requests) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("veylvik-control".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = serve_client(stream, &catalog, &sender, &stopping, &wake);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            requests,
            stop,
            thread: Some(thread),
            endpoint,
        })
    }
    /// Where this editor actually listens, which differs from the request when another editor held it.
    pub fn path(&self) -> &Path {
        &self.endpoint.socket
    }
    pub fn try_recv(&self) -> Option<Request> {
        self.requests.try_recv().ok()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn serve_client(
    stream: UnixStream,
    catalog: &Value,
    requests: &mpsc::SyncSender<Request>,
    stop: &AtomicBool,
    wake: &impl Fn(),
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut writer = stream.try_clone()?;
    let mut input = BufReader::new(stream);
    let mut protocol = Protocol::default();
    while let Some(bytes) = read_message(&mut input, stop)? {
        let response = match serde_json::from_slice(&bytes) {
            Ok(message) => protocol.handle_output(message, catalog, |name, arguments| {
                let (reply, result) = mpsc::sync_channel(1);
                requests
                    .try_send(Request {
                        name: name.into(),
                        arguments: arguments.clone(),
                        reply,
                    })
                    .map_err(|_| "editor request queue unavailable".to_owned())?;
                wake();
                loop {
                    if stop.load(Ordering::Acquire) {
                        return Err("editor session closed".into());
                    }
                    match result.recv_timeout(Duration::from_millis(100)) {
                        Ok(result) => return result,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => return Err("editor session closed".into()),
                    }
                }
            }),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"invalid JSON"}}),
            ),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut writer, &response)?;
            writeln!(writer)?;
            writer.flush()?;
        }
    }
    Ok(())
}
fn read_message(input: &mut impl BufRead, stop: &AtomicBool) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let limit = (MAX_MESSAGE_BYTES + 1 - bytes.len()) as u64;
        let result = io::Read::take(&mut *input, limit).read_until(b'\n', &mut bytes);
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP message exceeds 4 MiB",
            ));
        }
        match result {
            Ok(0) => return Ok((!bytes.is_empty()).then_some(bytes)),
            Ok(_) if bytes.last() == Some(&b'\n') => return Ok(Some(bytes)),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
}

/// How long a call waits for Studio to accept it; another client may be holding the session.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// Stdio MCP server for the open editor. It answers the handshake and tool list itself, so an
/// assistant can start it before Studio. Each tool call opens its own connection to the socket,
/// so restarts are picked up and Studio's single client slot is free between calls.
/// This program's file as it was when the relay started, to notice an update.
struct Build {
    path: PathBuf,
    modified: Option<std::time::SystemTime>,
}
impl Build {
    fn now() -> Option<Self> {
        let path = std::env::current_exe().ok()?;
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
        Some(Self { path, modified })
    }
    /// Whether the program was replaced since: its tool list is then out of date.
    fn replaced(&self) -> bool {
        fs::metadata(&self.path).and_then(|m| m.modified()).ok() != self.modified
    }
}
const UPDATED: &str = "Note: Veylvik was updated after this MCP server started, so its tool list may be out of date. Restart the MCP server (reconnect it in your client) to see the current tools.";
/// `reply` with the update note added to its text content; unchanged when it is not a tool result.
fn with_note(reply: Vec<u8>) -> Vec<u8> {
    let Ok(mut message) = serde_json::from_slice::<Value>(&reply) else {
        return reply;
    };
    let Some(content) = message["result"]["content"].as_array_mut() else {
        return reply;
    };
    content.push(json!({"type":"text","text":UPDATED}));
    let mut bytes = serde_json::to_vec(&message).unwrap_or(reply);
    bytes.push(b'\n');
    bytes
}
pub fn relay(socket: &Path, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let build = Build::now();
    let mut protocol = Protocol::default();
    let stop = AtomicBool::new(false);
    while let Some(bytes) = read_message(&mut input, &stop)? {
        let response = match serde_json::from_slice::<Value>(&bytes) {
            Ok(message)
                if protocol.is_ready()
                    && message["method"] == "tools/call"
                    && message
                        .get("id")
                        .is_some_and(|id| id.is_string() || id.is_number()) =>
            {
                match call_studio(socket, &message) {
                    Ok(reply) => {
                        let reply = if build.as_ref().is_some_and(Build::replaced) {
                            with_note(reply)
                        } else {
                            reply
                        };
                        output.write_all(&reply)?;
                        output.flush()?;
                        continue;
                    }
                    Err(error) => Some(
                        json!({"jsonrpc":"2.0","id":message["id"],"result":{"content":[{"type":"text","text":error}],"isError":true}}),
                    ),
                }
            }
            Ok(message) => protocol.handle_output(message, crate::live::catalog(), |_, _| {
                Err("Veylvik Studio is not connected".into())
            }),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"invalid JSON"}}),
            ),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            writeln!(output)?;
            output.flush()?;
        }
    }
    Ok(())
}
fn not_running(socket: &Path) -> String {
    format!(
        "Veylvik Studio is not running (nothing listens on {}). Start Studio and try again.",
        socket.display()
    )
}
/// One connection per call: handshake, forward the request unchanged, return Studio's reply line.
fn call_studio(socket: &Path, message: &Value) -> Result<Vec<u8>, String> {
    check_length(socket).map_err(|error| error.to_string())?;
    let metadata = match fs::symlink_metadata(socket) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(not_running(socket)),
        Err(error) => return Err(format!("{}: {error}", socket.display())),
    };
    let uid = current_uid().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(format!(
            "{} is not a control socket owned by you; refusing to connect",
            socket.display()
        ));
    }
    let stream = UnixStream::connect(socket).map_err(|error| match error.kind() {
        // A socket left behind by an editor that did not shut down cleanly.
        io::ErrorKind::ConnectionRefused => not_running(socket),
        _ => format!("cannot connect to {}: {error}", socket.display()),
    })?;
    let setup = |stream: &UnixStream| -> io::Result<(UnixStream, BufReader<UnixStream>)> {
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
        Ok((stream.try_clone()?, BufReader::new(stream.try_clone()?)))
    };
    let (mut writer, mut reader) = setup(&stream).map_err(|error| error.to_string())?;
    let lost = |error: io::Error| format!("lost the connection to Veylvik Studio: {error}");
    writeln!(
        writer,
        "{}",
        json!({"jsonrpc":"2.0","id":"veylvik-relay","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"veylvik-relay","version":env!("CARGO_PKG_VERSION")}}})
    )
    .map_err(lost)?;
    match read_reply(&mut reader, Instant::now() + HANDSHAKE_TIMEOUT) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::TimedOut => {
            return Err(
                "Veylvik Studio is busy with another MCP client; try again when it disconnects."
                    .into(),
            );
        }
        Err(error) => return Err(lost(error)),
    }
    writeln!(
        writer,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .map_err(lost)?;
    writeln!(writer, "{message}").map_err(lost)?;
    read_reply(&mut reader, Instant::now() + CALL_TIMEOUT).map_err(|error| {
        format!(
            "{}; the call may not have completed, so inspect before retrying",
            lost(error)
        )
    })
}
fn read_reply(input: &mut impl BufRead, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let limit = (MAX_MESSAGE_BYTES + 1 - bytes.len()) as u64;
        match io::Read::take(&mut *input, limit).read_until(b'\n', &mut bytes) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(_) if bytes.last() == Some(&b'\n') => return Ok(bytes),
            Ok(_) if bytes.len() > MAX_MESSAGE_BYTES => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reply exceeds 4 MiB",
                ));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                if Instant::now() >= deadline {
                    return Err(io::ErrorKind::TimedOut.into());
                }
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_updated_program_adds_a_note_to_tool_results_only() {
        let reply = br#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{}"}],"isError":false}}
"#;
        let noted = with_note(reply.to_vec());
        assert!(noted.ends_with(b"\n"));
        let message: Value = serde_json::from_slice(&noted).unwrap();
        assert_eq!(message["result"]["content"][1]["text"], UPDATED);
        assert_eq!(message["id"], 3);
        let other = br#"{"jsonrpc":"2.0","id":3,"error":{"code":1}}"#.to_vec();
        assert_eq!(with_note(other.clone()), other);
        let build = Build::now().unwrap();
        assert!(!build.replaced());
        let moved = Build {
            path: build.path.with_extension("gone"),
            modified: build.modified,
        };
        assert!(moved.replaced());
    }
    use std::os::unix::fs::PermissionsExt;
    /// A fresh path per test, under the runtime directory: a build's scratch TMPDIR can exceed `sun_path`.
    fn directory() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let directory = base.join(format!(
            "veylvik-control-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&directory);
        directory
    }
    #[test]
    fn private_transport_preserves_fragmented_messages_and_dispatches_on_host() {
        let directory = directory();
        let (wake, awakened) = mpsc::channel();
        let server = Server::bind(
            &directory.join(SOCKET_NAME),
            json!({"tools":[]}),
            move || {
                let _ = wake.send(());
            },
        )
        .unwrap();
        assert_eq!(server.path(), directory.join(SOCKET_NAME));
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.join(SOCKET_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // A second editor never takes over a live socket; it listens beside it.
        let second = Server::bind(&directory.join(SOCKET_NAME), json!({}), || {}).unwrap();
        assert_eq!(
            second.path(),
            directory.join(format!("control-{}.sock", std::process::id()))
        );
        drop(second);
        let path = directory.join(SOCKET_NAME);
        let client = std::thread::spawn(move || {
            let mut socket = UnixStream::connect(path).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut input = BufReader::new(socket.try_clone().unwrap());
            socket.write_all(b"{\"jsonrpc\":\"2.0\",").unwrap();
            std::thread::sleep(Duration::from_millis(250));
            socket
                .write_all(b"\"id\":1,\"method\":\"initialize\"}\n")
                .unwrap();
            let mut line = String::new();
            input.read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["result"]["protocolVersion"],
                "2025-11-25"
            );
            writeln!(
                socket,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .unwrap();
            writeln!(socket, "{}", json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect","arguments":{"test":42}}})).unwrap();
            line.clear();
            input.read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["result"]["structuredContent"]["revision"],
                7
            );
            writeln!(socket, "{}", json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"preview_image","arguments":{}}})).unwrap();
            line.clear();
            input.read_line(&mut line).unwrap();
            let result: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                result["result"]["content"][1],
                json!({"type":"image","mimeType":"image/png","data":"UE5H"})
            );
            assert_eq!(result["result"]["structuredContent"]["revision"], 7);
        });
        awakened.recv_timeout(Duration::from_secs(5)).unwrap();
        let request = server.try_recv().unwrap();
        assert_eq!(request.name, "inspect");
        assert_eq!(request.arguments["test"], 42);
        request.respond(Ok(json!({"revision":7})));
        awakened.recv_timeout(Duration::from_secs(5)).unwrap();
        let request = server.try_recv().unwrap();
        assert_eq!(request.name, "preview_image");
        request.respond_output(Ok(ToolOutput::png(json!({"revision":7}), b"PNG")));
        client.join().unwrap();
        drop(server);
        assert!(!directory.exists());
    }
    #[test]
    fn framing_is_bounded_and_cleanup_preserves_replaced_paths() {
        let stop = AtomicBool::new(false);
        let input = vec![b'x'; MAX_MESSAGE_BYTES + 2];
        assert!(read_message(&mut std::io::Cursor::new(input), &stop).is_err());
        let directory = directory();
        let socket = directory.join(SOCKET_NAME);
        let server = Server::bind(&socket, json!({"tools":[]}), || {}).unwrap();
        fs::remove_file(&socket).unwrap();
        fs::write(&socket, b"replacement").unwrap();
        drop(server);
        assert_eq!(fs::read(&socket).unwrap(), b"replacement");
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn stale_sockets_are_reclaimed_but_other_files_and_shared_directories_are_refused() {
        let directory = directory();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let socket = directory.join(SOCKET_NAME);
        // A crashed editor leaves its socket file with nobody listening.
        drop(UnixListener::bind(&socket).unwrap());
        assert!(UnixStream::connect(&socket).is_err());
        let server = Server::bind(&socket, json!({}), || {}).unwrap();
        assert_eq!(server.path(), socket);
        assert!(UnixStream::connect(&socket).is_ok());
        drop(server);
        assert!(!socket.exists());
        // The directory existed before, so it is kept.
        assert!(directory.is_dir());
        fs::write(&socket, b"not a socket").unwrap();
        let error = Server::bind(&socket, json!({}), || {}).err().unwrap();
        assert!(error.to_string().contains("not a socket"), "{error}");
        assert_eq!(fs::read(&socket).unwrap(), b"not a socket");
        fs::remove_file(&socket).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        let error = Server::bind(&socket, json!({}), || {}).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn socket_paths_beyond_sun_path_are_explained() {
        let long = PathBuf::from("/").join("d".repeat(120)).join(SOCKET_NAME);
        let error = Server::bind(&long, json!({}), || {}).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("at most 107"), "{error}");
        assert!(!long.parent().unwrap().exists());
    }
    #[test]
    fn relay_lists_tools_without_studio_and_connects_per_call() {
        let directory = directory();
        let socket = directory.join(SOCKET_NAME);
        let messages = |calls: &[u64]| {
            let mut input = format!(
                "{}\n{}\n{}\n",
                json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})
            );
            for id in calls {
                input += &format!(
                    "{}\n",
                    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"inspect","arguments":{}}})
                );
            }
            input
        };
        let run = |input: String| -> Vec<Value> {
            let mut output = Vec::new();
            relay(&socket, input.as_bytes(), &mut output).unwrap();
            String::from_utf8(output)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        };
        let offline = run(messages(&[3]));
        assert_eq!(offline[0]["result"]["serverInfo"]["name"], "veylvik");
        assert_eq!(&offline[1]["result"], crate::live::catalog());
        assert_eq!(offline[2]["id"], 3);
        assert_eq!(offline[2]["result"]["isError"], true);
        assert!(
            offline[2]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not running")
        );
        let server = Server::bind(&socket, json!({"tools":[]}), || {}).unwrap();
        let host = std::thread::spawn(move || {
            for revision in [4, 5] {
                let request = loop {
                    if let Some(request) = server.try_recv() {
                        break request;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                };
                assert_eq!(request.name, "inspect");
                request.respond(Ok(json!({"revision":revision})));
            }
            server
        });
        let online = run(messages(&[7, 8]));
        assert_eq!(online[2]["id"], 7);
        assert_eq!(online[2]["result"]["structuredContent"]["revision"], 4);
        assert_eq!(online[3]["id"], 8);
        assert_eq!(online[3]["result"]["structuredContent"]["revision"], 5);
        let server = host.join().unwrap();
        // Between calls the editor's single client slot is free for another client.
        let mut other = UnixStream::connect(server.path()).unwrap();
        other
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writeln!(
            other,
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize"})
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(other).read_line(&mut line).unwrap();
        assert!(line.contains("protocolVersion"));
        drop(server);
        assert!(!directory.exists());
    }
}
