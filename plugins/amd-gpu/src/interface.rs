use mockall::automock;
use std::{
    ffi::CStr,
    mem::{MaybeUninit, transmute, zeroed},
    os::raw::c_char,
    ptr::null_mut,
    sync::Arc,
};
use thiserror::Error;

use crate::{amd::utils::*, bindings::*};

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
pub struct AmdSmiLib {
    amdsmi: Arc<libamd_smi>,
}

#[cfg(not(test))]
pub type MockableSocketHandle = SocketHandle;
#[cfg(test)]
pub type MockableSocketHandle = MockSocketHandle;

#[cfg(not(test))]
pub type MockableProcessorHandle = ProcessorHandle;
#[cfg(test)]
pub type MockableProcessorHandle = MockProcessorProvider;

#[cfg(not(test))]
pub type AmdSmiRef = AmdSmiLib;
#[cfg(test)]
pub type AmdSmiRef = MockAmdSmiLib;

#[automock]
impl AmdSmiLib {
    /// Call the unsafe C binding function [`amdsmi_init`] to quit amd-smi library and clean properly its resources.
    ///
    /// # Arguments
    ///
    /// - `amdsmi_init_flags_t`: A [`amdsmi_init_flags_t`] type value use to define how AMD hardware we need to initialize (GPU, CPU).
    ///
    /// # Returns
    ///
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value
    pub fn init() -> Result<Self, AmdInitError> {
        let amdsmi = unsafe { libamd_smi::new(LIB_PATH) }?;
        let result = unsafe { amdsmi.amdsmi_init(INIT_FLAG.into()) };

        if result == SUCCESS {
            Ok(Self {
                amdsmi: unsafe { libamd_smi::new(LIB_PATH) }?.into(),
            })
        } else {
            Err(AmdInitError::Init(AmdError(result)))
        }
    }

    /// Call the unsafe C binding function [`amdsmi_shut_down`] to quit amd-smi library and clean properly its resources.
    ///
    /// # Returns
    ///
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value
    pub fn stop(&self) -> Result<(), AmdError> {
        let result = unsafe { self.amdsmi.amdsmi_shut_down() };
        if result == SUCCESS {
            Ok(())
        } else {
            Err(AmdError(result))
        }
    }

    /// Call the unsafe C binding function [`amdsmi_get_socket_handles`] to retrieve socket handles detected on system.
    ///
    /// # Returns
    ///
    /// - Set of [`amdsmi_socket_handle`] pointer to a block of memory to which values will be written.
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value
    #[cfg(test)]
    pub fn get_socket_handles(&self) -> Result<Vec<MockableSocketHandle>, AmdError> {
        panic!("you should use the mock in the tests");
    }

    #[cfg(not(test))]
    pub fn get_socket_handles(&self) -> Result<Vec<MockableSocketHandle>, AmdError> {
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
                .map(|s| SocketHandle {
                    amdsmi: self.clone(),
                    inner: s,
                })
                .collect())
        } else {
            Err(AmdError(result))
        }
    }
}

pub struct SocketHandle {
    amdsmi: AmdSmiLib,
    inner: amdsmi_socket_handle,
}

#[automock]
impl SocketHandle {
    #[cfg(test)]
    pub fn get_processor_handles(&self) -> Result<Vec<MockableProcessorHandle>, AmdError> {
        panic!("you should use the mock in the tests");
    }

    /// Call the unsafe C binding function [`amdsmi_get_processor_handles`] to retrieve socket handles detected for a give socket.
    ///
    /// # Arguments
    ///
    /// Pointer on a address coming from [`SocketHandle`].
    ///
    /// # Returns
    ///
    /// - Set of [`ProcessorHandle`] of pointer to a block of memory to which values will be written.
    /// - A [`AmdError`] error if we can't to retrieve the value
    #[cfg(not(test))]
    pub fn get_processor_handles(&self) -> Result<Vec<MockableProcessorHandle>, AmdError> {
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
                .map(|p| ProcessorHandle {
                    amdsmi: self.amdsmi.clone(),
                    inner: p,
                })
                .collect())
        } else {
            Err(AmdError(result))
        }
    }
}

#[automock]
pub trait ProcessorProvider {
    fn get_device_uuid(&self) -> Result<String, AmdError>;
    fn get_device_activity(&self) -> Result<amdsmi_engine_usage_t, AmdError>;
    fn get_device_energy_consumption(&self) -> Result<(u64, f32, u64), AmdError>;
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

pub struct ProcessorHandle {
    pub amdsmi: AmdSmiLib,
    pub inner: amdsmi_processor_handle,
}

impl ProcessorProvider for ProcessorHandle {
    /// Call the unsafe C binding function [`amdsmi_get_gpu_device_uuid`] to retrieve gpu uuid identifier values.
    /// Convert a declared buffer with an [`AMDSMI_GPU_UUID_SIZE`] in UTF-8 Rust string.
    ///
    /// # Arguments
    ///
    /// Address pointer on a AMD GPU device coming from [`ProcessorHandle`].
    ///
    /// # Returns
    ///
    /// - The formatted string corresponding of UUID of a gpu device.
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_uuid(&self) -> Result<String, AmdError> {
        let mut uuid_buffer = vec![0 as c_char; AMDSMI_GPU_UUID_SIZE as usize];
        let mut uuid_length = AMDSMI_GPU_UUID_SIZE;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_device_uuid(self.inner, &mut uuid_length, uuid_buffer.as_mut_ptr())
        };

        if result == SUCCESS {
            let c_str = unsafe { CStr::from_ptr(uuid_buffer.as_ptr()) };
            match c_str.to_str() {
                Ok(uuid_str) => Ok(uuid_str.to_owned()),
                Err(_) => Err(AmdError(result)),
            }
        } else {
            Err(AmdError(result))
        }
    }

    /// Call the unsafe C binding function [`amdsmi_get_gpu_activity`] to retrieve gpu activity values.
    ///
    /// # Arguments
    ///
    /// Address pointer on a AMD GPU device coming from [`ProcessorHandle`].
    ///
    /// # Returns
    ///
    /// - `gfx`: Main graphic unit of an AMD GPU that release graphic tasks and rendering in %.
    /// - `mm`: Unit responsible for managing and accessing VRAM, and coordinating data exchanges between it and the GPU in %.
    /// - `umc`: Single memory address space accessible from any processor within a system in %.
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_activity(&self) -> Result<amdsmi_engine_usage_t, AmdError> {
        let mut info = MaybeUninit::<amdsmi_engine_usage_t>::uninit();
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_activity(self.inner, info.as_mut_ptr())
        };

        if result == SUCCESS {
            let info = unsafe { info.assume_init() };
            Ok(info)
        } else {
            Err(AmdError(result))
        }
    }

    /// Call the unsafe C binding function [`amdsmi_get_energy_count`] to retrieve gpu energy consumption values.
    ///
    /// # Arguments
    ///
    /// Address pointer on a AMD GPU device coming from [`ProcessorHandle`].
    ///
    /// # Returns
    ///
    /// - `energy`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value.
    /// - `resolution`: Resolution precision of the energy counter in micro Joules.
    /// - `timestamp: Timestamp returned in ns.
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_energy_consumption(&self) -> Result<(u64, f32, u64), AmdError> {
        let mut energy = 0;
        let mut resolution = 0.0;
        let mut timestamp = 0;

        let result = unsafe {
            self.amdsmi.amdsmi.amdsmi_get_energy_count(
                self.inner,
                &mut energy as *mut u64,
                &mut resolution as *mut f32,
                &mut timestamp as *mut u64,
            )
        };

        if result == SUCCESS {
            Ok((energy, resolution, timestamp))
        } else {
            Err(AmdError(result))
        }
    }

    /// Call the unsafe C binding function [`amdsmi_get_gpu_memory_usage`] to retrieve gpu memories used values.
    ///
    /// # Arguments
    ///
    /// - Address pointer on a AMD GPU device.
    /// - `mem_type`: Kind of memory used among [`amdsmi_memory_type_t`].
    ///
    /// # Returns
    ///
    /// - `used`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in Bytes.
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_memory_usage(&self, mem_type: amdsmi_memory_type_t) -> Result<u64, AmdError> {
        let mut used = 0;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_memory_usage(self.inner, mem_type, &mut used)
        };

        if result == SUCCESS {
            Ok(used)
        } else {
            Err(AmdError(result))
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
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_power_consumption(&self) -> Result<amdsmi_power_info_t, AmdError> {
        let mut info: amdsmi_power_info_t = unsafe { zeroed() };
        let result = unsafe { self.amdsmi.amdsmi.amdsmi_get_power_info(self.inner, &mut info) };

        if result == SUCCESS {
            Ok(info)
        } else {
            Err(AmdError(result))
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
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_power_managment(&self) -> Result<bool, AmdError> {
        let mut enabled = false;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_is_gpu_power_management_enabled(self.inner, &mut enabled)
        };

        if result == SUCCESS {
            Ok(enabled)
        } else {
            Err(AmdError(result))
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
    /// - A [`AmdError`] error if we can't to retrieve the value.
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

        if result == SUCCESS {
            Ok(temperature)
        } else {
            Err(AmdError(result))
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
    /// - A [`AmdError`] error if we can't to retrieve the value.
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

        if result == SUCCESS {
            Ok(voltage)
        } else {
            Err(AmdError(result))
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
    /// - A [`AmdError`] error if we can't to retrieve the value.
    fn get_device_process_list(&self) -> Result<Vec<amdsmi_proc_info_t>, AmdError> {
        let mut max_processes = 64;
        let mut process_list = Vec::with_capacity(max_processes as usize);
        let list = process_list.as_mut_ptr() as *mut amdsmi_proc_info_t;

        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_process_list(self.inner, &mut max_processes, list)
        };
        if result != SUCCESS && result != OVERFLOW {
            return Err(AmdError(result));
        }

        unsafe {
            process_list.set_len(max_processes as usize);
        }

        let process_info_list =
            unsafe { transmute::<Vec<MaybeUninit<amdsmi_proc_info_t>>, Vec<amdsmi_proc_info_t>>(process_list) };

        Ok(process_info_list)
    }
}
