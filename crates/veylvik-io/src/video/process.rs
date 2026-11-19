//! Bounded child-process I/O. Children are killed and reaped on timeout or drop.
use crate::Result;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};
const TIMEOUT: Duration = Duration::from_secs(30);
fn diagnostics(mut stream: impl Read) -> String {
    let mut tail = Vec::new();
    let mut chunk = [0; 2048];
    while let Ok(n) = stream.read(&mut chunk) {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > 8192 {
            tail.drain(..tail.len() - 8192);
        }
    }
    String::from_utf8_lossy(&tail).trim().to_owned()
}
pub fn capture(command: &mut Command, limit: usize) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start FFmpeg/ffprobe; install FFmpeg: {e}"))?;
    let output = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = std::thread::spawn(move || diagnostics(stderr));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                break Err("Video indexing exceeded 30 seconds; use a shorter shot".to_owned());
            }
            Err(e) => break Err(e.to_string()),
        }
    };
    if status.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let bytes = reader.join().map_err(|_| "Video probe reader failed")??;
    let errors = errors.join().unwrap_or_default();
    let status = status?;
    if bytes.len() > limit {
        return Err("Video index exceeds its memory limit; use a shorter shot".into());
    }
    if !status.success() {
        return Err(format!(
            "this is not a readable video: the file is damaged, incomplete or of another kind ({})",
            errors.replace('\n', "; ")
        )
        .into());
    }
    Ok(bytes)
}
pub struct Decoder {
    child: Child,
    receiver: Option<mpsc::Receiver<std::io::Result<Vec<u8>>>>,
    reader: Option<JoinHandle<()>>,
    errors: Option<JoinHandle<String>>,
    pub next: usize,
}
impl Decoder {
    pub fn new(command: &mut Command, frame_bytes: usize, next: usize) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start FFmpeg; install FFmpeg: {e}"))?;
        let mut stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        // At most one decoded frame waits ahead of the caller.
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            loop {
                let mut bytes = vec![0; frame_bytes];
                let result = stdout.read_exact(&mut bytes).map(|_| bytes);
                let failed = result.is_err();
                if sender.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let errors = std::thread::spawn(move || diagnostics(stderr));
        Ok(Self {
            child,
            receiver: Some(receiver),
            reader: Some(reader),
            errors: Some(errors),
            next,
        })
    }
    pub fn read(&mut self) -> Result<Vec<u8>> {
        match self.receiver.as_ref().unwrap().recv_timeout(TIMEOUT) {
            Ok(Ok(bytes)) => {
                self.next += 1;
                Ok(bytes)
            }
            result => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                let errors = self
                    .errors
                    .take()
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                Err(format!(
                    "Video frame decode failed or timed out ({:?}): {errors}",
                    result.err()
                )
                .into())
            }
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.receiver.take();
        if let Some(thread) = self.reader.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.errors.take() {
            let _ = thread.join();
        }
    }
}
