use crate::bindings::*;
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
pub fn amd_sys_init(amdsmi: &libamd_smi, init_flag: amdsmi_init_flags_t) -> Result<(), amdsmi_status_t> {
    let result = unsafe { amdsmi.amdsmi_init(init_flag.into()) };

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
pub fn amd_sys_shutdown(amdsmi: &libamd_smi) -> Result<(), amdsmi_status_t> {
    let result = unsafe { amdsmi.amdsmi_shut_down() };

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
pub fn get_socket_handles(amdsmi: &libamd_smi) -> Result<Vec<amdsmi_socket_handle>, amdsmi_status_t> {
    unsafe {
        let mut socket_count = 0;
        let result = amdsmi.amdsmi_get_socket_handles(&mut socket_count, null_mut());
        if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            return Err(result);
        }

        let mut socket_handles = vec![zeroed(); socket_count as usize];

        let result = amdsmi.amdsmi_get_socket_handles(&mut socket_count, socket_handles.as_mut_ptr());
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
    amdsmi: &libamd_smi,
    socket_handle: amdsmi_socket_handle,
) -> Result<Vec<amdsmi_processor_handle>, amdsmi_status_t> {
    unsafe {
        let mut processor_count = 0;
        let result = amdsmi.amdsmi_get_processor_handles(socket_handle, &mut processor_count, null_mut());
        if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
            return Err(result);
        }

        let mut processor_handles = vec![zeroed(); processor_count as usize];

        let result =
            amdsmi.amdsmi_get_processor_handles(socket_handle, &mut processor_count, processor_handles.as_mut_ptr());
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
pub fn get_device_uuid(
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
) -> Result<String, amdsmi_status_t> {
    unsafe {
        let mut uuid_buffer = vec![0 as c_char; AMDSMI_GPU_UUID_SIZE as usize];
        let mut uuid_length = AMDSMI_GPU_UUID_SIZE;
        let result = amdsmi.amdsmi_get_gpu_device_uuid(processor_handle, &mut uuid_length, uuid_buffer.as_mut_ptr());

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
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
) -> Result<amdsmi_engine_usage_t, amdsmi_status_t> {
    let mut info = MaybeUninit::<amdsmi_engine_usage_t>::uninit();

    let result = unsafe { amdsmi.amdsmi_get_gpu_activity(processor_handle, info.as_mut_ptr()) };

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
pub fn get_device_energy(
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
) -> Result<(u64, f32, u64), amdsmi_status_t> {
    let mut energy = 0;
    let mut resolution = 0.0;
    let mut timestamp = 0;

    let result = unsafe {
        amdsmi.amdsmi_get_energy_count(
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
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
    mem_type: amdsmi_memory_type_t,
) -> Result<u64, amdsmi_status_t> {
    let mut used = 0;
    let result = unsafe { amdsmi.amdsmi_get_gpu_memory_usage(processor_handle, mem_type, &mut used) };

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
pub fn get_device_power(
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
) -> Result<amdsmi_power_info_t, amdsmi_status_t> {
    unsafe {
        let mut info: amdsmi_power_info_t = std::mem::zeroed();
        let result = amdsmi.amdsmi_get_power_info(processor_handle, &mut info);

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
pub fn get_device_power_managment(
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
) -> Result<bool, amdsmi_status_t> {
    let mut enabled = false;
    let result = unsafe { amdsmi.amdsmi_is_gpu_power_management_enabled(processor_handle, &mut enabled) };

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
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
    sensor_type: amdsmi_temperature_type_t,
    metric: amdsmi_temperature_metric_t,
) -> Result<i64, amdsmi_status_t> {
    let mut temperature = 0;
    let result = unsafe { amdsmi.amdsmi_get_temp_metric(processor_handle, sensor_type, metric, &mut temperature) };

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
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
    sensor_type: amdsmi_voltage_type_t,
    metric: amdsmi_voltage_metric_t,
) -> Result<i64, amdsmi_status_t> {
    let mut voltage = 0;
    let result = unsafe { amdsmi.amdsmi_get_gpu_volt_metric(processor_handle, sensor_type, metric, &mut voltage) };

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
    amdsmi: &libamd_smi,
    processor_handle: amdsmi_processor_handle,
) -> Result<Vec<amdsmi_proc_info_t>, amdsmi_status_t> {
    let mut max_processes = 64;
    let mut process_list = Vec::with_capacity(max_processes as usize);
    let list = process_list.as_mut_ptr() as *mut amdsmi_proc_info_t;

    let result = unsafe { amdsmi.amdsmi_get_gpu_process_list(processor_handle, &mut max_processes, list) };
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

#[cfg(test)]
mod tests_utils {
    use super::*;
    use std::{
        ffi::{CString, c_void},
        mem::zeroed,
    };

    use crate::{
        load_amdsmi,
        tests::common::ffi_mock::{
            ffi_mocks_activity_usage::set_mock_activity_usage,
            ffi_mocks_energy_consumption::set_mock_energy_consumption, ffi_mocks_init::set_mock_init,
            ffi_mocks_memory_usage::set_mock_memory_usage, ffi_mocks_power_consumption::set_mock_power_consumption,
            ffi_mocks_power_management_status::set_mock_power_management_status,
            ffi_mocks_process_list::set_mock_process_list, ffi_mocks_processor_handles::set_mock_processor_handles,
            ffi_mocks_socket_handles::set_mock_socket_handles, ffi_mocks_temperature::set_mock_temperature,
            ffi_mocks_uuid::set_mock_uuid, ffi_mocks_voltage_consumption::set_mock_voltage_consumption,
        },
    };

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
    const INIT_FLAG: amdsmi_init_flags_t = amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS;
    const UTF8_ERR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_DRM_ERROR;
    const TIMESTAMP: u64 = 1708236479191334820;

    // Test `amd_sys_init` function in success case
    #[test]
    fn test_amd_sys_init_success() -> anyhow::Result<()> {
        set_mock_init(SUCCESS);
        let res = amd_sys_init(load_amdsmi()?, INIT_FLAG);
        assert!(res.is_ok());
        Ok(())
    }

    // Test `amd_sys_init` function in error case
    #[test]
    fn test_amd_sys_init_error() -> anyhow::Result<()> {
        set_mock_init(ERROR);
        let res = amd_sys_init(load_amdsmi()?, INIT_FLAG);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `amd_sys_shutdown` function in success case
    #[test]
    fn test_amd_sys_shutdown_success() -> anyhow::Result<()> {
        set_mock_init(SUCCESS);
        let res = amd_sys_shutdown(load_amdsmi()?);
        assert!(res.is_ok());
        Ok(())
    }

    // Test `amd_sys_shutdown` function in error case
    #[test]
    fn test_amd_sys_shutdown_error() -> anyhow::Result<()> {
        set_mock_init(ERROR);
        let res = amd_sys_shutdown(load_amdsmi()?);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_socket_handles` function in success case
    #[test]
    fn test_get_socket_handles_success() -> anyhow::Result<()> {
        set_mock_socket_handles(3, SUCCESS, SUCCESS);
        let res = get_socket_handles(load_amdsmi()?);
        assert!(res.is_ok());

        let sockets = res.unwrap();
        let expected = vec![0 as *mut c_void, 1 as *mut c_void, 2 as *mut c_void];
        assert_eq!(sockets, expected);
        Ok(())
    }

    // Test `get_socket_handles` function with no handles
    #[test]
    fn test_get_socket_handles_empty() -> anyhow::Result<()> {
        set_mock_socket_handles(0, SUCCESS, SUCCESS);
        let res = get_socket_handles(load_amdsmi()?);
        assert!(res.is_ok());
        assert!(res.unwrap().is_empty());
        Ok(())
    }

    // Test `get_socket_handles` function in error case at first call
    #[test]
    fn test_get_socket_handles_error_first_call() -> anyhow::Result<()> {
        set_mock_socket_handles(0, ERROR, SUCCESS);
        let res = get_socket_handles(load_amdsmi()?);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_socket_handles` function in error case at second call
    #[test]
    fn test_get_socket_handles_error_second_call() -> anyhow::Result<()> {
        set_mock_socket_handles(2, SUCCESS, ERROR);
        let res = get_socket_handles(load_amdsmi()?);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_processor_handles` function in success case
    #[test]
    fn test_get_processor_handles_success() -> anyhow::Result<()> {
        let handles = vec![10 as amdsmi_processor_handle, 20 as amdsmi_processor_handle];
        set_mock_processor_handles(handles.clone(), SUCCESS, SUCCESS);

        let res = get_processor_handles(load_amdsmi()?, 0 as amdsmi_socket_handle);
        assert!(res.is_ok());

        let out = res.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], handles[0]);
        assert_eq!(out[1], handles[1]);
        Ok(())
    }

    // Test `get_processor_handles` function with no handles
    #[test]
    fn test_get_processor_handles_empty() -> anyhow::Result<()> {
        set_mock_processor_handles(vec![], SUCCESS, SUCCESS);
        let res = get_processor_handles(load_amdsmi()?, 0 as amdsmi_socket_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), 0);
        Ok(())
    }

    // Test `get_processor_handles` function in error case at first call
    #[test]
    fn test_get_processor_handles_error_first_call() -> anyhow::Result<()> {
        set_mock_processor_handles(vec![10 as amdsmi_processor_handle], ERROR, SUCCESS);
        let res = get_processor_handles(load_amdsmi()?, 0 as amdsmi_socket_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_processor_handles` function in error case at second call
    #[test]
    fn test_get_processor_handles_error_second_call() -> anyhow::Result<()> {
        set_mock_processor_handles(vec![10 as amdsmi_processor_handle], SUCCESS, ERROR);
        let res = get_processor_handles(load_amdsmi()?, 0 as amdsmi_socket_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_uuid` function in successful case
    #[test]
    fn test_get_device_uuid_success() -> anyhow::Result<()> {
        let ustr = "a4ff740f-0000-1000-80ea-e05c945bb3b2";
        let mut uuid = CString::new(ustr)
            .unwrap()
            .into_bytes_with_nul()
            .iter()
            .map(|b| *b as c_char)
            .collect::<Vec<c_char>>();

        uuid.resize(AMDSMI_GPU_UUID_SIZE as usize, 0);
        set_mock_uuid(uuid, SUCCESS);

        let res = get_device_uuid(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), ustr);
        Ok(())
    }

    // Test `get_device_uuid` function with invalids UTF-8 bytes in buffer
    #[test]
    fn test_get_device_uuid_invalid() -> anyhow::Result<()> {
        let mut uuid = vec![0xFFu8 as i8 as c_char, 0xFEu8 as i8 as c_char, 0];
        uuid.resize(AMDSMI_GPU_UUID_SIZE as usize, 0);
        set_mock_uuid(uuid, SUCCESS);

        let res = get_device_uuid(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), UTF8_ERR);
        Ok(())
    }

    // Test `get_device_uuid` function in error case
    #[test]
    fn test_get_device_uuid_error() -> anyhow::Result<()> {
        let uuid = vec![0 as c_char; AMDSMI_GPU_UUID_SIZE as usize];
        set_mock_uuid(uuid, ERROR);

        let res = get_device_uuid(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_activity` function in success case
    #[test]
    fn test_get_device_activity_success() -> anyhow::Result<()> {
        let mut data: amdsmi_engine_usage_t = unsafe { zeroed() };
        data.gfx_activity = 34;
        data.mm_activity = 12;
        data.umc_activity = 56;

        set_mock_activity_usage(SUCCESS, data);

        let res = get_device_activity(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());

        let info = res.unwrap();
        assert_eq!(info.gfx_activity, 34);
        assert_eq!(info.mm_activity, 12);
        assert_eq!(info.umc_activity, 56);
        Ok(())
    }

    // Test `get_device_activity` function in error case
    #[test]
    fn test_get_device_activity_error() -> anyhow::Result<()> {
        set_mock_activity_usage(ERROR, unsafe { zeroed() });

        let res = get_device_activity(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_energy` function in success case
    #[test]
    fn test_get_device_energy_success() -> anyhow::Result<()> {
        set_mock_energy_consumption(123456789, 0.5, TIMESTAMP, SUCCESS);
        let res = get_device_energy(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());

        let (energy, resolution, timestamp) = res.unwrap();
        assert_eq!(energy, 123456789);
        assert_eq!(resolution, 0.5);
        assert_eq!(timestamp, TIMESTAMP);
        Ok(())
    }

    // Test `get_device_energy` function in error case
    #[test]
    fn test_get_device_energy_error() -> anyhow::Result<()> {
        set_mock_energy_consumption(0, 0.0, 0, ERROR);
        let res = get_device_energy(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_memory` function in success case
    #[test]
    fn test_get_device_memory_usage_success() -> anyhow::Result<()> {
        set_mock_memory_usage(13443072, SUCCESS);
        let res = get_device_memory_usage(load_amdsmi()?, 0 as amdsmi_processor_handle, 0 as amdsmi_memory_type_t);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 13443072);
        Ok(())
    }

    // Test `get_device_memory` function in error case
    #[test]
    fn test_get_device_memory_usage_error() -> anyhow::Result<()> {
        set_mock_memory_usage(0, ERROR);
        let res = get_device_memory_usage(load_amdsmi()?, 0 as amdsmi_processor_handle, 0 as amdsmi_memory_type_t);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_power_management` function if the power management is disabled
    #[test]
    fn test_get_device_power_management_disabled() -> anyhow::Result<()> {
        set_mock_power_management_status(false, SUCCESS);
        let res = get_device_power_managment(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), false);
        Ok(())
    }

    // Test `get_device_power_management` function in success case
    #[test]
    fn test_get_device_power_management_success() -> anyhow::Result<()> {
        set_mock_power_management_status(true, SUCCESS);
        let res = get_device_power_managment(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), true);
        Ok(())
    }

    // Test `get_device_power_management` function in error case
    #[test]
    fn test_get_device_power_management_error() -> anyhow::Result<()> {
        set_mock_power_management_status(false, ERROR);
        let res = get_device_power_managment(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_power` function in success case
    #[test]
    fn test_get_device_power_success() -> anyhow::Result<()> {
        let mut data: amdsmi_power_info_t = unsafe { zeroed() };
        data.current_socket_power = 43;
        data.average_socket_power = 40;
        set_mock_power_consumption(data, SUCCESS);

        let res = get_device_power(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().current_socket_power, 43);
        assert_eq!(res.unwrap().average_socket_power, 40);
        Ok(())
    }

    // Test `get_device_power` function in success case
    #[test]
    fn test_get_device_power_error() -> anyhow::Result<()> {
        let data: amdsmi_power_info_t = unsafe { zeroed() };
        set_mock_power_consumption(data, ERROR);

        let res = get_device_power(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_voltage` function in success case
    #[test]
    fn test_get_device_voltage_success() -> anyhow::Result<()> {
        set_mock_voltage_consumption(830, SUCCESS);
        let res = get_device_voltage(
            load_amdsmi()?,
            0 as amdsmi_processor_handle,
            amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_LAST,
            amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT,
        );
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 830);
        Ok(())
    }

    // Test `get_device_voltage` function in error case
    #[test]
    fn test_get_device_voltage_error() -> anyhow::Result<()> {
        set_mock_voltage_consumption(0, ERROR);
        let res = get_device_voltage(
            load_amdsmi()?,
            0 as amdsmi_processor_handle,
            amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_LAST,
            amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT,
        );
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_temperature` function in success case
    #[test]
    fn test_get_device_temperature_success() -> anyhow::Result<()> {
        set_mock_temperature(52, SUCCESS);
        let res = get_device_temperature(
            load_amdsmi()?,
            0 as amdsmi_processor_handle,
            amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE,
            amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT,
        );
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 52);
        Ok(())
    }

    // Test `get_device_temperature` function in error case
    #[test]
    fn test_get_device_temperature_error() -> anyhow::Result<()> {
        set_mock_temperature(0, ERROR);
        let res = get_device_temperature(
            load_amdsmi()?,
            0 as amdsmi_processor_handle,
            amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE,
            amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT,
        );
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }

    // Test `get_device_process_list` function in success case
    #[test]
    fn test_get_device_process_list_success() -> anyhow::Result<()> {
        let mut process_1: amdsmi_proc_info_t = unsafe { zeroed() };
        let mut process_2: amdsmi_proc_info_t = unsafe { zeroed() };

        process_1.pid = 1;
        process_2.pid = 2;

        set_mock_process_list(vec![process_1, process_2], SUCCESS);
        let res = get_device_process_list(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());

        let processes = res.unwrap();
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].pid, 1);
        assert_eq!(processes[1].pid, 2);
        Ok(())
    }

    // Test `get_device_process_list` function for no processes
    #[test]
    fn test_get_device_process_list_empty() -> anyhow::Result<()> {
        set_mock_process_list(vec![], SUCCESS);
        let res = get_device_process_list(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), 0);
        Ok(())
    }

    // Test `get_device_process_list` function in error case
    #[test]
    fn test_get_device_process_list_error() -> anyhow::Result<()> {
        set_mock_process_list(vec![], ERROR);
        let res = get_device_process_list(load_amdsmi()?, 0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
        Ok(())
    }
}
