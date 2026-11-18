//! One pixel of a finished frame read back for inspection: 16 bytes copied and mapped, never the whole frame. The frame is unchanged.
use crate::{GpuFrame, Renderer, Result, error};
use std::sync::Arc;

impl Renderer {
    /// The scene-linear premultiplied value at `x`, `y` of the frame's region.
    /// Blocks until the GPU answers, so call it away from a UI thread.
    pub fn read_pixel(&self, frame: &GpuFrame, x: u32, y: u32) -> Result<[f32; 4]> {
        if !Arc::ptr_eq(&self.owner, &frame.owner) {
            return Err(error("GPU frame belongs to another renderer"));
        }
        let region = frame.region();
        if x >= region.width || y >= region.height {
            return Err(error("pixel is outside the frame"));
        }
        let index = u64::from(region.y + y) * u64::from(frame.width()) + u64::from(region.x + x);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pixel probe"),
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(frame.buffer(), index * 16, &buffer, 0, 16);
        let values: Vec<[f32; 4]> =
            self.map_values(&buffer, self.queue.submit([encoder.finish()]))?;
        values
            .first()
            .copied()
            .ok_or_else(|| error("pixel readback was empty"))
    }
}
