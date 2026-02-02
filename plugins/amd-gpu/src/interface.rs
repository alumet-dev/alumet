use mockall::automock;
use std::{
    ffi::CStr,
    mem::{MaybeUninit, transmute, zeroed},
    os::raw::c_char,
    ptr::null_mut,
    sync::Arc,
};
use thiserror::Error;

use crate::{
    amd::utils::*,
    bindings::{
        amdsmi_engine_usage_t, amdsmi_memory_type_t, amdsmi_power_info_t, amdsmi_proc_info_t, amdsmi_processor_handle,
        amdsmi_socket_handle, amdsmi_status_t, amdsmi_temperature_metric_t, amdsmi_temperature_type_t,
        amdsmi_voltage_metric_t, amdsmi_voltage_type_t, libamd_smi,
    },
};

/// Error treatment concerning AMD SMI library.
///
/// # Arguments
///
/// Take a status of [`amdsmi_status_t`] provided by AMD SMI library to catch dynamically the occurred error.
#[derive(Debug, Error)]
#[error("amd-smi library error: {0}")]
pub struct AmdError(pub amdsmi_status_t);

#[derive(Debug, Error)]
pub enum AmdInitError {
    #[error("amd-smi init error")]
    Init(#[from] AmdError),
    #[error("Failed to load {LIB_PATH}")]
    Load(#[from] libloading::Error),
}

#[derive(Clone)]
pub struct AmdSmi {
    amdsmi: Arc<libamd_smi>,
}

pub struct AmdSocketHandle {
    amdsmi: AmdSmi,
    inner: amdsmi_socket_handle,
}

pub struct AmdProcessorHandle {
    amdsmi: AmdSmi,
    inner: amdsmi_processor_handle,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AmdEnergyConsumptionInfo {
    /// The energy consumption value of an AMD GPU device since the last boot in micro Joules.
    pub energy: u64,
    /// Precision factor of the energy counter in micro Joules.
    pub resolution: f32,
    /// The time during which the energy value is recovered in ns.
    pub timestamp: u64,
}

pub type MockableAmdSmi = Arc<dyn AmdSmiTrait>;
pub type MockableAmdSocketHandle = Box<dyn SocketHandleTrait>;
pub type MockableAmdProcessorHandle = Box<dyn ProcessorHandleTrait>;

#[automock]
pub trait AmdSmiTrait: Send + Sync {
    fn stop(&self) -> Result<(), AmdError>;
    fn get_socket_handles(&self) -> Result<Vec<MockableAmdSocketHandle>, AmdError>;
}

#[automock]
pub trait SocketHandleTrait {
    fn get_processor_handles(&self) -> Result<Vec<MockableAmdProcessorHandle>, AmdError>;
}

#[automock]
pub trait ProcessorHandleTrait {
    fn get_device_uuid(&self) -> Result<String, AmdError>;
    fn get_device_activity(&self) -> Result<amdsmi_engine_usage_t, AmdError>;
    fn get_device_energy_consumption(&self) -> Result<AmdEnergyConsumptionInfo, AmdError>;
    fn get_device_memory_usage(&self, mem_type: amdsmi_memory_type_t) -> Result<u64, AmdError>;
    fn get_device_power_consumption(&self) -> Result<amdsmi_power_info_t, AmdError>;
    fn get_device_power_managment(&self) -> Result<bool, AmdError>;
    fn get_device_process_list(&self) -> Result<Vec<amdsmi_proc_info_t>, AmdError>;
    fn get_device_temperature(
        &self,
        sensor_type: amdsmi_temperature_type_t,
        metric: amdsmi_temperature_metric_t,
    ) -> Result<i64, AmdError>;
    fn get_device_voltage(
        &self,
        sensor_type: amdsmi_voltage_type_t,
        metric: amdsmi_voltage_metric_t,
    ) -> Result<i64, AmdError>;
}

#[inline]
fn get_value(status: amdsmi_status_t) -> Result<(), AmdError> {
    if status == SUCCESS {
        Ok(())
    } else {
        Err(AmdError(status))
    }
}

impl AmdSmi {
    /// Initialize and start amd-smi library with [`INIT_FLAG`].
    pub fn init() -> Result<Self, AmdInitError> {
        let amdsmi = unsafe { libamd_smi::new(LIB_PATH)? };
        let result = unsafe { amdsmi.amdsmi_init(INIT_FLAG.into()) };
        if result != SUCCESS {
            return Err(AmdInitError::Init(AmdError(result)));
        }
        Ok(AmdSmi {
            amdsmi: Arc::new(amdsmi),
        })
    }
}

impl AmdSmiTrait for AmdSmi {
    /// Quit amd-smi library and clean properly its resources.
    fn stop(&self) -> Result<(), AmdError> {
        let result = unsafe { self.amdsmi.amdsmi_shut_down() };
        get_value(result)
    }

    /// Retrieves a set of [`SocketHandle`] structure containing socket handles associated to a GPU device.
    fn get_socket_handles(&self) -> Result<Vec<MockableAmdSocketHandle>, AmdError> {
        let mut socket_count = 0;
        let result = unsafe { self.amdsmi.amdsmi_get_socket_handles(&mut socket_count, null_mut()) };
        if result != SUCCESS {
            return Err(AmdError(result));
        }

        let mut socket_handles = vec![unsafe { zeroed() }; socket_count as usize];

        let result = unsafe {
            self.amdsmi
                .amdsmi_get_socket_handles(&mut socket_count, socket_handles.as_mut_ptr())
        };
        if result == SUCCESS {
            socket_handles.truncate(socket_count as usize);
            Ok(socket_handles
                .into_iter()
                .map(|s| {
                    Box::new(AmdSocketHandle {
                        amdsmi: self.clone(),
                        inner: s,
                    }) as MockableAmdSocketHandle
                })
                .collect())
        } else {
            Err(AmdError(result))
        }
    }
}

impl SocketHandleTrait for AmdSocketHandle {
    /// Retrieves a set of [`ProcessorHandle`] structure containing processor handles associated to a GPU device.
    fn get_processor_handles(&self) -> Result<Vec<MockableAmdProcessorHandle>, AmdError> {
        let mut processor_count = 0;

        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_processor_handles(self.inner, &mut processor_count, null_mut())
        };
        if result != SUCCESS {
            return Err(AmdError(result));
        }

        let mut processor_handles = vec![unsafe { zeroed() }; processor_count as usize];

        let result = unsafe {
            self.amdsmi.amdsmi.amdsmi_get_processor_handles(
                self.inner,
                &mut processor_count,
                processor_handles.as_mut_ptr(),
            )
        };
        if result == SUCCESS {
            processor_handles.truncate(processor_count as usize);
            Ok(processor_handles
                .into_iter()
                .map(|s| {
                    Box::new(AmdProcessorHandle {
                        amdsmi: self.amdsmi.clone(),
                        inner: s,
                    }) as MockableAmdProcessorHandle
                })
                .collect())
        } else {
            Err(AmdError(result))
        }
    }
}

impl ProcessorHandleTrait for AmdProcessorHandle {
    /// Retrieves the UUID of the GPU device.
    fn get_device_uuid(&self) -> Result<String, AmdError> {
        let mut uuid_buffer = vec![0 as c_char; UUID_LENGTH as usize];
        let mut uuid_length = UUID_LENGTH;

        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_device_uuid(self.inner, &mut uuid_length, uuid_buffer.as_mut_ptr())
        };

        get_value(result)?;

        // Create a CStr based on the FFI buffer in checking the presence of an escaping character '\0'
        let c_str = if uuid_buffer[(uuid_length - 1) as usize] == 0 {
            unsafe { CStr::from_ptr(uuid_buffer.as_ptr()) }
        } else {
            // If the buffer doesn't had '\0', we create a secure stack buffer with it after a truncate.
            let mut cstr_buffer = [0 as c_char; UUID_LENGTH as usize + 1];
            cstr_buffer[..uuid_length as usize].copy_from_slice(&uuid_buffer[..uuid_length as usize]);
            cstr_buffer[uuid_length as usize] = 0;
            unsafe { CStr::from_ptr(cstr_buffer.as_ptr()) }
        };

        c_str.to_str().map(|s| s.to_owned()).map_err(|_| AmdError(result))
    }

    /// Retrieves a [`amdsmi_engine_usage_t`] structure containing all data about GPU device activities.
    fn get_device_activity(&self) -> Result<amdsmi_engine_usage_t, AmdError> {
        let mut info = MaybeUninit::<amdsmi_engine_usage_t>::uninit();
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_activity(self.inner, info.as_mut_ptr())
        };

        get_value(result)?;
        Ok(unsafe { info.assume_init() })
    }

    /// Retrieves the energy consumption of the GPU device.
    fn get_device_energy_consumption(&self) -> Result<AmdEnergyConsumptionInfo, AmdError> {
        let mut consumption = AmdEnergyConsumptionInfo {
            energy: 0,
            resolution: 0.0,
            timestamp: 0,
        };

        let result = unsafe {
            self.amdsmi.amdsmi.amdsmi_get_energy_count(
                self.inner,
                &mut consumption.energy as *mut u64,
                &mut consumption.resolution as *mut f32,
                &mut consumption.timestamp as *mut u64,
            )
        };

        get_value(result)?;
        Ok(consumption)
    }

    /// Retrieves the memory consumption of the GPU device.
    fn get_device_memory_usage(&self, mem_type: amdsmi_memory_type_t) -> Result<u64, AmdError> {
        let mut used = 0;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_memory_usage(self.inner, mem_type, &mut used)
        };

        get_value(result)?;
        Ok(used)
    }

    /// Retrieves a [`amdsmi_power_info_t`] structure containing all data about GPU device power consumption.
    fn get_device_power_consumption(&self) -> Result<amdsmi_power_info_t, AmdError> {
        let mut info = unsafe { zeroed() };
        let result = unsafe { self.amdsmi.amdsmi.amdsmi_get_power_info(self.inner, &mut info) };

        get_value(result)?;
        Ok(info)
    }

    /// Retrieves the power management status accessability of the GPU device.
    fn get_device_power_managment(&self) -> Result<bool, AmdError> {
        let mut enabled = false;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_is_gpu_power_management_enabled(self.inner, &mut enabled)
        };

        get_value(result)?;
        Ok(enabled)
    }

    /// Retrieves the temperature of a given area of the GPU device.
    ///
    /// # Arguments
    ///
    /// - `sensor_type`: Temperature retrieved by a [`amdsmi_temperature_metric_t`] sensor on AMD GPU hardware.
    /// - `metric`: Temperature type [`amdsmi_temperature_metric_t`] analysed (current, average...).
    fn get_device_temperature(
        &self,
        sensor_type: amdsmi_temperature_type_t,
        metric: amdsmi_temperature_metric_t,
    ) -> Result<i64, AmdError> {
        let mut temperature = 0;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_temp_metric(self.inner, sensor_type, metric, &mut temperature)
        };

        get_value(result)?;
        Ok(temperature)
    }

    /// Retrieves the voltage of a given area of the GPU device.
    ///
    /// # Arguments
    ///
    /// - `sensor_type`: Voltage retrieved by a [`amdsmi_voltage_type_t`] sensor on AMD GPU hardware.
    /// - `metric`: Voltage type [`amdsmi_voltage_metric_t`] analysed (current, average...).
    fn get_device_voltage(
        &self,
        sensor_type: amdsmi_voltage_type_t,
        metric: amdsmi_voltage_metric_t,
    ) -> Result<i64, AmdError> {
        let mut voltage = 0;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_volt_metric(self.inner, sensor_type, metric, &mut voltage)
        };

        get_value(result)?;
        Ok(voltage)
    }

    /// Retrieves a set of [`amdsmi_proc_info_t`] structure containing data about running processes on the GPU device.
    fn get_device_process_list(&self) -> Result<Vec<amdsmi_proc_info_t>, AmdError> {
        let mut max_processes = 0;

        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_process_list(self.inner, &mut max_processes, null_mut())
        };

        if result != SUCCESS && result != OVERFLOW {
            return Err(AmdError(result));
        }

        let mut list = vec![MaybeUninit::<amdsmi_proc_info_t>::uninit(); max_processes as usize];

        let result = unsafe {
            self.amdsmi.amdsmi.amdsmi_get_gpu_process_list(
                self.inner,
                &mut max_processes,
                list.as_mut_ptr() as *mut amdsmi_proc_info_t,
            )
        };

        get_value(result)?;

        let list = unsafe { transmute::<Vec<MaybeUninit<amdsmi_proc_info_t>>, Vec<amdsmi_proc_info_t>>(list) };
        Ok(list)
    }
}
