//! The editor's side of the helper: one child process, bounded messages, hard timeouts. A helper that crashes or stops answering is killed and started again on the next call, and the call that found it so fails with what the helper last wrote.
use crate::protocol::{self, Reply, Request};
use crate::{Description, Plugin, Result, SUBCOMMAND, Value};
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// Loading and describing may run a plugin's initialisation (licence checks, GPU probes).
const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(60);
/// A render of one frame.
const RENDER_TIMEOUT: Duration = Duration::from_secs(120);

type Message = Result<(Reply, Vec<f32>)>;
struct Running {
    /// Frames go to and from the helper here; without it, through the pipes.
    shared: Option<veylvik_interop::shm::Shared>,
    child: Child,
    input: BufWriter<ChildStdin>,
    replies: mpsc::Receiver<Message>,
    log: Arc<Mutex<String>>,
}
/// A helper process, started when first needed.
pub struct Worker {
    exe: PathBuf,
    running: Option<Running>,
    /// Whether frames may go through shared memory (else always the pipes).
    share: bool,
}
impl Worker {
    /// A worker run by `exe` (the `veylvik` program).
    pub fn new(exe: &Path) -> Self {
        Self {
            exe: exe.to_path_buf(),
            running: None,
            share: true,
        }
    }
    /// A worker that sends frames through the pipes only (the fallback when shared
    /// memory cannot be made; kept for comparison).
    pub fn without_shared_memory(exe: &Path) -> Self {
        let mut worker = Self::new(exe);
        worker.share = false;
        worker
    }
    fn start(&mut self) -> Result<&mut Running> {
        if self.running.is_none() {
            let shared = self
                .share
                .then(veylvik_interop::shm::Shared::create)
                .and_then(|made| made.ok());
            let mut command = Command::new(&self.exe);
            command.arg(SUBCOMMAND);
            if let Some(memory) = &shared {
                command.arg("--shared").arg(memory.path());
            }
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    format!(
                        "could not start the OpenFX helper ({}): {e}",
                        self.exe.display()
                    )
                })?;
            let stdout = child.stdout.take().ok_or("the helper has no stdout")?;
            let mut stderr = child.stderr.take().ok_or("the helper has no stderr")?;
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
                            let cut = (log.len() - 8192..log.len())
                                .find(|i| log.is_char_boundary(*i))
                                .unwrap_or(0);
                            log.drain(..cut);
                        }
                    }
                }
            });
            let (sender, replies) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let mut stdout = BufReader::new(stdout);
                loop {
                    let message = protocol::read::<Reply>(&mut stdout).and_then(|m| {
                        m.ok_or_else(|| {
                            "the OpenFX helper exited (the plugin may have crashed)".to_string()
                        })
                    });
                    let failed = message.is_err();
                    if sender.send(message).is_err() || failed {
                        break;
                    }
                }
            });
            let input = BufWriter::new(child.stdin.take().ok_or("the helper has no stdin")?);
            self.running = Some(Running {
                shared,
                child,
                input,
                replies,
                log,
            });
        }
        self.running
            .as_mut()
            .ok_or_else(|| "the OpenFX helper is not running".into())
    }
    /// Stops the helper; the next call starts a new one.
    pub fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
    fn call(&mut self, request: &Request, payload: &[&[f32]], timeout: Duration) -> Message {
        let running = self.start()?;
        let reply = protocol::write(&mut running.input, request, payload).and_then(|_| {
            running
                .replies
                .recv_timeout(timeout)
                .map_err(|_| {
                    format!(
                        "the OpenFX helper did not answer within {} s",
                        timeout.as_secs()
                    )
                })
                .and_then(|m| m)
        });
        match reply {
            Ok((reply, pixels)) => match reply.error {
                Some(error) => Err(error),
                None => Ok((reply, pixels)),
            },
            Err(error) => {
                let log = running
                    .log
                    .lock()
                    .map(|l| l.trim().to_string())
                    .unwrap_or_default();
                self.stop();
                let last = log
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("");
                Err(if last.is_empty() {
                    error
                } else {
                    format!("{error}: {last}")
                })
            }
        }
    }
    /// The image effects in a bundle, and why any others could not be used.
    pub fn list(&mut self, bundle: &Path) -> Result<(Vec<Plugin>, Vec<String>)> {
        let request = Request::List {
            bundle: bundle.to_string_lossy().into_owned(),
        };
        let (reply, _) = self.call(&request, &[], DESCRIBE_TIMEOUT)?;
        Ok((reply.plugins, reply.refused))
    }
    pub fn describe(&mut self, plugin: &Plugin, context: &str) -> Result<Description> {
        let request = Request::Describe {
            bundle: plugin.bundle.to_string_lossy().into_owned(),
            index: plugin.index,
            context: context.to_string(),
        };
        let (reply, _) = self.call(&request, &[], DESCRIBE_TIMEOUT)?;
        reply
            .description
            .ok_or_else(|| "the helper sent no description".into())
    }
    /// Renders `width` x `height` at `time` with `values`, from `inputs` (clip name and
    /// picture), into a new picture; also what the plugin said.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        plugin: &Plugin,
        context: &str,
        [width, height]: [u32; 2],
        time: f64,
        values: &[(String, Value)],
        inputs: &[(&str, &[f32])],
    ) -> Result<(Vec<f32>, Vec<String>)> {
        let mut out = Vec::new();
        let inputs: Vec<(&str, Option<f64>, &[f32])> =
            inputs.iter().map(|(c, p)| (*c, None, *p)).collect();
        let said = self.render_with(
            plugin,
            context,
            [width, height],
            time,
            1,
            values,
            &inputs,
            &mut |p| out = p.to_vec(),
        )?;
        Ok((out, said))
    }
    /// As [`Worker::render`], handing the picture to `deliver` where it lies (in the
    /// shared memory when the frames went that way), so a caller that copies it on
    /// anyway (to the graphics card) needs no frame of its own; returns what the plugin said.
    #[allow(clippy::too_many_arguments)]
    pub fn render_with(
        &mut self,
        plugin: &Plugin,
        context: &str,
        [width, height]: [u32; 2],
        time: f64,
        duration: u32,
        values: &[(String, Value)],
        inputs: &[(&str, Option<f64>, &[f32])],
        deliver: &mut dyn FnMut(&[f32]),
    ) -> Result<Vec<String>> {
        let pixels = width as usize * height as usize * 4;
        if let Some((name, _, _)) = inputs.iter().find(|(_, _, p)| p.len() != pixels) {
            return Err(format!("input {name} is not {width} x {height}"));
        }
        // In shared memory when there is some: the inputs one after another, then the output.
        let running = self.start()?;
        let shared = match running.shared.as_mut() {
            Some(memory) => memory
                .reserve(((inputs.len() + 1) * pixels * 4) as u64, true)
                .is_ok(),
            None => false,
        };
        if shared && let Some(memory) = running.shared.as_mut() {
            for (k, (_, _, picture)) in inputs.iter().enumerate() {
                memory
                    .floats_mut(k * pixels, pixels)?
                    .copy_from_slice(picture);
            }
        }
        let request = Request::Render {
            bundle: plugin.bundle.to_string_lossy().into_owned(),
            index: plugin.index,
            context: context.to_string(),
            width,
            height,
            time,
            values: values.to_vec(),
            inputs: inputs.iter().map(|(n, _, _)| n.to_string()).collect(),
            shared,
            times: inputs.iter().map(|(_, t, _)| *t).collect(),
            duration,
        };
        let payload: Vec<&[f32]> = if shared {
            Vec::new()
        } else {
            inputs.iter().map(|(_, _, p)| *p).collect()
        };
        let (reply, out) = self.call(&request, &payload, RENDER_TIMEOUT)?;
        if shared {
            let memory = self
                .running
                .as_ref()
                .and_then(|r| r.shared.as_ref())
                .ok_or("the OpenFX helper stopped")?;
            deliver(memory.floats(inputs.len() * pixels, pixels)?);
        } else if out.len() == pixels {
            deliver(&out);
        } else {
            return Err("the helper sent a picture of the wrong size".into());
        }
        Ok(reply.messages)
    }
}
impl Worker {
    /// The frames of each input clip `plugin` needs to render `time` with `values`.
    #[allow(clippy::too_many_arguments)]
    pub fn frames_needed(
        &mut self,
        plugin: &Plugin,
        context: &str,
        [width, height]: [u32; 2],
        time: f64,
        duration: u32,
        values: &[(String, Value)],
        connected: &[&str],
    ) -> Result<Vec<(String, Vec<f64>)>> {
        let request = Request::FramesNeeded {
            connected: connected.iter().map(|c| c.to_string()).collect(),
            bundle: plugin.bundle.to_string_lossy().into_owned(),
            index: plugin.index,
            context: context.to_string(),
            width,
            height,
            time,
            values: values.to_vec(),
            duration,
        };
        let (reply, _) = self.call(&request, &[], DESCRIBE_TIMEOUT)?;
        Ok(reply.frames)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(running) = self.running.as_mut() {
            let _ = protocol::write(&mut running.input, &Request::Quit, &[]);
        }
        self.stop();
    }
}
