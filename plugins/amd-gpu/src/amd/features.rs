use rocm_smi_lib::*;
use std::{fmt::Display, mem::zeroed, ptr::null_mut};

/// Memory types values available.
pub const MEMORY_TYPE: [(rsmi_memory_type_t, &str); 2] = [
    (rsmi_memory_type_t_RSMI_MEM_TYPE_GTT, "memory_graphic_translation_table"),
    (rsmi_memory_type_t_RSMI_MEM_TYPE_VRAM, "memory_video_computing"),
];

/// Temperature sensors values available.
pub const SENSOR_TYPE: [(u32, &str); 7] = [
    (rsmi_temperature_type_t_RSMI_TEMP_TYPE_EDGE, "thermal_global"),
    (rsmi_temperature_type_t_RSMI_TEMP_TYPE_JUNCTION, "thermal_hotspot"),
    (rsmi_temperature_type_t_RSMI_TEMP_TYPE_MEMORY, "thermal_memory"),
    (
        rsmi_temperature_type_t_RSMI_TEMP_TYPE_HBM_0,
        "thermal_high_bandwidth_memory_0",
    ),
    (
        rsmi_temperature_type_t_RSMI_TEMP_TYPE_HBM_1,
        "thermal_high_bandwidth_memory_1",
    ),
    (
        rsmi_temperature_type_t_RSMI_TEMP_TYPE_HBM_2,
        "thermal_high_bandwidth_memory_2",
    ),
    (
        rsmi_temperature_type_t_RSMI_TEMP_TYPE_HBM_3,
        "thermal_high_bandwidth_memory_3",
    ),
];

/// Indicates which features are available on a given ADM GPU device.
#[derive(Debug, Default)]
pub struct OptionalFeatures {
    /// GPU activity utilization feature validity.
    pub gpu_activity: bool,
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU memories usage feature validity.
    pub gpu_memories_usage: Vec<(rsmi_memory_type_t, bool)>,
    /// GPU electric power consumption feature validity.
    pub gpu_power_consumption: bool,
    /// GPU temperature feature validity.
    pub gpu_temperatures: Vec<(rsmi_temperature_metric_t, bool)>,
    /// GPU socket voltage consumption feature validity.
    pub gpu_voltage_consumption: bool,
    /// GPU process info feature validity.
    pub gpu_process_info: bool,
}

/// Call the unsafe C binding function [`rsmi_dev_activity_metric_get`] to retrieves gpu activity values.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
///
/// # Returns
///
/// - `gfx`: Main graphic unit of an AMD GPU that release graphic tasks and rendering in %.
/// - `mm`: Unit responsible for managing and accessing VRAM, and coordinating data exchanges between it and the GPU in %.
/// - `umc`: Single memory address space accessible from any processor within a system in %.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_activity(dv_ind: u32) -> Result<(u16, u16, u16), rsmi_status_t> {
    unsafe fn get_activity(dv_ind: u32, metric_type: u32) -> Result<u16, rsmi_status_t> {
        let mut counter = unsafe { zeroed::<rsmi_activity_metric_counter_t>() };
        let result = unsafe { rsmi_dev_activity_metric_get(dv_ind, metric_type, &mut counter) };
        if result != rsmi_status_t_RSMI_STATUS_SUCCESS {
            return Err(result);
        }

        let value = match metric_type {
            rsmi_activity_metric_t_RSMI_ACTIVITY_GFX => counter.average_gfx_activity,
            rsmi_activity_metric_t_RSMI_ACTIVITY_MM => counter.average_mm_activity,
            rsmi_activity_metric_t_RSMI_ACTIVITY_UMC => counter.average_umc_activity,
            _ => 0,
        };
        Ok(value)
    }

    let gfx = unsafe { get_activity(dv_ind, rsmi_activity_metric_t_RSMI_ACTIVITY_GFX)? };
    let mm = unsafe { get_activity(dv_ind, rsmi_activity_metric_t_RSMI_ACTIVITY_MM)? };
    let umc = unsafe { get_activity(dv_ind, rsmi_activity_metric_t_RSMI_ACTIVITY_UMC)? };

    Ok((gfx, mm, umc))
}

/// Call the unsafe C binding function [`rsmi_dev_energy_count_get`] to retrieves gpu energy consumption values.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
///
/// # Returns
///
/// - `energy`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value.
/// - `resolution`: Resolution precision of the energy counter in micro Joules.
/// - `timestamp: Timestamp returned in ns.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_energy(dv_ind: u32) -> Result<(u64, f32, u64), rsmi_status_t> {
    let mut energy = 0;
    let mut resolution = 0.0;
    let mut timestamp = 0;

    let result = unsafe {
        rsmi_dev_energy_count_get(
            dv_ind,
            &mut energy as *mut u64,
            &mut resolution as *mut f32,
            &mut timestamp as *mut u64,
        )
    };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok((energy, resolution, timestamp))
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`rsmi_dev_memory_usage_get`] to retrieves gpu memories used values.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
/// - `mem_type`: Kind of memory used among [`rsmi_memory_type_t`].
///
/// # Returns
///
/// - `used`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in Bytes.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_memory_usage(dv_ind: u32, mem_type: rsmi_memory_type_t) -> Result<u64, rsmi_status_t> {
    let mut used = 0;
    let result = unsafe { rsmi_dev_memory_usage_get(dv_ind, mem_type, &mut used) };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok(used)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`rsmi_dev_power_get`] to retrieves [`RSMI_POWER_TYPE`] gpu power consumption values.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
///
/// # Returns
///
/// - `power`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in µW.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_power(dv_ind: u32) -> Result<u64, rsmi_status_t> {
    let mut power = 0;
    let mut type_ = RSMI_POWER_TYPE::default();
    let result = unsafe { rsmi_dev_power_get(dv_ind, &mut power as *mut u64, &mut type_ as *mut _) };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok(power)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`rsmi_dev_temp_metric_get`] to retrieves gpu temperature values.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
/// - `sensor_type`: Temperature retrieves by a [`rsmi_temperature_metric_t`] sensor on AMD GPU hardware.
/// - `metric`: Temperature type [`rsmi_temperature_metric_t`] analysed (current, average...).
///
/// # Returns
///
/// - `temperature`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in °C.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_temperature(
    dv_ind: u32,
    sensor_type: rsmi_temperature_metric_t,
    metric: rsmi_temperature_metric_t,
) -> Result<i64, rsmi_status_t> {
    let mut temperature = 0;
    let result = unsafe { rsmi_dev_temp_metric_get(dv_ind, sensor_type, metric, &mut temperature) };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok(temperature)
    } else {
        Err(result)
    }
}

/// Call the unsafe C binding function [`rsmi_dev_volt_metric_get`] to retrieves gpu socket voltage values.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
/// - `sensor_type`: Voltage retrieves by a [`rsmi_voltage_type_t`] sensor on AMD GPU hardware.
/// - `metric`: Voltage type [`rsmi_voltage_metric_t`] analysed (current, average...).
///
/// # Returns
///
/// - `voltage`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value in mV.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_voltage(
    dv_ind: u32,
    sensor_type: rsmi_voltage_type_t,
    metric: rsmi_voltage_metric_t,
) -> Result<i64, rsmi_status_t> {
    let mut voltage = 0;
    let result = unsafe { rsmi_dev_volt_metric_get(dv_ind, sensor_type, metric, &mut voltage) };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok(voltage)
    } else {
        Err(result)
    }
}

/// Use firstly the unsafe C binding function [`rsmi_compute_process_info_get`] to retrieve running compute processes count.
/// Use secondly the unsafe C binding function [`rsmi_compute_process_info_by_device_get`] to retrieve data about running compute processes.
///
/// # Arguments
///
/// - `dv_ind`: Index of the AMD GPU device.
///
/// # Returns
///
/// - A vec of [`rsmi_process_info_t`] data concerning retrieved processes.
/// - If no processes are running we return an empty result.
/// - An [`rsmi_status_t`] error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
pub fn get_device_compute_process_info(dv_ind: u32) -> Result<Vec<rsmi_process_info_t>, rsmi_status_t> {
    let mut num_items = 0;

    let res = unsafe { rsmi_compute_process_info_get(null_mut(), &mut num_items) };
    if res != rsmi_status_t_RSMI_STATUS_SUCCESS {
        return Err(res);
    }
    if num_items == 0 {
        return Ok(Vec::with_capacity(0));
    }

    let mut processes = Vec::with_capacity(num_items as usize);
    let res = unsafe { rsmi_compute_process_info_get(processes.as_mut_ptr(), &mut num_items) };
    if res != rsmi_status_t_RSMI_STATUS_SUCCESS {
        return Err(res);
    }
    unsafe {
        processes.set_len(num_items as usize);
    }

    let result = processes
        .into_iter()
        .filter_map(|p| {
            let pid = p.process_id;
            let mut proc_ = unsafe { zeroed() };
            let res = unsafe { rsmi_compute_process_info_by_device_get(pid, dv_ind, &mut proc_) };
            if res == rsmi_status_t_RSMI_STATUS_SUCCESS {
                Some(proc_)
            } else {
                None
            }
        })
        .collect();

    Ok(result)
}

/// Checks if a feature is supported by the available GPU by inspecting the return type of an ROCM-SMI function.
pub fn is_supported<T>(res: Result<T, RocmErr>) -> Result<bool, RocmErr> {
    match res {
        Ok(_) => Ok(true),
        Err(RocmErr::RsmiStatusPermission) => Ok(false),
        Err(RocmErr::RsmiStatusNotSupported) => Ok(false),
        Err(RocmErr::RsmiStatusNotYetImplemented) => Ok(false),
        Err(RocmErr::RsmiStatusUnexpectedData) => Ok(false),
        Err(e) => Err(e),
    }
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(dv_ind: u32) -> Result<(Self, u32), RocmErr> {
        let mut gpu_temperatures = Vec::new();
        let mut gpu_memories_usage = Vec::new();

        for &(sensor, _) in &SENSOR_TYPE {
            let supported = is_supported(Ok(get_device_temperature(
                dv_ind,
                sensor,
                rsmi_temperature_metric_t_RSMI_TEMP_CURRENT,
            )))?;
            gpu_temperatures.push((sensor, supported));
        }

        for &(mem_type, _) in &MEMORY_TYPE {
            let supported = is_supported(Ok(get_device_memory_usage(dv_ind, mem_type)))?;
            gpu_memories_usage.push((mem_type, supported));
        }

        Ok((
            Self {
                gpu_activity: is_supported(Ok(get_device_activity(dv_ind)))?,
                gpu_energy_consumption: is_supported(Ok(get_device_energy(dv_ind)))?,
                gpu_power_consumption: is_supported(Ok(get_device_power(dv_ind)))?,
                gpu_voltage_consumption: is_supported(Ok(get_device_voltage(
                    dv_ind,
                    rsmi_voltage_type_t_RSMI_VOLT_TYPE_FIRST,
                    rsmi_voltage_metric_t_RSMI_VOLT_CURRENT,
                )))?,
                gpu_process_info: is_supported(Ok(get_device_compute_process_info(dv_ind)))?,
                gpu_memories_usage,
                gpu_temperatures,
            },
            dv_ind,
        ))
    }

    pub fn has_any(&self) -> bool {
        !(!self.gpu_activity
            && !self.gpu_energy_consumption
            && !self.gpu_power_consumption
            && !self.gpu_process_info
            && !self.gpu_voltage_consumption
            && !self.gpu_memories_usage.iter().any(|&(_memory, supported)| supported)
            && !self.gpu_temperatures.iter().any(|&(_sensor, supported)| supported))
    }
}

impl Display for OptionalFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut available = Vec::new();

        if self.gpu_activity {
            available.push("gpu_activity".to_string());
        }
        if self.gpu_energy_consumption {
            available.push("gpu_energy_consumption".to_string());
        }
        if self.gpu_power_consumption {
            available.push("gpu_power_consumption".to_string());
        }
        if self.gpu_process_info {
            available.push("gpu_process_info".to_string());
        }
        for (memory_type, supported) in &self.gpu_memories_usage {
            if *supported {
                available.push(format!("gpu_memories_usage::{memory_type:?}"));
            }
        }
        for (temp_type, supported) in &self.gpu_temperatures {
            if *supported {
                available.push(format!("gpu_temperatures::{temp_type:?}"));
            }
        }

        write!(f, "{}", available.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock optional features
    fn mock_optional_features() -> OptionalFeatures {
        let mut gpu_temperatures = Vec::new();
        let mut gpu_memories_usage = Vec::new();

        for (sensor_type, _) in &SENSOR_TYPE {
            gpu_temperatures.push((*sensor_type, false));
        }

        for (memory_type, _) in &MEMORY_TYPE {
            gpu_memories_usage.push((*memory_type, false));
        }

        OptionalFeatures {
            gpu_activity: false,
            gpu_energy_consumption: false,
            gpu_power_consumption: false,
            gpu_voltage_consumption: false,
            gpu_process_info: false,
            gpu_memories_usage,
            gpu_temperatures,
        }
    }

    // Test `fmt` function
    #[test]
    fn test_fmt_sucess() {
        let mut features = mock_optional_features();

        features.gpu_activity = true;
        features.gpu_energy_consumption = true;
        features.gpu_power_consumption = true;
        features.gpu_process_info = true;
        features
            .gpu_memories_usage
            .push((rsmi_memory_type_t_RSMI_MEM_TYPE_VRAM, true));
        features
            .gpu_temperatures
            .push((rsmi_temperature_type_t_RSMI_TEMP_TYPE_EDGE, true));

        assert_eq!(
            format!("{features}"),
            "gpu_activity, gpu_energy_consumption, gpu_power_consumption, gpu_process_info, gpu_memories_usage::0, gpu_temperatures::0"
        );
    }

    // Test `is_supported` function with identified RocmErr errors to disable a feature
    #[test]
    fn test_is_supported_errors() {
        let errors = [
            RocmErr::RsmiStatusPermission,
            RocmErr::RsmiStatusNotSupported,
            RocmErr::RsmiStatusNotYetImplemented,
            RocmErr::RsmiStatusUnexpectedData,
        ];
        for &err in &errors {
            let ret: Result<i32, RocmErr> = Err(err);
            let res = is_supported(ret).unwrap();
            assert!(!res);
        }
    }

    // Test `is_supported` function with other RocmErr errors
    #[test]
    fn test_is_supported_other_error() {
        let err = RocmErr::RsmiStatusUnknownError;
        let ret: Result<i32, RocmErr> = Err(err);
        let res = is_supported(ret);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), err);
    }

    // Test `has_any` function with no feature available
    #[test]
    fn test_has_any_all_false() {
        let features = OptionalFeatures {
            gpu_activity: false,
            gpu_energy_consumption: false,
            gpu_power_consumption: false,
            gpu_voltage_consumption: false,
            gpu_memories_usage: Vec::new(),
            gpu_temperatures: Vec::new(),
            gpu_process_info: false,
        };
        assert!(!features.has_any());
    }
}
