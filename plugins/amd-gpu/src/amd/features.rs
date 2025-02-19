use amdsmi::{
    AmdsmiMemoryTypeT, AmdsmiStatusT, AmdsmiTemperatureMetricT, AmdsmiTemperatureTypeT, amdsmi_get_energy_count,
    amdsmi_get_gpu_activity, amdsmi_get_gpu_memory_usage, amdsmi_get_gpu_process_list, amdsmi_get_power_info,
    amdsmi_get_temp_metric, amdsmi_is_gpu_power_management_enabled,
};
use std::ffi::c_void;
use std::{collections::HashMap, fmt::Display};

// Memories values available
pub const MEMORY_TYPE: [(AmdsmiMemoryTypeT, &str); 2] = [
    (AmdsmiMemoryTypeT::AmdsmiMemTypeGtt, "memory_graphic_translation_table"),
    (AmdsmiMemoryTypeT::AmdsmiMemTypeVram, "memory_video_computing"),
];

// Temperature sensors values available
pub const SENSOR_TYPE: [(AmdsmiTemperatureTypeT, &str); 8] = [
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeEdge, "thermal_global"),
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHotspot, "thermal_hotspot"),
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeVram, "thermal_vram"),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm0,
        "thermal_high_bandwidth_memory_0",
    ),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm1,
        "thermal_high_bandwidth_memory_1",
    ),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm2,
        "thermal_high_bandwidth_memory_2",
    ),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm3,
        "thermal_high_bandwidth_memory_3",
    ),
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypePlx, "thermal_pci_bus"),
];

/// Indicates which features are available on a given ADM GPU device.
#[derive(Debug, Default)]
pub struct OptionalFeatures {
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU engine units usage (graphics, memory and average multimedia engines) feature validity.
    pub gpu_engine_usage: bool,
    /// GPU memory usage (VRAM, GTT) feature validity.
    pub gpu_memory_usages: HashMap<AmdsmiMemoryTypeT, bool>,
    /// GPU electric power consumption feature validity.
    pub gpu_power_consumption: bool,
    /// GPU temperature feature validity.
    pub gpu_temperatures: HashMap<AmdsmiTemperatureTypeT, bool>,
    /// GPU power management feature validity.
    pub gpu_state_management: bool,
    /// GPU power management feature validity.
    pub gpu_process_info: bool,
}

/// Checks if a feature is supported by the available GPU by inspecting the return type of an AMDSMI function.
pub fn is_supported<T>(res: Result<T, AmdsmiStatusT>) -> Result<bool, AmdsmiStatusT> {
    match res {
        Ok(_) => Ok(true),
        Err(AmdsmiStatusT::AmdsmiStatusNotSupported) => Ok(false),
        Err(AmdsmiStatusT::AmdsmiStatusNotYetImplemented) => Ok(false),
        Err(AmdsmiStatusT::AmdsmiStatusUnexpectedData) => Ok(false),
        Err(e) => Err(e),
    }
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(device: *mut c_void) -> Result<Self, AmdsmiStatusT> {
        let mut gpu_memory_usages = HashMap::new();
        for &(memory, _) in &MEMORY_TYPE {
            let supported = is_supported(amdsmi_get_gpu_memory_usage(device, memory))?;
            gpu_memory_usages.insert(memory, supported);
        }

        let mut gpu_temperatures = HashMap::new();
        for &(sensor, _) in &SENSOR_TYPE {
            let supported = is_supported(amdsmi_get_temp_metric(
                device,
                sensor,
                AmdsmiTemperatureMetricT::AmdsmiTempCurrent,
            ))?;
            gpu_temperatures.insert(sensor, supported);
        }

        Ok(Self {
            gpu_energy_consumption: is_supported(amdsmi_get_energy_count(device))?,
            gpu_engine_usage: is_supported(amdsmi_get_gpu_activity(device))?,
            gpu_power_consumption: is_supported(amdsmi_get_power_info(device))?,
            gpu_state_management: is_supported(amdsmi_is_gpu_power_management_enabled(device))?,
            gpu_process_info: is_supported(amdsmi_get_gpu_process_list(device))?,
            gpu_memory_usages,
            gpu_temperatures,
        })
    }

    // Test and return the availability of feature on a given
    pub fn with_detected_features(device: *mut c_void) -> Result<(*mut c_void, Self), AmdsmiStatusT> {
        Self::detect_on(device).map(|features| (device, features))
    }

    pub fn has_any(&self) -> bool {
        !(!self.gpu_state_management
            && !self.gpu_energy_consumption
            && !self.gpu_engine_usage
            && !self.gpu_power_consumption
            && !self.gpu_process_info
            && !self.gpu_memory_usages.values().any(|&v| v)
            && !self.gpu_temperatures.values().any(|&v| v))
    }
}

impl Display for OptionalFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut available = Vec::new();

        if self.gpu_state_management {
            available.push("gpu_state_management".to_string());
        }
        if self.gpu_energy_consumption {
            available.push("gpu_energy_consumption".to_string());
        }
        if self.gpu_engine_usage {
            available.push("gpu_engine_usage".to_string());
        }
        if self.gpu_power_consumption {
            available.push("gpu_power_consumption".to_string());
        }
        if self.gpu_process_info {
            available.push("gpu_process_info".to_string());
        }

        for (mem_type, supported) in &self.gpu_memory_usages {
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
mod tests {
    use super::*;

    // Mock optional features
    fn mock_optional_features() -> OptionalFeatures {
        let mut gpu_memory_usages = HashMap::new();
        gpu_memory_usages.insert(AmdsmiMemoryTypeT::AmdsmiMemTypeGtt, false);
        gpu_memory_usages.insert(AmdsmiMemoryTypeT::AmdsmiMemTypeVram, false);

        let mut gpu_temperatures = HashMap::new();
        for (sensor_type, _) in &SENSOR_TYPE {
            gpu_temperatures.insert(*sensor_type, false);
        }

        OptionalFeatures {
            gpu_energy_consumption: false,
            gpu_engine_usage: false,
            gpu_memory_usages,
            gpu_power_consumption: false,
            gpu_temperatures,
            gpu_state_management: false,
            gpu_process_info: false,
        }
    }

    // Test `fmt` function
    #[test]
    fn test_fmt_sucess() {
        let mut features = mock_optional_features();

        features.gpu_state_management = true;
        features.gpu_energy_consumption = true;
        features.gpu_engine_usage = true;
        features.gpu_power_consumption = true;
        features.gpu_process_info = true;
        features
            .gpu_memory_usages
            .insert(AmdsmiMemoryTypeT::AmdsmiMemTypeVram, true);
        features
            .gpu_temperatures
            .insert(AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeEdge, true);

        assert_eq!(
            format!("{features}"),
            "gpu_state_management, gpu_energy_consumption, gpu_engine_usage, gpu_power_consumption, gpu_process_info, gpu_memory_usages::AmdsmiMemTypeFirst, gpu_temperatures::AmdsmiTemperatureTypeEdge"
        );
    }

    // Test `is_supported` function with identified AmdsmiStatusT errors to disable a feature
    #[test]
    fn test_is_supported_errors() {
        let errors = [
            AmdsmiStatusT::AmdsmiStatusNotSupported,
            AmdsmiStatusT::AmdsmiStatusNotYetImplemented,
            AmdsmiStatusT::AmdsmiStatusUnexpectedData,
        ];
        for &err in &errors {
            let ret: Result<i32, AmdsmiStatusT> = Err(err);
            let res = is_supported(ret).unwrap();
            assert!(!res);
        }
    }

    // Test `is_supported` function with other AmdsmiStatusT errors
    #[test]
    fn test_is_supported_other_error() {
        let err = AmdsmiStatusT::AmdsmiStatusUnknownError;
        let ret: Result<i32, AmdsmiStatusT> = Err(err);
        let res = is_supported(ret);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), err);
    }

    // Test `has_any` function with no feature available
    #[test]
    fn test_has_any_all_false() {
        let features = OptionalFeatures {
            gpu_energy_consumption: false,
            gpu_engine_usage: false,
            gpu_memory_usages: HashMap::new(),
            gpu_power_consumption: false,
            gpu_temperatures: HashMap::new(),
            gpu_state_management: false,
            gpu_process_info: false,
        };
        assert!(!features.has_any());
    }
}
