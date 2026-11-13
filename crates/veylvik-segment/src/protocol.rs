//! Worker wire format: a little-endian u32 length and a JSON header, then a u64 length and a binary
//! payload. Both lengths are bounded before anything is allocated.
use crate::{Point, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const MAX_HEADER: usize = 64 * 1024;
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Loads a model's sessions; replaces any loaded model.
    Load {
        model: String,
    },
    /// Segments one SIDE² sRGB frame (the payload). A prompt starts a new track with this frame as
    /// its conditioning frame; without one the frame is propagated from the previous frames.
    Frame {
        prompt: Option<Vec<Point>>,
    },
    /// Mattes the payload: `width` × `height` RGBA bytes with the trimap in alpha. Loads the
    /// matting model on first use; replies with f32 alpha.
    Matte {
        model: String,
        width: u32,
        height: u32,
    },
    Quit,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Object presence logit of a frame reply; at or below zero the subject is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Execution provider or device that ran the model, after a load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

pub fn write(mut out: impl Write, header: &impl Serialize, payload: &[u8]) -> Result<()> {
    let header = serde_json::to_vec(header)?;
    if header.len() > MAX_HEADER || payload.len() > MAX_PAYLOAD {
        return Err("segmentation message exceeds its size limit".into());
    }
    out.write_all(&(header.len() as u32).to_le_bytes())?;
    out.write_all(&header)?;
    out.write_all(&(payload.len() as u64).to_le_bytes())?;
    out.write_all(payload)?;
    out.flush()?;
    Ok(())
}
/// Reads one message; `None` at a clean end of stream.
pub fn read<T: for<'de> Deserialize<'de>>(mut input: impl Read) -> Result<Option<(T, Vec<u8>)>> {
    let mut len = [0; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_HEADER {
        return Err("segmentation header exceeds 64 KiB".into());
    }
    let mut header = vec![0; len];
    input.read_exact(&mut header)?;
    let mut size = [0; 8];
    input.read_exact(&mut size)?;
    let size = usize::try_from(u64::from_le_bytes(size)).unwrap_or(usize::MAX);
    if size > MAX_PAYLOAD {
        return Err("segmentation payload exceeds 16 MiB".into());
    }
    let mut payload = vec![0; size];
    input.read_exact(&mut payload)?;
    Ok(Some((serde_json::from_slice(&header)?, payload)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn messages_round_trip_and_oversized_lengths_are_refused_before_allocation() {
        let request = Request::Frame {
            prompt: Some(vec![Point {
                x: 0.25,
                y: 0.5,
                positive: true,
            }]),
        };
        let mut bytes = Vec::new();
        write(&mut bytes, &request, &[1, 2, 3]).unwrap();
        let (back, payload): (Request, _) = read(bytes.as_slice()).unwrap().unwrap();
        assert_eq!((back, payload), (request, vec![1, 2, 3]));
        assert!(read::<Request>(&[][..]).unwrap().is_none());
        let huge = (MAX_HEADER as u32 + 1).to_le_bytes();
        assert!(read::<Request>(&huge[..]).is_err());
        let mut big_payload = Vec::new();
        let header = br#"{"op":"quit"}"#;
        big_payload.extend((header.len() as u32).to_le_bytes());
        big_payload.extend(header);
        big_payload.extend((MAX_PAYLOAD as u64 + 1).to_le_bytes());
        assert!(read::<Request>(big_payload.as_slice()).is_err());
    }
}
