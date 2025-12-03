use mockall::automock;
use std::{
    ffi::CStr,
    mem::{MaybeUninit, transmute, zeroed},
    os::raw::c_char,
    ptr::null_mut,
};
use thiserror::Error;

use crate::bindings::*;

const LIB_PATH: &str = "libamd_smi.so";

/// Error treatment concerning AMD SMI library.
///
/// # Arguments
///
/// Take a status of [`amdsmi_status_t`] provided by AMD SMI library to catch dynamically the occurred error.
#[derive(Debug, Error)]
#[error("amd-smi library error: {0}")]
pub struct AmdError(pub amdsmi_status_t);

pub const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
pub const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

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

#[derive(Debug, Error)]
pub enum AmdInitError {
    #[error("amd-smi init error")]
    Init(#[from] AmdError),
    #[error("Failed to load {LIB_PATH}")]
    Load(#[from] libloading::Error),
}

pub struct AmdSmiLib {
    amdsmi: libamd_smi,
}

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
        const INIT_FLAG: amdsmi_init_flags_t = amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS;
        let amdsmi = unsafe { libamd_smi::new(LIB_PATH) }?;
        let result = unsafe { amdsmi.amdsmi_init(INIT_FLAG.into()) };

        if result == SUCCESS {
            Ok(Self {
                amdsmi: unsafe { libamd_smi::new(LIB_PATH) }?,
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
    pub fn stop(&mut self) -> Result<(), AmdError> {
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
    pub fn get_socket_handles<'a>(&'a self) -> Result<Vec<SocketHandle<'a>>, AmdError> {
        unsafe {
            let mut socket_count = 0;
            let result = self.amdsmi.amdsmi_get_socket_handles(&mut socket_count, null_mut());
            if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
                return Err(AmdError(result));
            }

            let mut socket_handles = vec![zeroed(); socket_count as usize];

            let result = self
                .amdsmi
                .amdsmi_get_socket_handles(&mut socket_count, socket_handles.as_mut_ptr());
            if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
                socket_handles.truncate(socket_count as usize);
                Ok(socket_handles
                    .into_iter()
                    .map(|s| SocketHandle { amdsmi: self, inner: s })
                    .collect())
            } else {
                Err(AmdError(result))
            }
        }
    }
}

pub struct SocketHandle<'a> {
    amdsmi: &'a AmdSmiLib,
    inner: amdsmi_socket_handle,
}

#[automock]
impl<'a> SocketHandle<'a> {
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
    pub fn get_processor_handles(&self) -> Result<Vec<ProcessorHandle<'a>>, AmdError> {
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
                    amdsmi: self.amdsmi,
                    inner: p,
                })
                .collect())
        } else {
            Err(AmdError(result))
        }
    }
}

pub struct ProcessorHandle<'a> {
    amdsmi: &'a AmdSmiLib,
    inner: amdsmi_processor_handle,
}

#[automock]
impl<'a> ProcessorHandle<'a> {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
    pub fn get_device_uuid(&self) -> Result<String, AmdError> {
        unsafe {
            let mut uuid_buffer = vec![0 as c_char; AMDSMI_GPU_UUID_SIZE as usize];
            let mut uuid_length = AMDSMI_GPU_UUID_SIZE;
            let result =
                self.amdsmi
                    .amdsmi
                    .amdsmi_get_gpu_device_uuid(self.inner, &mut uuid_length, uuid_buffer.as_mut_ptr());

            if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
                let c_str = CStr::from_ptr(uuid_buffer.as_ptr());
                match c_str.to_str() {
                    Ok(uuid_str) => Ok(uuid_str.to_owned()),
                    Err(_) => Err(AmdError(result)),
                }
            } else {
                Err(AmdError(result))
            }
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value
    pub fn get_device_activity(&self) -> Result<amdsmi_engine_usage_t, AmdError> {
        let mut info = MaybeUninit::<amdsmi_engine_usage_t>::uninit();
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_activity(self.inner, info.as_mut_ptr())
        };

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value
    pub fn get_device_energy(&self) -> Result<(u64, f32, u64), AmdError> {
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

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
    pub fn get_device_memory_usage(&self, mem_type: amdsmi_memory_type_t) -> Result<u64, AmdError> {
        let mut used = 0;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_memory_usage(self.inner, mem_type, &mut used)
        };

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
    pub fn get_device_power(&self) -> Result<amdsmi_power_info_t, AmdError> {
        unsafe {
            let mut info: amdsmi_power_info_t = zeroed();
            let result = self.amdsmi.amdsmi.amdsmi_get_power_info(self.inner, &mut info);

            if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
                Ok(info)
            } else {
                Err(AmdError(result))
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
    pub fn get_device_power_managment(&self) -> Result<bool, AmdError> {
        let mut enabled = false;
        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_is_gpu_power_management_enabled(self.inner, &mut enabled)
        };

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
    pub fn get_device_temperature(
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

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
    pub fn get_device_voltage(
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

        if result == amdsmi_status_t_AMDSMI_STATUS_SUCCESS {
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
    /// - A [`amdsmi_status_t`] error if we can't to retrieve the value.
    pub fn get_device_process_list(&self) -> Result<Vec<amdsmi_proc_info_t>, AmdError> {
        let mut max_processes = 64;
        let mut process_list = Vec::with_capacity(max_processes as usize);
        let list = process_list.as_mut_ptr() as *mut amdsmi_proc_info_t;

        let result = unsafe {
            self.amdsmi
                .amdsmi
                .amdsmi_get_gpu_process_list(self.inner, &mut max_processes, list)
        };
        if result != amdsmi_status_t_AMDSMI_STATUS_SUCCESS && result != amdsmi_status_t_AMDSMI_STATUS_OUT_OF_RESOURCES {
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

#[cfg(test)]
mod tests {
    use mockall_double::double;

    #[double]
    use crate::interface::{AmdSmiLib, ProcessorHandle, SocketHandle};

    #[test]
    fn essai_mock() {
        // initialisation du mock
        let mut mock = AmdSmiLib::new();

        // on veut que, quand socket_handles() est appelé, il renvoie Ok(vec vide)
        mock.expect_get_socket_handles()
            .returning(|| Ok(vec![SocketHandle::new()]));

        // scénario de test: on appelle les fonctions de lib
        let res = mock.get_socket_handles();

        // vérification des expect
        mock.checkpoint();

        // (autre test si on veut)
    }
}
