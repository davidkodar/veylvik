//! Optional foreign queue ownership for DMA-BUF media consumers.
use super::*;
pub async fn request_device(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Result<(wgpu::Device, wgpu::Queue)> {
    if !adapter.features().contains(descriptor.required_features)
        || !descriptor.required_limits.check_limits(&adapter.limits())
    {
        return Err("Requested Vulkan features or limits are unsupported".into());
    }
    // SAFETY: The guard retains this adapter; we create a device from precisely
    // this HAL adapter, with the same supported features/limits as the descriptor.
    let opened = unsafe {
        let hal = adapter
            .as_hal::<wgpu::hal::api::Vulkan>()
            .ok_or("Vulkan adapter required")?;
        if hal
            .physical_device_capabilities()
            .supports_extension(ash::ext::queue_family_foreign::NAME)
        {
            hal.open_with_callback(
                descriptor.required_features,
                &descriptor.required_limits,
                &descriptor.memory_hints,
                Some(Box::new(|args| {
                    if !args
                        .extensions
                        .contains(&ash::ext::queue_family_foreign::NAME)
                    {
                        args.extensions.push(ash::ext::queue_family_foreign::NAME);
                    }
                })),
            )
            .ok()
        } else {
            None
        }
    };
    match opened {
        // SAFETY: Opened above from the same adapter, preserving all HAL features
        // and adding only an advertised extension with no feature structs.
        Some(opened) => Ok(unsafe {
            adapter.create_device_from_hal::<wgpu::hal::api::Vulkan>(opened, descriptor)?
        }),
        None => Ok(adapter.request_device(descriptor).await?),
    }
}
