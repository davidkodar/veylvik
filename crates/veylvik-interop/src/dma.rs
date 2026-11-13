//! Linear DMA-BUF export to a foreign media driver on the same DRM device.
use super::*;
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaLayout {
    pub width: u32,
    pub height: u32,
    pub ten_bit: bool,
    pub pitch: u32,
    pub uv_offset: u32,
    pub bytes: u64,
}
impl DmaLayout {
    pub fn new(width: u32, height: u32, ten_bit: bool) -> Result<Self> {
        if !(2..=8192).contains(&width)
            || !(2..=8192).contains(&height)
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
        {
            return Err("DMA-BUF dimensions must be even and within 2..8192".into());
        }
        let pitch = (width * if ten_bit { 2 } else { 1 }).div_ceil(256) * 256;
        let storage_height = height.div_ceil(32) * 32;
        let uv_offset = (pitch * storage_height).div_ceil(4096) * 4096;
        let bytes = u64::from(uv_offset) + u64::from(pitch) * u64::from(storage_height / 2);
        Ok(Self {
            width,
            height,
            ten_bit,
            pitch,
            uv_offset,
            bytes,
        })
    }
    pub fn validate(self) -> Result<()> {
        if self != Self::new(self.width, self.height, self.ten_bit)? {
            return Err("Invalid DMA-BUF plane layout".into());
        }
        Ok(())
    }
}
/// Immutable exported memory; encoder references must retain this entire object.
/// It is never written or reacquired by Vulkan after foreign ownership release.
pub struct DmaFrame {
    _buffer: wgpu::Buffer,
    fd: OwnedFd,
    _device: wgpu::Device,
    layout: DmaLayout,
    allocation_size: u64,
    render_device: u64,
    accounting: Option<Arc<dyn Send + Sync>>,
}
impl DmaFrame {
    pub fn layout(&self) -> DmaLayout {
        self.layout
    }
    pub fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;
        self.fd.as_fd()
    }
    pub fn allocation_size(&self) -> u64 {
        self.allocation_size
    }
    pub fn render_device(&self) -> u64 {
        self.render_device
    }
    pub fn retain_accounting(&mut self, owner: Arc<dyn Send + Sync>) {
        self.accounting = Some(owner);
    }
    pub fn export(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Buffer,
        layout: DmaLayout,
        render_node: &Path,
        allocation_limit: u64,
    ) -> Result<Self> {
        layout.validate()?;
        let size = layout.bytes;
        if size > allocation_limit
            || source.size() < size
            || !source.usage().contains(wgpu::BufferUsages::COPY_SRC)
        {
            return Err("Invalid DMA-BUF source or allocation budget".into());
        }
        let metadata = std::fs::metadata(render_node)?;
        if !metadata.file_type().is_char_device() {
            return Err("VA-API render node is not a device".into());
        }
        let render_device = metadata.rdev();
        // SAFETY: Retained Vulkan HAL device; handles are borrowed, never destroyed.
        let hal =
            unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }.ok_or("Vulkan device required")?;
        for extension in [
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::ext::queue_family_foreign::NAME,
        ] {
            if !hal.enabled_device_extensions().contains(&extension) {
                return Err(format!("DMA-BUF requires {}", extension.to_string_lossy()).into());
            }
        }
        let raw = hal.raw_device();
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        // SAFETY: Querying supported properties/extensions on the retained device.
        unsafe {
            let extensions = instance.enumerate_device_extension_properties(physical)?;
            if !extensions.iter().any(|e| {
                std::ffi::CStr::from_ptr(e.extension_name.as_ptr())
                    == ash::ext::physical_device_drm::NAME
            }) {
                return Err("Vulkan DRM device identification is unavailable".into());
            }
            let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
            let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
            instance.get_physical_device_properties2(physical, &mut properties);
            let major = ((render_device >> 8) & 0xfff) | ((render_device >> 32) & 0xfffff000);
            let minor = (render_device & 0xff) | ((render_device >> 12) & 0xffffff00);
            if drm.has_render != vk::TRUE
                || drm.render_major != major as i64
                || drm.render_minor != minor as i64
            {
                return Err("Vulkan and VA-API render devices do not match".into());
            }
        }
        let handles = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::TRANSFER_DST;
        let query = vk::PhysicalDeviceExternalBufferInfo::default()
            .usage(usage)
            .handle_type(handles);
        let mut properties = vk::ExternalBufferProperties::default();
        // SAFETY: Valid immutable device and output/query storage.
        unsafe {
            instance.get_physical_device_external_buffer_properties(
                physical,
                &query,
                &mut properties,
            );
        }
        if !properties
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::EXPORTABLE)
        {
            return Err("Vulkan buffer cannot export DMA-BUF memory".into());
        }
        let mut external = vk::ExternalMemoryBufferCreateInfo::default().handle_types(handles);
        let description = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .push_next(&mut external);
        // SAFETY: The descriptor contains supported usage and valid external-memory metadata.
        let buffer = unsafe { raw.create_buffer(&description, None)? };
        let mut owned = Allocation {
            device: raw.clone(),
            buffer,
            memory: vk::DeviceMemory::null(),
            pending: false,
        };
        // SAFETY: This crate exclusively owns the live buffer.
        let requirements = unsafe { raw.get_buffer_memory_requirements(buffer) };
        if requirements.size > allocation_limit {
            return Err("External allocation alignment exceeds GPU budget".into());
        }
        // SAFETY: Physical device remains live through HAL guard.
        let memories = unsafe { instance.get_physical_device_memory_properties(physical) };
        let memory_type = (0..memories.memory_type_count)
            .find(|&i| {
                requirements.memory_type_bits & (1 << i) != 0
                    && memories.memory_types[i as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .ok_or("No device-local external memory type")?;
        let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(handles);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().buffer(buffer);
        let info = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type)
            .push_next(&mut export)
            .push_next(&mut dedicated);
        // SAFETY: Dedicated allocation uses this buffer's queried size and supported memory type.
        unsafe {
            owned.memory = raw.allocate_memory(&info, None)?;
            raw.bind_buffer_memory(buffer, owned.memory, 0)?;
        }
        let exporter = ash::khr::external_memory_fd::Device::new(instance, raw);
        let info = vk::MemoryGetFdInfoKHR::default()
            .memory(owned.memory)
            .handle_type(handles);
        // SAFETY: Extension enabled, allocation exportable; success returns a new owned FD.
        let fd = unsafe { OwnedFd::from_raw_fd(exporter.get_memory_fd(&info)?) };
        // SAFETY: Transfer ownership to wgpu before using its tracked copy API.
        // wgpu initializes the new buffer and retains it through submission.
        let shared = unsafe {
            let imported = wgpu::hal::vulkan::Buffer::from_raw_managed(
                owned.buffer,
                owned.memory,
                0,
                requirements.size,
            );
            owned.buffer = vk::Buffer::null();
            owned.memory = vk::DeviceMemory::null();
            device.create_buffer_from_hal::<wgpu::hal::api::Vulkan>(
                imported,
                &wgpu::BufferDescriptor {
                    label: Some("DMA-BUF encoder handoff"),
                    size,
                    usage: wgpu::BufferUsages::COPY_DST
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                },
            )
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(source, 0, &shared, 0, size);
        let copy_commands = encoder.finish();
        let mut encoder = device.create_command_encoder(&Default::default());
        // SAFETY: Exclusive raw encoder recording after the tracked copy. Only our
        // private buffer is released to the foreign media driver; it is never reused by Vulkan.
        unsafe {
            encoder.as_hal_mut::<wgpu::hal::api::Vulkan, _, _>(|encoder| -> Result<()> {
                let encoder = encoder.ok_or("Vulkan command encoder unavailable")?;
                let barrier = vk::BufferMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::empty())
                    .src_queue_family_index(hal.queue_family_index())
                    .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                    .buffer(buffer)
                    .offset(0)
                    .size(size);
                raw.cmd_pipeline_barrier(
                    encoder.raw_handle(),
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[barrier],
                    &[],
                );
                Ok(())
            })?;
        }
        let submission = queue.submit([copy_commands, encoder.finish()]);
        if let Err(e) = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        }) {
            std::mem::forget(device.clone());
            std::mem::forget(shared);
            return Err(FatalInterop(format!("DMA-BUF release failed: {e}")).into());
        }
        Ok(Self {
            _buffer: shared,
            fd,
            _device: device.clone(),
            layout,
            allocation_size: requirements.size,
            render_device,
            accounting: None,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_are_aligned_bounded_and_reject_mutation() {
        for (w, h) in [(2, 2), (322, 182), (3840, 2160), (8192, 8192)] {
            for ten in [false, true] {
                let layout = DmaLayout::new(w, h, ten).unwrap();
                assert!(layout.pitch.is_multiple_of(256));
                assert!(layout.uv_offset.is_multiple_of(4096));
                assert!(layout.pitch >= w * if ten { 2 } else { 1 });
                assert!(layout.uv_offset >= layout.pitch * h);
                layout.validate().unwrap();
                let mut broken = layout;
                broken.bytes -= 1;
                assert!(broken.validate().is_err());
            }
        }
        assert!(DmaLayout::new(321, 180, false).is_err());
        assert!(DmaLayout::new(0, 180, false).is_err());
    }
}
