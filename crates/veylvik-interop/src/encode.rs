//! Exclusive Vulkan-to-CUDA handoff for encoder frame planes.
use super::*;
/// A uniquely writable CUDA destination. The lifetime prevents submitting its
/// owning encoder frame while a transfer is in progress.
pub struct CudaDestination<'a> {
    frame: CudaFrame,
    written: bool,
    _borrow: std::marker::PhantomData<&'a mut ()>,
}
impl<'a> CudaDestination<'a> {
    /// # Safety
    /// The owner must retain valid writable planes/context/stream, with the same
    /// geometry requirements as CudaFrame::from_raw. No other access to these
    /// planes may occur until this destination is dropped. Previous writes must
    /// be complete or ordered on the supplied stream.
    pub unsafe fn from_raw(
        context: *mut c_void,
        stream: *mut c_void,
        planes: [(u64, usize); 2],
        size: [u32; 2],
        sample_bytes: u32,
        owner: &'a mut Arc<dyn Send + Sync>,
    ) -> Result<Self> {
        // SAFETY: This constructor's caller guarantees the retained plane ranges.
        let frame = unsafe {
            CudaFrame::from_raw(context, stream, planes, size, sample_bytes, owner.clone())?
        };
        Ok(Self {
            frame,
            written: false,
            _borrow: std::marker::PhantomData,
        })
    }
    pub fn is_written(&self) -> bool {
        self.written
    }
    pub fn dimensions(&self) -> [u32; 2] {
        self.frame.dimensions()
    }
    pub fn sample_bytes(&self) -> u32 {
        self.frame.sample_bytes()
    }
    /// Copy tightly packed GPU NV12/P010 into the pitched encoder allocation.
    /// Host waits establish completion, but no pixel data is mapped on the host.
    pub fn copy_from_vulkan(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Buffer,
        allocation_limit: u64,
    ) -> Result<()> {
        self.written = false;
        let size = self.frame.byte_len().div_ceil(4) * 4;
        if size > allocation_limit
            || source.size() < size
            || !source.usage().contains(wgpu::BufferUsages::COPY_SRC)
        {
            return Err("Invalid encoder GPU source or allocation budget".into());
        }
        // SAFETY: The HAL guard retains the device; no borrowed handle is destroyed.
        let hal =
            unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }.ok_or("Vulkan device required")?;
        if !hal
            .enabled_device_extensions()
            .contains(&ash::khr::external_memory_fd::NAME)
        {
            return Err("Vulkan external memory FD is unavailable".into());
        }
        let raw = hal.raw_device();
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        let mut ids = vk::PhysicalDeviceIDProperties::default();
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut ids);
        // SAFETY: Physical device belongs to the retained instance; output chain is valid.
        unsafe {
            instance.get_physical_device_properties2(physical, &mut properties);
        }
        same_device(self.frame.uuid()?, ids.device_uuid)?;
        let handles = vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD;
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
            return Err("Vulkan buffer cannot export CUDA-compatible memory".into());
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
                    label: Some("GPU encoder handoff"),
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
        // private buffer is released to CUDA; it is never reused by Vulkan.
        unsafe {
            encoder.as_hal_mut::<wgpu::hal::api::Vulkan, _, _>(|encoder| -> Result<()> {
                let encoder = encoder.ok_or("Vulkan command encoder unavailable")?;
                let barrier = vk::BufferMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::empty())
                    .src_queue_family_index(hal.queue_family_index())
                    .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
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
            return Err(FatalInterop(format!("Vulkan encoder release failed: {e}")).into());
        }
        let result = self.copy_external(fd, requirements.size, size);
        if result.as_ref().is_err_and(|e| e.is::<FatalInterop>()) {
            std::mem::forget(device.clone());
            std::mem::forget(shared);
        }
        self.written = result.is_ok();
        result
    }
    fn copy_external(&mut self, fd: OwnedFd, allocation_size: u64, size: u64) -> Result<()> {
        let api = api()?;
        let _context = Context::enter(api, self.frame.context)?;
        let import = api.cuImportExternalMemory.as_ref()?;
        let map = api.cuExternalMemoryGetMappedBuffer.as_ref()?;
        let copy = api.cuMemcpy2DAsync_v2.as_ref()?;
        let sync = api.cuStreamSynchronize.as_ref()?;
        let free = api.cuMemFree_v2.as_ref()?;
        let destroy = api.cuDestroyExternalMemory.as_ref()?;
        let mut external = ptr::null_mut();
        let desc = cu::CUDA_EXTERNAL_MEMORY_HANDLE_DESC {
            type_: cu::CUexternalMemoryHandleType::CU_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD,
            handle: cu::CUDA_EXTERNAL_MEMORY_HANDLE_DESC_st__bindgen_ty_1 { fd: fd.as_raw_fd() },
            size: allocation_size,
            flags: 1,
            ..Default::default()
        };
        // SAFETY: Matching UUID, dedicated exportable Vulkan allocation, completed
        // release and owned FD. CUDA consumes the FD on successful import only.
        unsafe {
            check(import(&mut external, &desc))?;
        }
        let _consumed = fd.into_raw_fd();
        let mut source = 0;
        let result = (|| {
            let desc = cu::CUDA_EXTERNAL_MEMORY_BUFFER_DESC {
                offset: 0,
                size,
                ..Default::default()
            };
            // SAFETY: Mapping lies inside the retained external allocation.
            unsafe {
                check(map(&mut source, external, &desc))?;
            }
            let row = (self.frame.width * self.frame.sample_bytes) as usize;
            for (plane, rows, offset) in [
                (0, self.frame.height as usize, 0),
                (
                    1,
                    self.frame.height as usize / 2,
                    row * self.frame.height as usize,
                ),
            ] {
                let desc = cu::CUDA_MEMCPY2D {
                    srcMemoryType: cu::CUmemorytype::CU_MEMORYTYPE_DEVICE,
                    srcDevice: source + offset as u64,
                    srcPitch: row,
                    dstMemoryType: cu::CUmemorytype::CU_MEMORYTYPE_DEVICE,
                    dstDevice: self.frame.planes[plane].0,
                    dstPitch: self.frame.planes[plane].1,
                    WidthInBytes: row,
                    Height: rows,
                    ..Default::default()
                };
                // SAFETY: Source offsets fit the mapped buffer; destination
                // pitches/ranges and exclusive writes are constructor contracts.
                unsafe {
                    check(copy(&desc, self.frame.stream.cast()))?;
                }
            }
            Ok(())
        })();
        // SAFETY: Wait even on partial failure before dropping either allocation.
        if let Err(e) = unsafe { check(sync(self.frame.stream.cast())) } {
            std::mem::forget(self.frame._owner.clone());
            return Err(FatalInterop(format!("CUDA encoder copy completion failed: {e}")).into());
        }
        // SAFETY: Completed copies; mapping and external object exclusively owned.
        let cleanup = unsafe {
            let freed = if source != 0 {
                check(free(source))
            } else {
                Ok(())
            };
            freed.and_then(|()| check(destroy(external)))
        };
        if let Err(e) = cleanup {
            std::mem::forget(self.frame._owner.clone());
            return Err(FatalInterop(format!("CUDA encoder mapping cleanup failed: {e}")).into());
        }
        result
    }
}
