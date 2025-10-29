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

#[cfg(test)]
mod tests_init {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
    const INIT_FLAG: amdsmi_init_flags_t = amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS;

    thread_local! {
        static MOCK: Cell<amdsmi_status_t> = Cell::new(SUCCESS);
    }

    fn set_mock(val: amdsmi_status_t) {
        MOCK.with(|v| v.set(val));
    }

    // Mock of FFI `amdsmi_init` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_init(_flag: amdsmi_init_flags_t) -> amdsmi_status_t {
        MOCK.with(|v| v.get())
    }

    // Test `amd_sys_init` function in success case
    #[test]
    fn test_amd_sys_init_success() {
        set_mock(SUCCESS);
        let res = amd_sys_init(INIT_FLAG);
        assert!(res.is_ok());
    }

    // Test `amd_sys_init` function in error case
    #[test]
    fn test_amd_sys_init_error() {
        set_mock(ERROR);
        let res = amd_sys_init(INIT_FLAG);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

#[cfg(test)]
mod tests_shutdown {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    thread_local! {
        static MOCK: Cell<amdsmi_status_t> = Cell::new(SUCCESS);
    }

    fn set_mock(val: amdsmi_status_t) {
        MOCK.with(|v| v.set(val));
    }

    // Mock of FFI `amdsmi_shut_down` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_shut_down() -> amdsmi_status_t {
        MOCK.with(|v| v.get())
    }

    // Test `amd_sys_shutdown` function in success case
    #[test]
    fn test_amd_sys_shutdown_success() {
        set_mock(SUCCESS);
        let res = amd_sys_shutdown();
        assert!(res.is_ok());
    }

    // Test `amd_sys_shutdown` function in error case
    #[test]
    fn test_amd_sys_shutdown_error() {
        set_mock(ERROR);
        let res = amd_sys_shutdown();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to get socket handles to identifying AMD hardware installed
#[cfg(test)]
mod tests_socket_handles {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::c_void;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    #[derive(Clone)]
    struct MockCall {
        count: u32,
        status_1: amdsmi_status_t,
        status_2: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockCall>> = RefCell::new(None);
    }

    fn set_mock(count: u32, status_1: amdsmi_status_t, status_2: amdsmi_status_t) {
        MOCK.with(|m| {
            m.replace(Some(MockCall {
                count,
                status_1,
                status_2,
            }))
        });
    }

    // Mock of FFI `amdsmi_get_socket_handles` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_socket_handles(
        count: *mut u32,
        handles: *mut amdsmi_socket_handle,
    ) -> amdsmi_status_t {
        MOCK.with(|m| {
            let mock = m.borrow();
            let Some(mock) = &*mock else {
                return ERROR;
            };

            unsafe {
                // First call
                if handles.is_null() {
                    *count = mock.count;
                    return mock.status_1;
                }
                // Second call
                *count = mock.count;
                for i in 0..mock.count {
                    *handles.add(i as usize) = i as amdsmi_socket_handle;
                }
                return mock.status_2;
            }
        })
    }

    // Test `get_socket_handles` function in success case
    #[test]
    fn test_get_socket_handles_success() {
        set_mock(3, SUCCESS, SUCCESS);
        let res = get_socket_handles();
        assert!(res.is_ok());

        let sockets = res.unwrap();
        let expected = vec![0 as *mut c_void, 1 as *mut c_void, 2 as *mut c_void];
        assert_eq!(sockets, expected);
    }

    // Test `get_socket_handles` function with no handles
    #[test]
    fn test_get_socket_handles_empty() {
        set_mock(0, SUCCESS, SUCCESS);
        let res = get_socket_handles();
        assert!(res.is_ok());
        assert!(res.unwrap().is_empty());
    }

    // Test `get_socket_handles` function in error case at first call
    #[test]
    fn test_get_socket_handles_error_first_call() {
        set_mock(0, ERROR, SUCCESS);
        let res = get_socket_handles();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }

    // Test `get_socket_handles` function in error case at second call
    #[test]
    fn test_get_socket_handles_error_second_call() {
        set_mock(2, SUCCESS, ERROR);
        let res = get_socket_handles();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to get processor handles to identifying AMD hardware installed
#[cfg(test)]
mod tests_processor_handles {
    use super::*;
    use std::cell::RefCell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    #[derive(Clone)]
    struct MockCall {
        handles: Vec<amdsmi_processor_handle>,
        status_1: amdsmi_status_t,
        status_2: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockCall>> = RefCell::new(None);
    }

    fn set_mock(handles: Vec<amdsmi_processor_handle>, status_1: amdsmi_status_t, status_2: amdsmi_status_t) {
        MOCK.with(|m| {
            m.replace(Some(MockCall {
                handles,
                status_1,
                status_2,
            }))
        });
    }

    // Mock of FFI `amdsmi_get_processor_handles` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_processor_handles(
        _socket: amdsmi_socket_handle,
        count: *mut u32,
        list: *mut amdsmi_processor_handle,
    ) -> amdsmi_status_t {
        MOCK.with(|m| {
            let mock = m.borrow();
            let Some(mock) = &*mock else {
                return ERROR;
            };

            unsafe {
                // First call
                if list.is_null() {
                    *count = mock.handles.len() as u32;
                    return mock.status_1;
                }
                // Second call
                *count = mock.handles.len() as u32;
                for (i, h) in mock.handles.iter().enumerate() {
                    *list.add(i) = *h;
                }

                return mock.status_2;
            }
        })
    }

    // Test `get_processor_handles` function in success case
    #[test]
    fn test_get_processor_handles_success() {
        let handles = vec![10 as amdsmi_processor_handle, 20 as amdsmi_processor_handle];
        set_mock(handles.clone(), SUCCESS, SUCCESS);

        let res = get_processor_handles(0 as amdsmi_socket_handle);
        assert!(res.is_ok());

        let out = res.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], handles[0]);
        assert_eq!(out[1], handles[1]);
    }

    // Test `get_processor_handles` function with no handles
    #[test]
    fn test_get_processor_handles_empty() {
        set_mock(vec![], SUCCESS, SUCCESS);
        let res = get_processor_handles(0 as amdsmi_socket_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), 0);
    }

    // Test `get_processor_handles` function in error case at first call
    #[test]
    fn test_get_processor_handles_error_first_call() {
        set_mock(vec![10 as amdsmi_processor_handle], ERROR, SUCCESS);
        let res = get_processor_handles(0 as amdsmi_socket_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }

    // Test `get_processor_handles` function in error case at second call
    #[test]
    fn test_get_processor_handles_error_second_call() {
        set_mock(vec![10 as amdsmi_processor_handle], SUCCESS, ERROR);
        let res = get_processor_handles(0 as amdsmi_socket_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve UUID AMD device value
#[cfg(test)]
mod tests_uuid {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::CString;
    use std::os::raw::c_char;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
    const UTF8_ERR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_DRM_ERROR;

    #[derive(Clone)]
    struct MockUUID {
        uuid: Vec<c_char>,
        status: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockUUID>> = RefCell::new(None);
    }

    fn set_mock(uuid: Vec<c_char>, status: amdsmi_status_t) {
        MOCK.with(|m| m.replace(Some(MockUUID { uuid, status })));
    }

    // Mock of FFI `amdsmi_get_gpu_device_uuid` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_gpu_device_uuid(
        _handle: amdsmi_processor_handle,
        length: *mut u32,
        buffer: *mut c_char,
    ) -> amdsmi_status_t {
        MOCK.with(|m| {
            let mock = m.borrow();
            let Some(mock) = &*mock else {
                return ERROR;
            };

            unsafe {
                if !length.is_null() {
                    *length = mock.uuid.len() as u32;
                }

                if !buffer.is_null() {
                    for (i, ch) in mock.uuid.iter().enumerate() {
                        *buffer.add(i) = *ch;
                    }
                }
            }

            mock.status
        })
    }

    // Test `get_device_uuid` function in successful case
    #[test]
    fn test_get_device_uuid_success() {
        let uuid_str = "a4ff740f-0000-1000-80ea-e05c945bb3b2";
        let mut uuid = CString::new(uuid_str)
            .unwrap()
            .into_bytes_with_nul()
            .iter()
            .map(|b| *b as c_char)
            .collect::<Vec<c_char>>();

        uuid.resize(AMDSMI_GPU_UUID_SIZE as usize, 0);
        set_mock(uuid, SUCCESS);

        let res = get_device_uuid(0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), uuid_str);
    }

    // Test `get_device_uuid` function with invalids UTF-8 bytes in buffer
    #[test]
    fn test_get_device_uuid_invalid() {
        let mut uuid = vec![0xFFu8 as i8 as c_char, 0xFEu8 as i8 as c_char, 0];
        uuid.resize(AMDSMI_GPU_UUID_SIZE as usize, 0);
        set_mock(uuid, SUCCESS);

        let res = get_device_uuid(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), UTF8_ERR);
    }

    // Test `get_device_uuid` function in error case
    #[test]
    fn test_get_device_uuid_error() {
        let uuid = vec![0 as c_char; AMDSMI_GPU_UUID_SIZE as usize];
        set_mock(uuid, ERROR);

        let res = get_device_uuid(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve activity engine usage value
#[cfg(test)]
mod tests_activity {
    use super::*;
    use std::{cell::Cell, mem::zeroed};

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    thread_local! {
        static MOCK_STATUS: Cell<amdsmi_status_t> = Cell::new(SUCCESS);
        static MOCK_ACTIVITY: Cell<amdsmi_engine_usage_t> = Cell::new(unsafe { zeroed() });
    }

    fn set_mock(status: amdsmi_status_t, activity: amdsmi_engine_usage_t) {
        MOCK_STATUS.with(|s| s.set(status));
        MOCK_ACTIVITY.with(|a| a.set(activity));
    }

    //  Mock of FFI `amdsmi_get_gpu_activity` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_gpu_activity(
        _processor: amdsmi_processor_handle,
        info: *mut amdsmi_engine_usage_t,
    ) -> amdsmi_status_t {
        let status = MOCK_STATUS.with(|s| s.get());
        if status == SUCCESS {
            let value = MOCK_ACTIVITY.with(|a| a.get());
            unsafe { *info = value };
        }
        status
    }

    // Test `get_device_activity` function in success case
    #[test]
    fn test_get_device_activity_success() {
        let data = amdsmi_engine_usage_t {
            gfx_activity: 34,
            mm_activity: 12,
            umc_activity: 56,
            reserved: [0; 13],
        };

        set_mock(SUCCESS, data);
        let res = get_device_activity(0 as amdsmi_processor_handle);
        assert!(res.is_ok());

        let info = res.unwrap();
        assert_eq!(info.gfx_activity, 34);
        assert_eq!(info.mm_activity, 12);
        assert_eq!(info.umc_activity, 56);
    }

    // Test `get_device_activity` function in error case
    #[test]
    fn test_get_device_activity_error() {
        set_mock(ERROR, unsafe { zeroed() });
        let res = get_device_activity(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve energy consumption value
#[cfg(test)]
mod tests_energy {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
    const TIMESTAMP: u64 = 1708236479191334820;

    thread_local! {
        static MOCK: Cell<Option<(u64, f32, u64, amdsmi_status_t)>> = Cell::new(None);
    }

    fn set_mock(energy: u64, resolution: f32, timestamp: u64, val: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((energy, resolution, timestamp, val))));
    }

    // Mock of FFI `amdsmi_get_energy_count` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_energy_count(
        _handle: amdsmi_processor_handle,
        energy: *mut u64,
        resolution: *mut f32,
        timestamp: *mut u64,
    ) -> amdsmi_status_t {
        MOCK.with(|c| {
            if let Some((e, r, t, res)) = c.get() {
                unsafe {
                    if !energy.is_null() {
                        *energy = e;
                    }
                    if !resolution.is_null() {
                        *resolution = r;
                    }
                    if !timestamp.is_null() {
                        *timestamp = t;
                    }
                }
                res
            } else {
                ERROR
            }
        })
    }

    // Test `get_device_energy` function in success case
    #[test]
    fn test_get_device_energy_success() {
        set_mock(12345, 0.5, TIMESTAMP, SUCCESS);
        let res = get_device_energy(0 as amdsmi_processor_handle);
        assert!(res.is_ok());

        let (energy, resolution, timestamp) = res.unwrap();
        assert_eq!(energy, 12345);
        assert_eq!(resolution, 0.5);
        assert_eq!(timestamp, TIMESTAMP);
    }

    // Test `get_device_energy` function in error case
    #[test]
    fn test_get_device_energy_error() {
        set_mock(0, 0.0, 0, ERROR);
        let res = get_device_energy(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve memory usage value
#[cfg(test)]
mod tests_memory {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    thread_local! {
        static MOCK: Cell<Option<(u64, amdsmi_status_t)>> = Cell::new(None);
    }

    fn set_mock(used: u64, val: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((used, val))));
    }

    // Mock of FFI `amdsmi_get_gpu_memory_usage` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_gpu_memory_usage(
        _handle: amdsmi_processor_handle,
        _mem_type: amdsmi_memory_type_t,
        used: *mut u64,
    ) -> amdsmi_status_t {
        MOCK.with(|c| {
            if let Some((u, res)) = c.get() {
                unsafe {
                    if !used.is_null() {
                        *used = u;
                    }
                }
                res
            } else {
                ERROR
            }
        })
    }

    // Test `get_device_memory` function in success case
    #[test]
    fn test_get_device_memory_usage_success() {
        set_mock(13443072, SUCCESS);
        let res = get_device_memory_usage(0 as amdsmi_processor_handle, 0 as amdsmi_memory_type_t);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 13443072);
    }

    // Test `get_device_memory` function in error case
    #[test]
    fn test_get_device_memory_usage_error() {
        set_mock(0, ERROR);
        let res = get_device_memory_usage(0 as amdsmi_processor_handle, 0 as amdsmi_memory_type_t);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve power consumption value
#[cfg(test)]
mod tests_power {
    use super::*;
    use std::{cell::Cell, mem::zeroed};

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    thread_local! {
        static MOCK: Cell<Option<(amdsmi_power_info_t, amdsmi_status_t)>> = Cell::new(None);
    }

    fn set_mock(info: amdsmi_power_info_t, result: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((info, result))));
    }

    // Mock of FFI `amdsmi_get_power_info` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_power_info(
        _handle: amdsmi_processor_handle,
        info: *mut amdsmi_power_info_t,
    ) -> amdsmi_status_t {
        MOCK.with(|c| {
            if let Some((data, res)) = c.get() {
                unsafe {
                    if !info.is_null() {
                        *info = data;
                    }
                }
                res
            } else {
                ERROR
            }
        })
    }

    // Test `get_device_power` function in success case
    #[test]
    fn test_get_device_power_success() {
        let mut data: amdsmi_power_info_t = unsafe { zeroed() };
        data.current_socket_power = 43;
        data.average_socket_power = 40;

        set_mock(data, SUCCESS);
        let res = get_device_power(0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().current_socket_power, 43);
        assert_eq!(res.unwrap().average_socket_power, 40);
    }

    // Test `get_device_power` function in success case
    #[test]
    fn test_get_device_power_error() {
        let data: amdsmi_power_info_t = unsafe { zeroed() };
        set_mock(data, ERROR);

        let res = get_device_power(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve power management status value
#[cfg(test)]
mod tests_power_management {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    thread_local! {
        static MOCK: Cell<Option<(bool, amdsmi_status_t)>> = Cell::new(None);
    }

    fn set_mock(enabled: bool, val: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((enabled, val))));
    }

    // Mock of FFI `amdsmi_is_gpu_power_management_enabled` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_is_gpu_power_management_enabled(
        _handle: amdsmi_processor_handle,
        enabled: *mut bool,
    ) -> amdsmi_status_t {
        MOCK.with(|c| {
            if let Some((value, res)) = c.get() {
                unsafe {
                    if !enabled.is_null() {
                        *enabled = value;
                    }
                }
                res
            } else {
                ERROR
            }
        })
    }

    // Test `get_device_power_management` function
    #[test]
    fn test_get_device_power_management_success() {
        set_mock(true, SUCCESS);
        let res = get_device_power_managment(0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), true);
    }

    // Test `get_device_power_management` function if the power management is disabled
    #[test]
    fn test_get_device_power_management_disabled() {
        set_mock(false, SUCCESS);
        let res = get_device_power_managment(0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), false);
    }

    // Test `get_device_power_management` function
    #[test]
    fn test_get_device_power_management_error() {
        set_mock(false, ERROR);
        let res = get_device_power_managment(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve voltage value
#[cfg(test)]
mod tests_voltage {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    thread_local! {
        static MOCK: Cell<Option<(i64, amdsmi_status_t)>> = Cell::new(None);
    }

    fn set_mock(voltage: i64, result: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((voltage, result))));
    }

    // Mock of FFI `amdsmi_get_gpu_volt_metric` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_gpu_volt_metric(
        _handle: amdsmi_processor_handle,
        _sensor_type: amdsmi_voltage_type_t,
        _metric: amdsmi_voltage_metric_t,
        voltage: *mut i64,
    ) -> amdsmi_status_t {
        MOCK.with(|c| {
            if let Some((v, res)) = c.get() {
                unsafe {
                    if !voltage.is_null() {
                        *voltage = v;
                    }
                }
                res
            } else {
                ERROR
            }
        })
    }

    // Test `get_device_voltage` function in success case
    #[test]
    fn test_get_device_voltage_success() {
        set_mock(1200, SUCCESS);
        let res = get_device_voltage(
            0 as amdsmi_processor_handle,
            amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_LAST,
            amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT,
        );
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 1200);
    }

    // Test `get_device_voltage` function in error case
    #[test]
    fn test_get_device_voltage_error() {
        set_mock(0, ERROR);
        let res = get_device_voltage(
            0 as amdsmi_processor_handle,
            amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_LAST,
            amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT,
        );
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve temperature value
#[cfg(test)]
mod tests_temperature {
    use super::*;
    use std::cell::Cell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
    const METRIC: amdsmi_status_t = amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT;
    const SENSOR: amdsmi_status_t = amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE;

    thread_local! {
        static MOCK: Cell<Option<(i64, amdsmi_status_t)>> = Cell::new(None);
    }

    fn set_mock(temperature: i64, status: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((temperature, status))));
    }

    // Mock of FFI `amdsmi_get_temp_metric` C function
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_temp_metric(
        _handle: amdsmi_processor_handle,
        _sensor_type: amdsmi_temperature_type_t,
        _metric: amdsmi_temperature_metric_t,
        temperature: *mut i64,
    ) -> amdsmi_status_t {
        MOCK.with(|c| {
            if let Some((t, res)) = c.get() {
                unsafe {
                    if !temperature.is_null() {
                        *temperature = t;
                    }
                }
                res
            } else {
                ERROR
            }
        })
    }

    // Test `get_device_temperature` function in success case
    #[test]
    fn test_get_device_temperature_success() {
        set_mock(52, SUCCESS);
        let res = get_device_temperature(0 as amdsmi_processor_handle, SENSOR, METRIC);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 52);
    }

    // Test `get_device_temperature` function in error case
    #[test]
    fn test_get_device_temperature_error() {
        set_mock(0, ERROR);
        let res = get_device_temperature(0 as amdsmi_processor_handle, SENSOR, METRIC);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}

// Tests to retrieve processes information values
#[cfg(test)]
mod tests_processes {
    use super::*;
    use std::cell::RefCell;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    #[derive(Clone)]
    struct MockProcessCall {
        processes: Vec<amdsmi_proc_info_t>,
        status: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockProcessCall>> = RefCell::new(None);
    }

    fn set_mock(processes: Vec<amdsmi_proc_info_t>, status: amdsmi_status_t) {
        MOCK.with(|m| m.replace(Some(MockProcessCall { processes, status })));
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_gpu_process_list(
        _handle: amdsmi_processor_handle,
        max_processes: *mut u32,
        list: *mut amdsmi_proc_info_t,
    ) -> amdsmi_status_t {
        MOCK.with(|m| {
            let mock = m.borrow();
            let Some(mock) = &*mock else {
                return ERROR;
            };

            unsafe {
                if !max_processes.is_null() {
                    *max_processes = mock.processes.len() as u32;
                }

                if !list.is_null() {
                    for (i, p) in mock.processes.iter().enumerate() {
                        *list.add(i) = *p;
                    }
                }
            }

            mock.status
        })
    }

    // Test `get_device_process_list` function
    #[test]
    fn test_get_device_process_list_success() {
        let mut process_1: amdsmi_proc_info_t = unsafe { zeroed() };
        let mut process_2: amdsmi_proc_info_t = unsafe { zeroed() };

        process_1.pid = 128;
        process_2.pid = 256;

        set_mock(vec![process_1, process_2], SUCCESS);
        let res = get_device_process_list(0 as amdsmi_processor_handle);
        assert!(res.is_ok());

        let processes = res.unwrap();
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].pid, 128);
        assert_eq!(processes[1].pid, 256);
    }

    // Test `get_device_process_list` function
    #[test]
    fn test_get_device_process_list_empty() {
        set_mock(vec![], SUCCESS);

        let res = get_device_process_list(0 as amdsmi_processor_handle);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), 0);
    }

    // Test `get_device_process_list` function
    #[test]
    fn test_get_device_process_list_error() {
        set_mock(vec![], ERROR);

        let res = get_device_process_list(0 as amdsmi_processor_handle);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), ERROR);
    }
}
