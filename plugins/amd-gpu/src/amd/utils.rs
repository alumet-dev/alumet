use amd_smi_lib_sys::bindings::*;
use std::{
    ffi::CStr,
    mem::{MaybeUninit, transmute, zeroed},
    os::raw::c_char,
    ptr::null_mut,
};

/// Memories values available.
pub const MEMORY_TYPE: [(amdsmi_memory_type_t, &str); 2] = [
    (
        amdsmi_memory_type_t_AMDSMI_MEM_TYPE_GTT,
        "memory_graphic_translation_table",
    ),
    (amdsmi_memory_type_t_AMDSMI_MEM_TYPE_VRAM, "memory_video_computing"),
];

/// Temperature sensors values available.
pub const SENSOR_TYPE: [(amdsmi_temperature_type_t, &str); 7] = [
    (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE, "thermal_global"),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HOTSPOT,
        "thermal_hotspot",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_0,
        "thermal_high_bandwidth_memory_0",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_1,
        "thermal_high_bandwidth_memory_1",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_2,
        "thermal_high_bandwidth_memory_2",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_3,
        "thermal_high_bandwidth_memory_3",
    ),
    (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_PLX, "thermal_pci_bus"),
];

/// Call the unsafe C binding function [`amdsmi_init`] to quit amd-smi library and clean properly its resources.
///
/// # Arguments
///
/// - `amdsmi_init_flags_t`: A [`amdsmi_init_flags_t`] type value use to define how AMD hardware we need to initialize (GPU, CPU).
///
/// # Returns
///
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value
pub fn amd_sys_init(init_flag: amdsmi_init_flags_t) -> Result<(), amdsmi_status_t> {
    let result = unsafe { amdsmi_init(init_flag.into()) };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_shut_down`] to quit amd-smi library and clean properly its resources.
///
/// # Returns
///
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value
pub fn amd_sys_shutdown() -> Result<(), amdsmi_status_t> {
    let result = unsafe { amdsmi_shut_down() };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_socket_handles`] to retrieve socket handles detected on system.
///
/// # Returns
///
/// - Set of [`amdsmi_socket_handle`] pointer to a block of memory to which values will be written.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value
pub fn get_socket_handles() -> Result<Vec<amdsmi_socket_handle>, amdsmi_status_t> {
    unsafe {
        let mut socket_count = 0;
        let result = amdsmi_get_socket_handles(&mut socket_count, null_mut());
        if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            return Err(result);
        }

        let mut socket_handles = vec![zeroed(); socket_count as usize];

        let result = amdsmi_get_socket_handles(&mut socket_count, socket_handles.as_mut_ptr());
        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            socket_handles.truncate(socket_count as usize);
            Ok(socket_handles)
        } else {
            Err(result)
        }
    }
}

/// Call the unsafe C binding function [`amdsmi_get_processor_handles`] to retrieve socket handles detected for a give socket.
///
/// # Arguments
///
/// - `amdsmi_socket_handle`: The socket to query.
///
/// # Returns
///
/// - Set of [`amdsmi_processor_handle`] of pointer to a block of memory to which values will be written.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value
pub fn get_processor_handles(
    socket_handle: amdsmi_socket_handle,
) -> Result<Vec<amdsmi_processor_handle>, amdsmi_status_t> {
    unsafe {
        let mut processor_count = 0;
        let result = amdsmi_get_processor_handles(socket_handle, &mut processor_count, null_mut());
        if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            return Err(result);
        }

        let mut processor_handles = vec![zeroed(); processor_count as usize];

        let result = amdsmi_get_processor_handles(socket_handle, &mut processor_count, processor_handles.as_mut_ptr());
        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            processor_handles.truncate(processor_count as usize);
            Ok(processor_handles)
        } else {
            Err(result)
        }
    }
}

/// Call the unsafe C binding function [`amdsmi_get_gpu_device_uuid`] to retrieve gpu uuid identifier values.
/// Convert a declared buffer with an [`AMDSMI_GPU_UUID_SIZE`] in UTF-8 Rust string.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
///
/// # Returns
///
/// - The formatted string corresponding of UUID of a gpu device.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_uuid(processor_handle: amdsmi_processor_handle) -> Result<String, amdsmi_status_t> {
    unsafe {
        let mut uuid_buffer = vec![0 as c_char; AMDSMI_GPU_UUID_SIZE as usize];
        let mut uuid_length = AMDSMI_GPU_UUID_SIZE;
        let result = amdsmi_get_gpu_device_uuid(processor_handle, &mut uuid_length, uuid_buffer.as_mut_ptr());

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            let c_str = CStr::from_ptr(uuid_buffer.as_ptr());
            match c_str.to_str() {
                Ok(uuid_str) => Ok(uuid_str.to_owned()),
                Err(_) => Err(amdsmi_status_t_AMDSMI_STATUS_DRM_ERROR),
            }
        } else {
            Err(result)
        }
    }
}

/// Call the unsafe C binding function [`amdsmi_get_gpu_activity`] to retrieve gpu activity values.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
///
/// # Returns
///
/// - `gfx`: Main graphic unit of an AMD GPU that release graphic tasks and rendering in %.
/// - `mm`: Unit responsible for managing and accessing VRAM, and coordinating data exchanges between it and the GPU in %.
/// - `umc`: Single memory address space accessible from any processor within a system in %.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value
pub fn get_device_activity(
    processor_handle: amdsmi_processor_handle,
) -> Result<amdsmi_engine_usage_t, amdsmi_status_t> {
    let mut info = MaybeUninit::<amdsmi_engine_usage_t>::uninit();

    let result = unsafe { amdsmi_get_gpu_activity(processor_handle, info.as_mut_ptr()) };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        let info = unsafe { info.assume_init() };
        Ok(info)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_energy_count`] to retrieve gpu energy consumption values.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
///
/// # Returns
///
/// - `energy`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value.
/// - `resolution`: Resolution precision of the energy counter in micro Joules.
/// - `timestamp: Timestamp returned in ns.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value
pub fn get_device_energy(processor_handle: amdsmi_processor_handle) -> Result<(u64, f32, u64), amdsmi_status_t> {
    let mut energy = 0;
    let mut resolution = 0.0;
    let mut timestamp = 0;

    let result = unsafe {
        amdsmi_get_energy_count(
            processor_handle,
            &mut energy as *mut u64,
            &mut resolution as *mut f32,
            &mut timestamp as *mut u64,
        )
    };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok((energy, resolution, timestamp))
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_gpu_memory_usage`] to retrieve gpu memories used values.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
/// - `mem_type`: Kind of memory used among [`amdsmi_memory_type_t`].
///
/// # Returns
///
/// - `used`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in Bytes.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_memory_usage(
    processor_handle: amdsmi_processor_handle,
    mem_type: amdsmi_memory_type_t,
) -> Result<u64, amdsmi_status_t> {
    let mut used = 0;
    let result = unsafe { amdsmi_get_gpu_memory_usage(processor_handle, mem_type, &mut used) };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok(used)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_power_info`] to retrieve [`RSMI_POWER_TYPE`] gpu power consumption values.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
///
/// # Returns
///
/// - `power`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in µW.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_power(processor_handle: amdsmi_processor_handle) -> Result<amdsmi_power_info_t, amdsmi_status_t> {
    unsafe {
        let mut info: amdsmi_power_info_t = std::mem::zeroed();
        let result = amdsmi_get_power_info(processor_handle, &mut info);

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            Ok(info)
        } else {
            Err(result)
        }
    }
}

/// Call the unsafe C binding function [`amdsmi_is_gpu_power_management_enabled`] to retrieve gpu state flag to enable the power consumption evaluation.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
///
/// # Returns
///
/// - `enabled`: Pointer for C binding function, to allow it to allocate memory to get its corresponding boolean value.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_power_managment(processor_handle: amdsmi_processor_handle) -> Result<bool, amdsmi_status_t> {
    let mut enabled = false;
    let result = unsafe { amdsmi_is_gpu_power_management_enabled(processor_handle, &mut enabled) };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok(enabled)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_temp_metric`] to retrieve gpu temperature values.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
/// - `sensor_type`: Temperature retrieved by a [`amdsmi_temperature_metric_t`] sensor on AMD GPU hardware.
/// - `metric`: Temperature type [`amdsmi_temperature_metric_t`] analysed (current, average...).
///
/// # Returns
///
/// - `temperature`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in °C.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_temperature(
    processor_handle: amdsmi_processor_handle,
    sensor_type: amdsmi_temperature_type_t,
    metric: amdsmi_temperature_metric_t,
) -> Result<i64, amdsmi_status_t> {
    let mut temperature = 0;
    let result = unsafe { amdsmi_get_temp_metric(processor_handle, sensor_type, metric, &mut temperature) };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok(temperature)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_gpu_volt_metric`] to retrieve gpu socket voltage values.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
/// - `sensor_type`: Voltage retrieved by a [`amdsmi_voltage_type_t`] sensor on AMD GPU hardware.
/// - `metric`: Voltage type [`amdsmi_voltage_metric_t`] analysed (current, average...).
///
/// # Returns
///
/// - `voltage`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in mV.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_voltage(
    processor_handle: amdsmi_processor_handle,
    sensor_type: amdsmi_voltage_type_t,
    metric: amdsmi_voltage_metric_t,
) -> Result<i64, amdsmi_status_t> {
    let mut voltage = 0;
    let result = unsafe { amdsmi_get_gpu_volt_metric(processor_handle, sensor_type, metric, &mut voltage) };

    if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
        Ok(voltage)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`amdsmi_get_gpu_process_list`] to retrieve data about running compute processes.
///
/// # Arguments
///
/// - `processor_handle`: Address pointer on a AMD GPU device.
///
/// # Returns
///
/// - A vec of [`amdsmi_proc_info_t`] data concerning retrieved processes.
/// - If no processes are running we return an empty result.
/// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
pub fn get_device_process_list(
    processor_handle: amdsmi_processor_handle,
) -> Result<Vec<amdsmi_proc_info_t>, amdsmi_status_t> {
    let mut max_processes = 64;
    let mut process_list = Vec::with_capacity(max_processes as usize);
    let list = process_list.as_mut_ptr() as *mut amdsmi_proc_info_t;

    let result = unsafe { amdsmi_get_gpu_process_list(processor_handle, &mut max_processes, list) };
    if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS && result != amdsmi_status_t_AMDSMI_STATUS_OUT_OF_RESOURCES {
        return Err(result);
    }

    unsafe {
        process_list.set_len(max_processes as usize);
    }

    let process_info_list =
        unsafe { transmute::<Vec<MaybeUninit<amdsmi_proc_info_t>>, Vec<amdsmi_proc_info_t>>(process_list) };

    Ok(process_info_list)
}
