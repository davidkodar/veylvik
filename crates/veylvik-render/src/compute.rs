//! The narrow GPU interface analysis crates build on: budgeted scratch buffers, blocking
//! submission with readback, device ownership checks and new frames, without renderer internals.
use crate::{GpuFrame, Region, Renderer, Result, error, frame::DeviceIdentity, pool::FrameBuffers};
use std::sync::Arc;

/// Identifies the device a renderer, pipeline or frame belongs to.
#[derive(Clone)]
pub struct DeviceKey(Arc<DeviceIdentity>);
impl PartialEq for DeviceKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Renderer {
    pub fn device_key(&self) -> DeviceKey {
        DeviceKey(self.owner.clone())
    }
    /// Whether `frame` lives on this renderer's device.
    pub fn owns(&self, frame: &GpuFrame) -> bool {
        Arc::ptr_eq(&self.owner, &frame.owner)
    }
    /// Runs GPU work whose buffers count against the renderer's memory budget. Buffers are
    /// recycled only after the work completed; a failure marks the renderer unusable, as
    /// for rendering.
    pub fn compute<R>(&self, work: impl FnOnce(&mut Scratch<'_, '_>) -> Result<R>) -> Result<R> {
        let mut pool = self.working_pool()?;
        let mut scratch = Scratch {
            renderer: self,
            buffers: pool.frame(self.budget_bytes),
        };
        work(&mut scratch)
    }
}
pub struct Scratch<'r, 'p> {
    renderer: &'r Renderer,
    buffers: FrameBuffers<'p>,
}
impl Scratch<'_, '_> {
    pub fn device(&self) -> &wgpu::Device {
        &self.renderer.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.renderer.queue
    }
    /// A buffer from the shared pool; fails rather than exceed the budget.
    pub fn buffer(
        &mut self,
        size: u64,
        usage: wgpu::BufferUsages,
        label: &str,
    ) -> Result<wgpu::Buffer> {
        self.buffers.take(&self.renderer.device, size, usage, label)
    }
    /// Submits `encoder` and waits for the device to finish it.
    pub fn run(&mut self, encoder: wgpu::CommandEncoder) -> Result<()> {
        self.buffers.submitted();
        self.renderer
            .wait(self.renderer.queue.submit([encoder.finish()]))?;
        self.buffers.completed();
        Ok(())
    }
    /// Submits `encoder`, which must copy results into the mappable `readback`, and reads them.
    pub fn read<T: bytemuck::Pod>(
        &mut self,
        encoder: wgpu::CommandEncoder,
        readback: &wgpu::Buffer,
    ) -> Result<Vec<T>> {
        self.buffers.submitted();
        let submission = self.renderer.queue.submit([encoder.finish()]);
        let values = self.renderer.map_values(readback, submission)?;
        self.buffers.completed();
        Ok(values)
    }
    /// Keeps a scratch `buffer` of premultiplied RGBA32F pixels alive as a full, uncropped frame.
    pub fn frame(&mut self, buffer: &wgpu::Buffer, width: u32, height: u32) -> Result<GpuFrame> {
        if buffer.size() != u64::from(width) * u64::from(height) * 16 {
            return Err(error("frame buffer size does not match its dimensions"));
        }
        Ok(GpuFrame {
            flow_report: None,
            identity: crate::frame::next_identity()?,
            storage: Arc::new(self.buffers.pin(buffer)),
            owner: self.renderer.owner.clone(),
            region: Region {
                x: 0,
                y: 0,
                width,
                height,
            },
            width,
            height,
        })
    }
}
impl GpuFrame {
    pub fn device_key(&self) -> DeviceKey {
        DeviceKey(self.owner.clone())
    }
}
