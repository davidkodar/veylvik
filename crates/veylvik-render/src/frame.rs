//! Explicit image and render-request contracts. No filesystem access or GPU handles.
use crate::{Result, error};
use std::{collections::BTreeMap, sync::Arc};
use veylvik_document::Time;

/// Scene-linear Rec.709 premultiplied pixels. RGB may be negative or exceed one.
#[derive(Debug)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}
impl Image {
    pub fn new(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> Result<Self> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || pixels.len() as u64 != width as u64 * height as u64
        {
            return Err(error("invalid image dimensions or pixel count"));
        }
        if pixels.iter().any(|p| {
            p.iter().any(|v| !v.is_finite())
                || !(0.0..=1.0).contains(&p[3])
                || (p[3] == 0.0 && p[..3].iter().any(|v| *v != 0.0))
        }) {
            return Err(error(
                "image pixels must be finite and premultiplied, with alpha in [0,1] and zero RGB at zero alpha",
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[[f32; 4]] {
        &self.pixels
    }
    pub fn byte_size(&self) -> u64 {
        self.pixels.len() as u64 * 16
    }
}
/// The host supplies immutable images; a later render can supply new content under the same key.
#[derive(Default, Clone)]
pub struct ImageInputs(BTreeMap<String, Arc<Image>>);
impl ImageInputs {
    pub fn insert(&mut self, key: impl Into<String>, image: Arc<Image>) {
        self.0.insert(key.into(), image);
    }
    pub fn get(&self, key: &str) -> Option<&Arc<Image>> {
        self.0.get(key)
    }
}
/// A crop in composition pixels. The initial executor evaluates the whole graph before cropping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Region {
    pub fn validate(self, width: u32, height: u32) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|end| end > width)
            || self
                .y
                .checked_add(self.height)
                .is_none_or(|end| end > height)
        {
            return Err(error(
                "render region must be nonempty and inside the composition",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct RenderRequest {
    pub time: Time,
    pub region: Option<Region>,
}
impl RenderRequest {
    pub fn full(time: Time) -> Self {
        Self { time, region: None }
    }
}
pub struct RenderedFrame {
    pub region: Region,
    pub pixels: Vec<[f32; 4]>,
}
