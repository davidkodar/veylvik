//! Helper wire format: a little-endian u32 length and a JSON header, then a u64 length and a binary payload (pictures, RGBA f32 little-endian). Both lengths are bounded before anything is allocated.
use crate::{Description, Plugin, Result, Value};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const MAX_HEADER: usize = 4 * 1024 * 1024;
/// Four inputs at 8K.
pub const MAX_PAYLOAD: u64 = 4 * 8192 * 8192 * 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    List {
        bundle: String,
    },
    Describe {
        bundle: String,
        index: usize,
        context: String,
    },
    /// The payload is one picture per name in `inputs`, in that order; with `shared`
    /// the pictures are in the shared memory instead, one after another, and the
    /// output goes after them.
    Render {
        bundle: String,
        index: usize,
        context: String,
        width: u32,
        height: u32,
        time: f64,
        values: Vec<(String, Value)>,
        inputs: Vec<String>,
        #[serde(default)]
        shared: bool,
        /// Each input's frame (None: the render's own).
        #[serde(default)]
        times: Vec<Option<f64>>,
        /// The composition's length in frames.
        #[serde(default)]
        duration: u32,
    },
    /// The frames of each input clip the plugin needs to render `time`.
    FramesNeeded {
        bundle: String,
        index: usize,
        context: String,
        width: u32,
        height: u32,
        time: f64,
        values: Vec<(String, Value)>,
        duration: u32,
        /// The input clips that are wired.
        #[serde(default)]
        connected: Vec<String>,
    },
    Quit,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<Plugin>,
    /// Plugins that could not be described, as "identifier: why".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
    /// Frames needed, per input clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<(String, Vec<f64>)>,
    /// What the plugin said.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
}
pub fn write(mut out: impl Write, header: &impl Serialize, payload: &[&[f32]]) -> Result<()> {
    let header = serde_json::to_vec(header).map_err(|e| e.to_string())?;
    let bytes: u64 = payload.iter().map(|p| p.len() as u64 * 4).sum();
    if header.len() > MAX_HEADER || bytes > MAX_PAYLOAD {
        return Err("an OpenFX helper message exceeds its size limit".into());
    }
    let io = |e: std::io::Error| e.to_string();
    out.write_all(&(header.len() as u32).to_le_bytes())
        .map_err(io)?;
    out.write_all(&header).map_err(io)?;
    out.write_all(&bytes.to_le_bytes()).map_err(io)?;
    let mut chunk = Vec::with_capacity(1 << 20);
    for picture in payload {
        for part in picture.chunks(1 << 18) {
            chunk.clear();
            chunk.extend(part.iter().flat_map(|v| v.to_le_bytes()));
            out.write_all(&chunk).map_err(io)?;
        }
    }
    out.flush().map_err(io)
}
/// Reads one message; `None` at a clean end of stream.
pub fn read<T: for<'de> Deserialize<'de>>(mut input: impl Read) -> Result<Option<(T, Vec<f32>)>> {
    let io = |e: std::io::Error| e.to_string();
    let mut len = [0; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_HEADER {
        return Err("an OpenFX helper header is too large".into());
    }
    let mut header = vec![0; len];
    input.read_exact(&mut header).map_err(io)?;
    let header = serde_json::from_slice(&header).map_err(|e| e.to_string())?;
    let mut size = [0; 8];
    input.read_exact(&mut size).map_err(io)?;
    let size = u64::from_le_bytes(size);
    if size > MAX_PAYLOAD || size % 4 != 0 {
        return Err("an OpenFX helper payload is malformed".into());
    }
    let mut bytes = vec![0; size as usize];
    input.read_exact(&mut bytes).map_err(io)?;
    let floats = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    Ok(Some((header, floats)))
}
