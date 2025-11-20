use crate::{bindings::*, load_amdsmi};
use std::fmt::Display;

use super::utils::*;

const NO_PERM: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_NO_PERM;
const NOT_SUPPORTED: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_NOT_SUPPORTED;
const NOT_YET_IMPLEMENTED: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_NOT_YET_IMPLEMENTED;
const UNEXPECTED_DATA: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_UNEXPECTED_DATA;

/// Indicates which features are available on a given ADM GPU device.
#[derive(Debug, Default)]
pub struct OptionalFeatures {
    /// GPU activity usage feature validity.
    pub gpu_activity_usage: bool,
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU memory usage (VRAM, GTT) feature validity.
    pub gpu_memories_usage: Vec<(amdsmi_memory_type_t, bool)>,
    /// GPU electric power consumption feature validity.
    pub gpu_power_consumption: bool,
    /// GPU power management feature validity.
    pub gpu_power_state_management: bool,
    /// GPU temperature feature validity.
    pub gpu_temperatures: Vec<(amdsmi_temperature_metric_t, bool)>,
    /// GPU power management feature validity.
    pub gpu_process_info: bool,
    // GPU socket voltage feature validity.
    pub gpu_voltage: bool,
}

/// Checks if a feature is supported by the available GPU by inspecting the return type of an AMDSMI function.
pub fn is_supported<T>(res: Result<T, amdsmi_status_t>) -> Result<bool, amdsmi_status_t> {
    match res {
        Ok(_) => Ok(true),
        Err(NO_PERM) => Ok(false),
        Err(NOT_SUPPORTED) => Ok(false),
        Err(NOT_YET_IMPLEMENTED) => Ok(false),
        Err(UNEXPECTED_DATA) => Ok(false),
        Err(e) => Err(e),
    }
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(processor_handle: amdsmi_processor_handle) -> Result<Self, amdsmi_status_t> {
        const TEMP_METRIC: amdsmi_temperature_metric_t = amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT;
        const VOLTAGE_SENSOR_TYPE: amdsmi_voltage_type_t = amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX;
        const VOLTAGE_METRIC: amdsmi_voltage_metric_t = amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT;

        let amdsmi = load_amdsmi().map_err(|_| amdsmi_status_t_AMDSMI_STATUS_DRM_ERROR)?;

        let mut gpu_temperatures = Vec::new();
        let mut gpu_memories_usage = Vec::new();

        for &(mem_type, _) in &MEMORY_TYPE {
            let supported = is_supported(get_device_memory_usage(&amdsmi, processor_handle, mem_type))?;
            gpu_memories_usage.push((mem_type, supported));
        }

        for &(sensor, _) in &SENSOR_TYPE {
            let supported = is_supported(get_device_temperature(&amdsmi, processor_handle, sensor, TEMP_METRIC))?;
            gpu_temperatures.push((sensor, supported));
        }

        Ok(Self {
            gpu_activity_usage: is_supported(get_device_activity(&amdsmi, processor_handle))?,
            gpu_energy_consumption: is_supported(get_device_energy(&amdsmi, processor_handle))?,
            gpu_power_consumption: is_supported(get_device_power(&amdsmi, processor_handle))?,
            gpu_power_state_management: is_supported(get_device_power_managment(&amdsmi, processor_handle))?,
            gpu_process_info: is_supported(get_device_process_list(&amdsmi, processor_handle))?,
            gpu_voltage: is_supported(get_device_voltage(
                &amdsmi,
                processor_handle,
                VOLTAGE_SENSOR_TYPE,
                VOLTAGE_METRIC,
            ))?,
            gpu_memories_usage,
            gpu_temperatures,
        })
    }

    /// Test and return the availability of feature on a given
    pub fn with_detected_features(
        device: amdsmi_processor_handle,
    ) -> Result<(amdsmi_processor_handle, Self), amdsmi_status_t> {
        Self::detect_on(device).map(|features| (device, features))
    }

    pub fn has_any(&self) -> bool {
        !(!self.gpu_energy_consumption
            && !self.gpu_activity_usage
            && !self.gpu_power_consumption
            && !self.gpu_power_state_management
            && !self.gpu_process_info
            && !self.gpu_voltage
            && !self.gpu_memories_usage.iter().any(|&(_memory, supported)| supported)
            && !self.gpu_temperatures.iter().any(|&(_sensor, supported)| supported))
    }
}

impl Display for OptionalFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut available = Vec::new();

        if self.gpu_activity_usage {
            available.push("gpu_activity_usage".to_string());
        }
        if self.gpu_energy_consumption {
            available.push("gpu_energy_consumption".to_string());
        }
        if self.gpu_power_consumption {
            available.push("gpu_power_consumption".to_string());
        }
        if self.gpu_power_state_management {
            available.push("gpu_power_state_management".to_string());
        }
        if self.gpu_process_info {
            available.push("gpu_process_info".to_string());
        }
        if self.gpu_voltage {
            available.push("gpu_voltage".to_string());
        }

        for (mem_type, supported) in &self.gpu_memories_usage {
            if *supported {
                available.push(format!("gpu_memory_usages::{mem_type:?}"));
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
mod tests_feature {
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
            gpu_activity_usage: false,
            gpu_energy_consumption: false,
            gpu_power_consumption: false,
            gpu_power_state_management: false,
            gpu_process_info: false,
            gpu_voltage: false,
            gpu_memories_usage,
            gpu_temperatures,
        }
    }

    // Test `fmt` function
    #[test]
    fn test_fmt_sucess() {
        let mut features = mock_optional_features();

        features.gpu_activity_usage = true;
        features.gpu_energy_consumption = true;
        features.gpu_power_consumption = true;
        features.gpu_power_state_management = true;
        features.gpu_process_info = true;
        features.gpu_voltage = true;

        features
            .gpu_memories_usage
            .push((amdsmi_memory_type_t_AMDSMI_MEM_TYPE_VRAM, true));
        features
            .gpu_temperatures
            .push((amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE, true));

        assert_eq!(
            format!("{features}"),
            "gpu_activity_usage, gpu_energy_consumption, gpu_power_consumption, gpu_power_state_management, gpu_process_info, gpu_voltage, gpu_memory_usages::0, gpu_temperatures::0"
        );
    }

    // Test `is_supported` function with identified amdsmi status errors to disable a feature
    #[test]
    fn test_is_supported_errors() {
        let errors = [NO_PERM, NOT_SUPPORTED, NOT_YET_IMPLEMENTED, UNEXPECTED_DATA];
        for &err in &errors {
            let ret: Result<i32, amdsmi_status_t> = Err(err);
            let res = is_supported(ret).unwrap();
            assert!(!res);
        }
    }

    // Test `is_supported` function with other amdsmi status errors
    #[test]
    fn test_is_supported_other_error() {
        let err = amdsmi_status_t_AMDSMI_STATUS_UNKNOWN_ERROR;
        let ret: Result<i32, amdsmi_status_t> = Err(err);
        let res = is_supported(ret);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), err);
    }

    // Test `has_any` function with no feature available
    #[test]
    fn test_has_any_all_false() {
        let features = OptionalFeatures {
            gpu_activity_usage: false,
            gpu_energy_consumption: false,
            gpu_power_consumption: false,
            gpu_power_state_management: false,
            gpu_process_info: false,
            gpu_voltage: false,
            gpu_memories_usage: Vec::new(),
            gpu_temperatures: Vec::new(),
        };
        assert!(!features.has_any());
    }

    // Test `has_any` function with identified amdsmi status errors to disable a feature
    #[test]
    fn test_has_any_all_supported() {
        let features = OptionalFeatures {
            gpu_activity_usage: is_supported(Ok(())).unwrap(),
            gpu_energy_consumption: is_supported(Ok(())).unwrap(),
            gpu_power_consumption: is_supported(Ok(())).unwrap(),
            gpu_power_state_management: is_supported(Ok(())).unwrap(),
            gpu_process_info: is_supported(Ok(())).unwrap(),
            gpu_voltage: is_supported(Ok(())).unwrap(),
            gpu_memories_usage: vec![(amdsmi_memory_type_t_AMDSMI_MEM_TYPE_VRAM, true)],
            gpu_temperatures: vec![(amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT, true)],
        };
        assert!(features.has_any());
    }
}
