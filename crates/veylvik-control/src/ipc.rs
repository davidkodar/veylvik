//! Opt-in local transport. A private new directory owns one Unix socket.
use crate::{MAX_MESSAGE_BYTES, Protocol, ToolOutput};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};
pub const SOCKET_NAME: &str = "control.sock";
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
    _endpoint: Endpoint,
}
struct Endpoint {
    directory: PathBuf,
    directory_id: (u64, u64),
    socket_id: Option<(u64, u64)>,
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        let socket = self.directory.join(SOCKET_NAME);
        if self.socket_id.is_some() && identity(&socket).ok() == self.socket_id {
            let _ = fs::remove_file(socket);
        }
        if identity(&self.directory).ok() == Some(self.directory_id) {
            let _ = fs::remove_dir(&self.directory);
        }
    }
}
fn identity(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}
impl Server {
    /// Refuse existing directories. Never replace or adopt a pre-existing endpoint.
    /// One client is served at a time; requests are dispatched to the host thread.
    pub fn bind(
        directory: &Path,
        catalog: Value,
        wake: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        fs::DirBuilder::new().mode(0o700).create(directory)?;
        let directory_id = identity(directory)?;
        let directory = directory.canonicalize()?;
        let mut endpoint = Endpoint {
            directory,
            directory_id,
            socket_id: None,
        };
        let socket = endpoint.directory.join(SOCKET_NAME);
        let listener = UnixListener::bind(&socket)?;
        endpoint.socket_id = Some(identity(&socket)?);
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
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
            _endpoint: endpoint,
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "veylvik-control-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn private_transport_preserves_fragmented_messages_and_dispatches_on_host() {
        let directory = directory();
        let (wake, awakened) = mpsc::channel();
        let server = Server::bind(&directory, json!({"tools":[]}), move || {
            let _ = wake.send(());
        })
        .unwrap();
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
        assert!(Server::bind(&directory, json!({}), || {}).is_err());
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
        let server = Server::bind(&directory, json!({"tools":[]}), || {}).unwrap();
        let socket = directory.join(SOCKET_NAME);
        fs::remove_file(&socket).unwrap();
        fs::write(&socket, b"replacement").unwrap();
        drop(server);
        assert_eq!(fs::read(&socket).unwrap(), b"replacement");
        fs::remove_dir_all(directory).unwrap();
    }
}
