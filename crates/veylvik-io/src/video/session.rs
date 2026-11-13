//! Hardware sessions stay in-process; the compatibility scaler remains isolated.
use super::{Backend, process};
use crate::Result;
use std::{path::Path, sync::Arc};
use veylvik_render::VideoFormat;
#[derive(Clone)]
pub(super) enum Decoded {
    Host(Arc<[u8]>),
    Hardware(Arc<veylvik_decode::HardwareFrame>),
}
impl Decoded {
    pub fn byte_len(&self) -> u64 {
        match self {
            Self::Host(b) => b.len() as u64,
            Self::Hardware(f) => f.byte_len(),
        }
    }
    pub fn stage(&self) -> Result<Arc<[u8]>> {
        match self {
            Self::Host(b) => Ok(b.clone()),
            Self::Hardware(f) => Ok(f.stage()?.into()),
        }
    }
}
pub(super) enum Session {
    Hardware {
        decoder: Box<veylvik_decode::Decoder>,
        next: usize,
    },
    Software(process::Decoder),
}
impl Session {
    pub fn hardware(
        path: &Path,
        backend: &Backend,
        size: [u32; 2],
        format: VideoFormat,
        index: usize,
        pts: i64,
    ) -> Result<Self> {
        use veylvik_decode::{DeviceKind, PlaneFormat};
        let (kind, codec) = match backend {
            Backend::Nvdec(codec) => (DeviceKind::Cuda, Some(*codec)),
            Backend::Vaapi(path) => (DeviceKind::Vaapi(path.clone()), None),
            Backend::Software => return Err("Software is not a native hardware session".into()),
        };
        let format = match format {
            VideoFormat::Nv12 => PlaneFormat::Nv12,
            VideoFormat::P010 => PlaneFormat::P010,
            _ => return Err("Unsupported hardware format".into()),
        };
        let mut decoder = veylvik_decode::Decoder::open(path, kind, codec, size, format)?;
        if index > 0 {
            decoder.seek(pts)?;
        }
        Ok(Self::Hardware {
            decoder: Box::new(decoder),
            next: index,
        })
    }
    pub fn next(&self) -> usize {
        match self {
            Self::Hardware { next, .. } => *next,
            Self::Software(d) => d.next,
        }
    }
    pub fn read(&mut self, timestamps: &[i64]) -> Result<Decoded> {
        match self {
            Self::Hardware { decoder, next } => {
                let pts = *timestamps.get(*next).ok_or("Frame index out of range")?;
                let frame = decoder.frame_at(pts)?;
                *next += 1;
                Ok(Decoded::Hardware(Arc::new(frame)))
            }
            Self::Software(d) => d.read().map(|b| Decoded::Host(b.into())),
        }
    }
    pub fn device_id(&self) -> Option<u64> {
        match self {
            Self::Hardware { decoder, .. } => Some(decoder.device_id()),
            _ => None,
        }
    }
    pub fn seeks(&self) -> u64 {
        match self {
            Self::Hardware { decoder, .. } => decoder.seeks,
            _ => 0,
        }
    }
    /// Native preroll/skipped frames remain on the decoder GPU, never staged.
    pub fn position(&mut self, index: usize, timestamps: &[i64]) -> Result<bool> {
        match self {
            Self::Hardware { decoder, next } => {
                if *next > index || index - *next > 8 {
                    decoder.seek(timestamps[index])?;
                }
                *next = index;
                Ok(true)
            }
            Self::Software(_) => Ok(false),
        }
    }
}
