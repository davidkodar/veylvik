//! CUDA/Vulkan GPU buffer handoffs. Pixels never visit the host.
//! Each direction waits for completion before the receiving API uses the planes.
//! Only synchronization waits run on the host; this is not an asynchronous bridge.
use ash::vk;
use cudarc::driver::sys as cu;
use std::{
    ffi::c_void,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    ptr,
    sync::{Arc, OnceLock},
};
pub mod ofx;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// Completion could not be established. Callers must stop using this renderer
/// rather than retry transfers and accumulate unreclaimable GPU allocations.
#[derive(Debug)]
pub struct FatalInterop(pub String);
impl std::fmt::Display for FatalInterop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for FatalInterop {}
fn same_device(cuda: [u8; 16], vulkan: [u8; 16]) -> Result<()> {
    if cuda == [0; 16] || cuda != vulkan {
        return Err("CUDA and Vulkan device UUIDs do not match".into());
    }
    Ok(())
}
fn check(result: cu::CUresult) -> Result<()> {
    if result == cu::CUresult::CUDA_SUCCESS {
        Ok(())
    } else {
        Err(format!("CUDA interop: {result:?}").into())
    }
}
fn api() -> Result<&'static cu::Lib> {
    static API: OnceLock<std::result::Result<cu::Lib, String>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: Load only the driver ABI, retaining the library for process life.
        unsafe { cu::Lib::new("libcuda.so.1") }.map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| e.clone().into())
}
struct Context(&'static cu::Lib);
impl Context {
    fn enter(api: &'static cu::Lib, context: *mut c_void) -> Result<Self> {
        // Resolve the matching pop before changing thread-local state.
        api.cuCtxPopCurrent_v2.as_ref()?;
        // SAFETY: Constructor contract retains this CUDA context through owner.
        unsafe {
            check((api.cuCtxPushCurrent_v2.as_ref()?)(context.cast()))?;
        }
        Ok(Self(api))
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        if let Ok(pop) = &self.0.cuCtxPopCurrent_v2 {
            let mut previous = ptr::null_mut();
            // SAFETY: Balances our one successful push on this thread.
            unsafe {
                pop(&mut previous);
            }
        }
    }
}
/// Immutable hardware planes, with their context and allocation owner retained.
pub struct CudaFrame {
    context: *mut c_void,
    stream: *mut c_void,
    planes: [(u64, usize); 2],
    width: u32,
    height: u32,
    sample_bytes: u32,
    _owner: Arc<dyn Send + Sync>,
}
// SAFETY: Owned immutable planes survive all operations. Each calling thread
// pushes/pops the context. CUDA synchronizes shared stream access internally.
unsafe impl Send for CudaFrame {}
// SAFETY: Operations only read the retained source and create private destinations.
unsafe impl Sync for CudaFrame {}
impl CudaFrame {
    /// # Safety
    /// `owner` must keep the context, stream and both immutable device planes
    /// alive. Each plane must span pitch * rows bytes (height, height/2 rows).
    /// Decode writes must be complete or ordered before reads on `stream`.
    pub unsafe fn from_raw(
        context: *mut c_void,
        stream: *mut c_void,
        planes: [(u64, usize); 2],
        size: [u32; 2],
        sample_bytes: u32,
        owner: Arc<dyn Send + Sync>,
    ) -> Result<Self> {
        let [width, height] = size;
        if context.is_null()
            || width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width % 2 != 0
            || height % 2 != 0
            || ![1, 2].contains(&sample_bytes)
            || planes
                .iter()
                .any(|(p, pitch)| *p == 0 || *pitch < (width * sample_bytes) as usize)
        {
            return Err("Invalid CUDA frame planes".into());
        }
        Ok(Self {
            context,
            stream,
            planes,
            width,
            height,
            sample_bytes,
            _owner: owner,
        })
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.width, self.height]
    }
    pub fn sample_bytes(&self) -> u32 {
        self.sample_bytes
    }
    pub fn byte_len(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * u64::from(self.sample_bytes) * 3 / 2
    }
    pub fn uuid(&self) -> Result<[u8; 16]> {
        let api = api()?;
        let _context = Context::enter(api, self.context)?;
        let mut device = 0;
        let mut uuid = cu::CUuuid { bytes: [0; 16] };
        // SAFETY: Current context is retained, outputs are valid stack storage.
        unsafe {
            check((api.cuCtxGetDevice.as_ref()?)(&mut device))?;
            check((api.cuDeviceGetUuid.as_ref()?)(&mut uuid, device))?;
        }
        Ok(uuid.bytes.map(|b| b as u8))
    }
    /// Returns a completed, initialized buffer owned by the supplied wgpu device.
    /// Rejects mismatched GPUs and unavailable capabilities before transferring.
    pub fn into_vulkan(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        allocation_limit: u64,
    ) -> Result<wgpu::Buffer> {
        let size = self.byte_len().div_ceil(4) * 4;
        if size > allocation_limit
            || size > device.limits().max_buffer_size
            || size > device.limits().max_storage_buffer_binding_size
        {
            return Err("Interop buffer exceeds GPU budget or limits".into());
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
        same_device(self.uuid()?, ids.device_uuid)?;
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
        if let Err(e) = self.copy_to_external(fd, requirements.size, size, &mut owned) {
            if owned.pending {
                std::mem::forget(device.clone());
                std::mem::forget(owned);
            }
            return Err(e);
        }
        // CUDA is complete and its mapping is gone. Acquire external ownership
        // on wgpu's own synchronized queue; never submit directly to its VkQueue.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("acquire decoded CUDA planes"),
        });
        // SAFETY: Exclusive encoder recording, valid live buffer, no concurrent
        // CUDA access. This only changes ownership of our private external buffer.
        unsafe {
            encoder.as_hal_mut::<wgpu::hal::api::Vulkan, _, _>(|encoder| -> Result<()> {
                let encoder = encoder.ok_or("Vulkan command encoder unavailable")?;
                let barrier = vk::BufferMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::TRANSFER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                    .dst_queue_family_index(hal.queue_family_index())
                    .buffer(buffer)
                    .offset(0)
                    .size(size);
                raw.cmd_pipeline_barrier(
                    encoder.raw_handle(),
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[barrier],
                    &[],
                );
                Ok(())
            })?;
        }
        let submission = queue.submit([encoder.finish()]);
        // If the device cannot confirm completion, don't free objects referenced
        // by the submitted raw command buffer. Leak only this failed handoff.
        if let Err(e) = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        }) {
            std::mem::forget(device.clone());
            std::mem::forget(owned);
            return Err(FatalInterop(format!("CUDA/Vulkan acquisition failed: {e}")).into());
        }
        // SAFETY: Data and ownership are initialized. CUDA has relinquished all
        // access. wgpu takes sole ownership of buffer and dedicated memory.
        let imported = unsafe {
            let buffer = wgpu::hal::vulkan::Buffer::from_raw_managed(
                owned.buffer,
                owned.memory,
                0,
                requirements.size,
            );
            owned.buffer = vk::Buffer::null();
            owned.memory = vk::DeviceMemory::null();
            device.create_buffer_from_hal::<wgpu::hal::api::Vulkan>(
                buffer,
                &wgpu::BufferDescriptor {
                    label: Some("GPU decoded planes"),
                    size,
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                },
            )
        };
        Ok(imported)
    }
    fn copy_to_external(
        &self,
        fd: OwnedFd,
        allocation_size: u64,
        buffer_size: u64,
        allocation: &mut Allocation,
    ) -> Result<()> {
        let api = api()?;
        let _context = Context::enter(api, self.context)?;
        // Resolve every required entry point before accepting ownership or work.
        let import = api.cuImportExternalMemory.as_ref()?;
        let map = api.cuExternalMemoryGetMappedBuffer.as_ref()?;
        let memset = api.cuMemsetD8Async.as_ref()?;
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
        // SAFETY: FD owns a dedicated Vulkan allocation with matching size;
        // CUDA consumes the FD only on success. All pointers are valid outputs.
        unsafe {
            check(import(&mut external, &desc))?;
        }
        let _consumed = fd.into_raw_fd();
        let mut target = 0;
        let result = (|| {
            let desc = cu::CUDA_EXTERNAL_MEMORY_BUFFER_DESC {
                offset: 0,
                size: buffer_size,
                ..Default::default()
            };
            // SAFETY: Mapping stays within the dedicated external allocation.
            unsafe {
                check(map(&mut target, external, &desc))?;
            }
            let row = (self.width * self.sample_bytes) as usize;
            // SAFETY: Target spans buffer_size bytes; padding is initialized too.
            unsafe {
                check(memset(target, 0, buffer_size as usize, self.stream.cast()))?;
            }
            for (plane, rows, offset) in [
                (0, self.height as usize, 0),
                (1, self.height as usize / 2, row * self.height as usize),
            ] {
                let desc = cu::CUDA_MEMCPY2D {
                    srcMemoryType: cu::CUmemorytype::CU_MEMORYTYPE_DEVICE,
                    srcDevice: self.planes[plane].0,
                    srcPitch: self.planes[plane].1,
                    dstMemoryType: cu::CUmemorytype::CU_MEMORYTYPE_DEVICE,
                    dstDevice: target + offset as u64,
                    dstPitch: row,
                    WidthInBytes: row,
                    Height: rows,
                    ..Default::default()
                };
                // SAFETY: Source validity is constructor's contract; tightly
                // packed target offsets and rows are within the mapped allocation.
                unsafe {
                    check(copy(&desc, self.stream.cast()))?;
                }
            }
            Ok(())
        })();
        // SAFETY: Complete all queued copies even on error before unmapping;
        // the owner keeps source context and planes alive until this returns.
        let completed = unsafe { check(sync(self.stream.cast())) };
        if let Err(e) = completed {
            // No safe reclamation without completion. Retain source ownership too.
            allocation.pending = true;
            std::mem::forget(self._owner.clone());
            return Err(FatalInterop(format!("CUDA completion failed: {}", e)).into());
        }
        // SAFETY: Work completed, mapping and external object are uniquely owned.
        unsafe {
            if target != 0 {
                check(free(target))?;
            }
            check(destroy(external))?;
        }
        result
    }
}
struct Allocation {
    device: ash::Device,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    pending: bool,
}
impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: Only pre-submit or confirmed-complete allocations reach Drop;
        // successful handoff nulls these handles and transfers ownership to wgpu.
        unsafe {
            self.device.destroy_buffer(self.buffer, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unrelated_or_unknown_gpu_identities_are_rejected() {
        assert!(super::same_device([7; 16], [7; 16]).is_ok());
        assert!(super::same_device([7; 16], [8; 16]).is_err());
        assert!(super::same_device([0; 16], [0; 16]).is_err());
    }
}

mod encode;
pub use encode::CudaDestination;

mod device;
pub use device::request_device;
mod dma;
pub use dma::{DmaFrame, DmaLayout};
/// A pipeline cache for `device`, started from `data` when given: compiled GPU programs
/// kept between runs, so a driver need not compile them again. None when the device was
/// not made with `wgpu::Features::PIPELINE_CACHE`. Data that the driver does not accept
/// (another driver, another card) starts an empty cache instead.
pub fn pipeline_cache(device: &wgpu::Device, data: Option<&[u8]>) -> Option<wgpu::PipelineCache> {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    // SAFETY: wgpu requires `data` to be what an earlier `PipelineCache::get_data` gave.
    // Callers pass only bytes they stored from `get_data` and checked whole on reading
    // (veylvik-render keeps a length and a checksum beside them); `fallback` makes the
    // driver's own header check (vendor, device, driver build) start afresh on a mismatch.
    Some(unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("Veylvik"),
            data,
            fallback: true,
        })
    })
}
