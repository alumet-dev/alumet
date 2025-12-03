pub mod ffi_mock_common {
    pub use crate::bindings::{
        amdsmi_engine_usage_t, amdsmi_init_flags_t, amdsmi_memory_type_t, amdsmi_power_info_t, amdsmi_proc_info_t,
        amdsmi_processor_handle, amdsmi_socket_handle, amdsmi_status_t, amdsmi_status_t_AMDSMI_STATUS_INVAL,
        amdsmi_status_t_AMDSMI_STATUS_SUCCESS, amdsmi_temperature_metric_t, amdsmi_temperature_type_t,
        amdsmi_voltage_metric_t, amdsmi_voltage_type_t,
    };
    pub use std::{
        cell::{Cell, RefCell},
        os::raw::c_char,
    };

    pub const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    pub const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
}

// Mock of AMD functions for AMD-SMI library initialisation and shutdown
#[cfg(test)]
pub mod ffi_mocks_init {
    use super::ffi_mock_common::*;
    use crate::bindings::libamd_smi;
    use std::sync::OnceLock;

    thread_local! {
        pub static MOCK_STATUS: Cell<amdsmi_status_t> =
            Cell::new(amdsmi_status_t_AMDSMI_STATUS_SUCCESS);
    }

    pub fn set_mock_status(status: amdsmi_status_t) {
        MOCK_STATUS.with(|c| c.set(status));
    }

    unsafe extern "C" fn mock_amdsmi_init(_flags: u64) -> amdsmi_status_t {
        MOCK_STATUS.with(|c| c.get())
    }

    unsafe extern "C" fn mock_amdsmi_shutdown() -> amdsmi_status_t {
        MOCK_STATUS.with(|c| c.get())
    }

    pub fn mocked_lib() -> libamd_smi {
        let mut lib = unsafe { libamd_smi::new("libamd_smi.so").unwrap() };
        lib.amdsmi_init = Ok(mock_amdsmi_init);
        lib.amdsmi_shut_down = Ok(mock_amdsmi_shutdown);
        lib
    }

    static MOCK_INSTANCE: OnceLock<libamd_smi> = OnceLock::new();

    pub fn get_mock() -> &'static libamd_smi {
        MOCK_INSTANCE.get_or_init(mocked_lib)
    }
}

// Mock of AMD function for socket handles to identify GPUs address
#[cfg(test)]
pub mod ffi_mocks_socket_handles {
    use super::ffi_mock_common::*;

    #[derive(Clone)]
    struct MockSocketCall {
        count: u32,
        status_1: amdsmi_status_t,
        status_2: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockSocketCall>> = RefCell::new(None);
    }

    pub fn set_mock_socket_handles(count: u32, status_1: amdsmi_status_t, status_2: amdsmi_status_t) {
        MOCK.with(|m| {
            m.replace(Some(MockSocketCall {
                count,
                status_1,
                status_2,
            }))
        });
    }

    // Mock of `amdsmi_get_socket_handles` FFI function from AMD-SMI
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
                if handles.is_null() {
                    *count = mock.count;
                    return mock.status_1;
                }
                *count = mock.count;
                for i in 0..mock.count {
                    *handles.add(i as usize) = i as amdsmi_socket_handle;
                }
                mock.status_2
            }
        })
    }
}

// Mock of AMD function for processor handles from socket handles to identify GPUs installed
#[cfg(test)]
pub mod ffi_mocks_processor_handles {
    use super::ffi_mock_common::*;

    #[derive(Clone)]
    struct MockProcessorCall {
        handles: Vec<amdsmi_processor_handle>,
        status_1: amdsmi_status_t,
        status_2: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockProcessorCall>> = RefCell::new(None);
    }

    pub fn set_mock_processor_handles(
        handles: Vec<amdsmi_processor_handle>,
        status_1: amdsmi_status_t,
        status_2: amdsmi_status_t,
    ) {
        MOCK.with(|m| {
            m.replace(Some(MockProcessorCall {
                handles,
                status_1,
                status_2,
            }))
        });
    }

    // Mock of `amdsmi_get_processor_handles` FFI function from AMD-SMI
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
                if list.is_null() {
                    *count = mock.handles.len() as u32;
                    return mock.status_1;
                }
                *count = mock.handles.len() as u32;
                for (i, h) in mock.handles.iter().enumerate() {
                    *list.add(i) = *h;
                }
                mock.status_2
            }
        })
    }
}

// Mock of AMD function to get the UUID of an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_uuid {
    use super::ffi_mock_common::*;

    #[derive(Clone)]
    struct MockUUID {
        uuid: Vec<c_char>,
        status: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockUUID>> = RefCell::new(None);
    }

    pub fn set_mock_uuid(uuid: Vec<c_char>, status: amdsmi_status_t) {
        MOCK.with(|m| m.replace(Some(MockUUID { uuid, status })));
    }

    // Mock of `amdsmi_get_gpu_device_uuid` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the engine activity usage of an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_activity_usage {
    use super::ffi_mock_common::*;

    #[derive(Clone)]
    struct MockActivity {
        status: amdsmi_status_t,
        activity: amdsmi_engine_usage_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockActivity>> = RefCell::new(None);
    }

    pub fn set_mock_activity_usage(status: amdsmi_status_t, activity: amdsmi_engine_usage_t) {
        MOCK.with(|m| m.replace(Some(MockActivity { status, activity })));
    }

    // Mock of `amdsmi_get_gpu_activity` FFI function from AMD-SMI
    #[unsafe(no_mangle)]
    pub extern "C" fn amdsmi_get_gpu_activity(
        _processor: amdsmi_processor_handle,
        info: *mut amdsmi_engine_usage_t,
    ) -> amdsmi_status_t {
        MOCK.with(|m| {
            let mock = m.borrow();
            let Some(mock) = &*mock else {
                return ERROR;
            };
            if mock.status == SUCCESS {
                unsafe {
                    if !info.is_null() {
                        *info = mock.activity;
                    }
                }
            }
            mock.status
        })
    }
}

// Mock of AMD function to get the energy consumed by an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_energy_consumption {
    use super::ffi_mock_common::*;

    thread_local! {
        static MOCK: Cell<Option<(u64, f32, u64, amdsmi_status_t)>> = Cell::new(None);
    }

    pub fn set_mock_energy_consumption(energy: u64, resolution: f32, timestamp: u64, val: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((energy, resolution, timestamp, val))));
    }

    // Mock of `amdsmi_get_energy_count` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the power management status for an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_power_management_status {
    use super::ffi_mock_common::*;

    thread_local! {
        static MOCK: Cell<Option<(bool, amdsmi_status_t)>> = Cell::new(None);
    }

    pub fn set_mock_power_management_status(enabled: bool, val: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((enabled, val))));
    }

    // Mock of `amdsmi_is_gpu_power_management_enabled` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the power consumed by an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_power_consumption {
    use super::ffi_mock_common::*;

    // Mock of AMD FFI function for power
    thread_local! {
        static MOCK: Cell<Option<(amdsmi_power_info_t, amdsmi_status_t)>> = Cell::new(None);
    }

    pub fn set_mock_power_consumption(info: amdsmi_power_info_t, result: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((info, result))));
    }

    // Mock of `amdsmi_get_power_info` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the memory usage for a AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_memory_usage {
    use super::ffi_mock_common::*;

    thread_local! {
        static MOCK: Cell<Option<(u64, amdsmi_status_t)>> = Cell::new(None);
    }

    pub fn set_mock_memory_usage(used: u64, val: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((used, val))));
    }

    // Mock of `amdsmi_get_gpu_memory_usage` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the voltage consumed by an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_voltage_consumption {
    use super::ffi_mock_common::*;

    thread_local! {
        static MOCK: Cell<Option<(i64, amdsmi_status_t)>> = Cell::new(None);
    }

    pub fn set_mock_voltage_consumption(voltage: i64, result: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((voltage, result))));
    }

    // Mock of `amdsmi_get_gpu_volt_metric` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the temperatures of an AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_temperature {
    use super::ffi_mock_common::*;

    thread_local! {
        static MOCK: Cell<Option<(i64, amdsmi_status_t)>> = Cell::new(None);
    }

    pub fn set_mock_temperature(temperature: i64, status: amdsmi_status_t) {
        MOCK.with(|c| c.set(Some((temperature, status))));
    }

    // Mock of `amdsmi_get_temp_metric` FFI function from AMD-SMI
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
}

// Mock of AMD function to get the list of process running on a AMD GPU device
#[cfg(test)]
pub mod ffi_mocks_process_list {
    use super::ffi_mock_common::*;

    #[derive(Clone)]
    struct MockProcessCall {
        processes: Vec<amdsmi_proc_info_t>,
        status: amdsmi_status_t,
    }

    thread_local! {
        static MOCK: RefCell<Option<MockProcessCall>> = RefCell::new(None);
    }

    pub fn set_mock_process_list(processes: Vec<amdsmi_proc_info_t>, status: amdsmi_status_t) {
        MOCK.with(|m| m.replace(Some(MockProcessCall { processes, status })));
    }

    // Mock of `amdsmi_get_gpu_process_list` FFI function from AMD-SMI
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
}
