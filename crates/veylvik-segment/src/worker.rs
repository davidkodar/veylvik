//! The segmentation worker: a child process that owns the ONNX Runtime and the GPU sessions.
//! The host talks to it over pipes with bounded messages and hard timeouts, and kills it on drop.
use crate::{
    FRAME_BYTES, Point, Result, SIDE,
    protocol::{self, Reply, Request},
};
use std::{
    io::{BufReader, BufWriter, Read},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
    time::Duration,
};

/// Subcommand of the `veylvik` binary that runs [`serve`].
pub const SUBCOMMAND: &str = "segment-worker";
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Generous: the first frame on a new device may autotune convolutions.
const FRAME_TIMEOUT: Duration = Duration::from_secs(60);
const RUNTIME: &str = "/usr/lib/libonnxruntime.so";
/// The CUDA provider needs cuDNN symbols its library does not link; preload them when present.
const CUDNN: &[&str] = &[
    "/usr/lib/libcudnn.so.9",
    "/opt/cuda/lib64/libcudnn.so.9",
    "/usr/local/cuda/lib64/libcudnn.so.9",
];

/// One frame's result: SIDE² mask logits (row-major) and the presence logit.
pub struct Mask {
    pub logits: Vec<f32>,
    pub score: f32,
}
impl Mask {
    pub fn present(&self) -> bool {
        self.score > 0.0
    }
}
type Message = Result<(Reply, Vec<u8>)>;
pub struct Worker {
    child: Child,
    input: Option<BufWriter<ChildStdin>>,
    replies: mpsc::Receiver<Message>,
    reader: Option<JoinHandle<()>>,
    log: Arc<Mutex<String>>,
    pub device: String,
}
impl Worker {
    /// Starts `exe segment-worker` and loads `model` (which must be installed).
    pub fn spawn(exe: &Path, model: &str) -> Result<Self> {
        let mut command = Command::new(exe);
        command.arg(SUBCOMMAND);
        if std::env::var_os("ORT_DYLIB_PATH").is_none() {
            if !Path::new(RUNTIME).exists() {
                return Err("ONNX Runtime is not installed; install onnxruntime-cuda (or set ORT_DYLIB_PATH)".into());
            }
            command.env("ORT_DYLIB_PATH", RUNTIME);
        }
        if let Some(cudnn) = CUDNN.iter().find(|p| Path::new(p).exists()) {
            let preload = match std::env::var("LD_PRELOAD") {
                Ok(existing) if !existing.is_empty() => format!("{existing} {cudnn}"),
                _ => (*cudnn).to_owned(),
            };
            command.env("LD_PRELOAD", preload);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start the segmentation worker: {e}"))?;
        let stdout = child.stdout.take().ok_or("worker has no stdout")?;
        let mut stderr = child.stderr.take().ok_or("worker has no stderr")?;
        let log = Arc::new(Mutex::new(String::new()));
        let sink = log.clone();
        std::thread::spawn(move || {
            let mut chunk = [0; 2048];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if let Ok(mut log) = sink.lock() {
                    log.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    if log.len() > 8192 {
                        let cut = log.len() - 8192;
                        let cut = (cut..log.len())
                            .find(|i| log.is_char_boundary(*i))
                            .unwrap_or(0);
                        log.drain(..cut);
                    }
                }
            }
        });
        // At most one reply waits; the worker never runs ahead of the host.
        let (sender, replies) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let message = protocol::read::<Reply>(&mut stdout)
                    .and_then(|m| m.ok_or_else(|| "the segmentation worker exited".into()));
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });
        let mut worker = Self {
            input: child.stdin.take().map(BufWriter::new),
            child,
            replies,
            reader: Some(reader),
            log,
            device: String::new(),
        };
        let (reply, _) = worker.call(
            &Request::Load {
                model: model.to_owned(),
            },
            &[],
            LOAD_TIMEOUT,
        )?;
        worker.device = reply.device.unwrap_or_default();
        Ok(worker)
    }
    fn call(&mut self, request: &Request, payload: &[u8], timeout: Duration) -> Message {
        let sent = match self.input.as_mut() {
            Some(input) => protocol::write(input, request, payload),
            None => Err("the segmentation worker has stopped".into()),
        };
        let reply = sent.and_then(|_| {
            self.replies
                .recv_timeout(timeout)
                .map_err(|_| "the segmentation worker stopped responding".into())
                .and_then(|m| m)
        });
        match reply {
            Ok((reply, payload)) => match reply.error {
                Some(error) => Err(error.into()),
                None => Ok((reply, payload)),
            },
            Err(error) => {
                self.stop();
                let log = self
                    .log
                    .lock()
                    .map(|l| l.trim().to_owned())
                    .unwrap_or_default();
                let detail = log
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("");
                Err(if detail.is_empty() {
                    error
                } else {
                    format!("{error}: {detail}").into()
                })
            }
        }
    }
    /// Segments one SIDE² sRGB frame; a prompt restarts tracking from this frame.
    pub fn frame(&mut self, rgb: &[u8], prompt: Option<&[Point]>) -> Result<Mask> {
        if rgb.len() != FRAME_BYTES {
            return Err("a model frame must be SIDE² RGB bytes".into());
        }
        if let Some(points) = prompt {
            crate::validate_points(points)?;
        }
        let request = Request::Frame {
            prompt: prompt.map(<[Point]>::to_vec),
        };
        let (reply, payload) = self.call(&request, rgb, FRAME_TIMEOUT)?;
        if payload.len() != SIDE * SIDE * 4 {
            return Err("the segmentation worker returned a malformed mask".into());
        }
        Ok(Mask {
            logits: payload
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
            score: reply.score.unwrap_or(f32::NEG_INFINITY),
        })
    }
    /// Refines alpha with `model` (a matting model) inside the trimap carried in `pixels`.
    pub fn matte(
        &mut self,
        model: &str,
        pixels: &[u8],
        width: usize,
        height: usize,
    ) -> Result<Vec<f32>> {
        if pixels.len() != width * height * 4 {
            return Err("matting input does not match its size".into());
        }
        let request = Request::Matte {
            model: model.to_owned(),
            width: u32::try_from(width).map_err(|_| "matting input too wide")?,
            height: u32::try_from(height).map_err(|_| "matting input too tall")?,
        };
        let (_, payload) = self.call(&request, pixels, LOAD_TIMEOUT)?;
        if payload.len() != width * height * 4 {
            return Err("the segmentation worker returned a malformed matte".into());
        }
        Ok(payload
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect())
    }
    fn stop(&mut self) {
        if let Some(input) = self.input.as_mut() {
            let _ = protocol::write(input, &Request::Quit, &[]);
        }
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// The worker loop: answers requests on `input`/`output` until Quit or end of input.
#[cfg(feature = "engine")]
pub fn serve(input: impl std::io::Read, output: impl std::io::Write) -> Result<()> {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    let mut engine: Option<Box<dyn crate::engine::Tracker>> = None;
    let mut matting: Option<(String, crate::engine::vitmatte::Matting)> = None;
    while let Some((request, payload)) = protocol::read::<Request>(&mut input)? {
        let fail = |e: crate::Error| Reply {
            error: Some(e.to_string()),
            score: None,
            device: None,
        };
        match request {
            Request::Quit => break,
            Request::Load { model } => {
                let reply = match crate::models::find(&model).and_then(|model| {
                    if !crate::models::installed(model)? {
                        return Err(format!("{} is not installed", model.title).into());
                    }
                    crate::engine::load(model, &crate::models::dir(model)?)
                }) {
                    Ok(loaded) => {
                        engine = Some(loaded);
                        Reply {
                            error: None,
                            score: None,
                            device: Some("CUDA".into()),
                        }
                    }
                    Err(e) => fail(e),
                };
                protocol::write(&mut output, &reply, &[])?;
            }
            Request::Matte {
                model,
                width,
                height,
            } => {
                let (width, height) = (width as usize, height as usize);
                let result = (|| -> crate::Result<Vec<f32>> {
                    if width.max(height) > crate::edges::MAX_SIDE
                        || payload.len() != width * height * 4
                    {
                        return Err("matting input does not match its size".into());
                    }
                    if matting.as_ref().is_none_or(|(id, _)| *id != model) {
                        let found = crate::models::find(&model)?;
                        if !crate::models::ready(found)? {
                            return Err(format!(
                                "{} is not installed or is switched off",
                                found.title
                            )
                            .into());
                        }
                        matting = Some((
                            model.clone(),
                            crate::engine::vitmatte::Matting::load(&crate::models::dir(found)?)?,
                        ));
                    }
                    let (_, session) = matting.as_mut().ok_or("matting model unavailable")?;
                    session.matte(&payload, width, height)
                })();
                match result {
                    Ok(alpha) => {
                        let bytes: Vec<u8> = alpha.iter().flat_map(|v| v.to_le_bytes()).collect();
                        let reply = Reply {
                            error: None,
                            score: None,
                            device: None,
                        };
                        protocol::write(&mut output, &reply, &bytes)?;
                    }
                    Err(e) => protocol::write(&mut output, &fail(e), &[])?,
                }
            }
            Request::Frame { prompt } => {
                let result = match engine.as_mut() {
                    None => Err("no segmentation model is loaded".into()),
                    Some(_) if payload.len() != FRAME_BYTES => {
                        Err("a model frame must be SIDE² RGB bytes".into())
                    }
                    Some(engine) => engine.frame(&payload, prompt.as_deref()),
                };
                match result {
                    Ok(mask) => {
                        let bytes: Vec<u8> =
                            mask.logits.iter().flat_map(|v| v.to_le_bytes()).collect();
                        let reply = Reply {
                            error: None,
                            score: Some(mask.score),
                            device: None,
                        };
                        protocol::write(&mut output, &reply, &bytes)?;
                    }
                    Err(e) => protocol::write(&mut output, &fail(e), &[])?,
                }
            }
        }
    }
    Ok(())
}
