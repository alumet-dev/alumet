use amd_smi_wrapper::{
    AmdError, ProcessorHandle,
    utils::{AmdMemoryType, AmdStatus, AmdTemperatureMetric, AmdTemperatureType, AmdVoltageMetric, AmdVoltageType},
};

use crate::amd::utils::{MEMORY_TYPE, SENSOR_TYPE};
use std::fmt::{self, Display, Formatter};

/// Indicates which features are available on a given ADM GPU device.
#[derive(Debug, Default)]
pub struct OptionalFeatures {
    /// GPU activity usage feature validity.
    pub gpu_activity_usage: bool,
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU memory usage (VRAM, GTT) feature validity.
    pub gpu_memories_usage: Vec<(AmdMemoryType, bool)>,
    /// GPU electric power consumption feature validity.
    pub gpu_power_consumption: bool,
    /// GPU power management feature validity.
    pub gpu_power_state_management: bool,
    /// GPU temperature feature validity.
    pub gpu_temperatures: Vec<(AmdTemperatureType, bool)>,
    /// GPU power management feature validity.
    pub gpu_process: bool,
    // GPU socket voltage feature validity.
    pub gpu_voltage: bool,
}

/// Checks if a feature is supported by the available GPU by inspecting the return type of an AMD-SMI function.
pub fn is_supported<T>(res: Result<T, AmdError>) -> Result<bool, AmdStatus> {
    match res {
        Ok(_) => Ok(true),
        Err(AmdError(status)) => match status {
            AmdStatus::AMDSMI_STATUS_NO_PERM
            | AmdStatus::AMDSMI_STATUS_NOT_SUPPORTED
            | AmdStatus::AMDSMI_STATUS_NOT_YET_IMPLEMENTED
            | AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA => Ok(false),
            other => Err(other),
        },
    }
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(processor_handle: &impl ProcessorHandle) -> Result<Self, AmdStatus> {
        let mut gpu_temperatures = Vec::new();
        let mut gpu_memories_usage = Vec::new();

        for &(mem_type, _) in &MEMORY_TYPE {
            let supported = is_supported(processor_handle.device_memory_usage(mem_type))?;
            gpu_memories_usage.push((mem_type, supported));
        }

        for &(sensor, _) in &SENSOR_TYPE {
            let supported =
                is_supported(processor_handle.device_temperature(sensor, AmdTemperatureMetric::AMDSMI_TEMP_CURRENT))?;
            gpu_temperatures.push((sensor, supported));
        }

        Ok(Self {
            gpu_activity_usage: is_supported(processor_handle.device_activity())?,
            gpu_energy_consumption: is_supported(processor_handle.device_energy_consumption())?,
            gpu_power_consumption: is_supported(processor_handle.device_power_consumption())?,
            gpu_power_state_management: is_supported(processor_handle.device_power_managment())?,
            gpu_process: is_supported(processor_handle.device_process_list())?,
            gpu_voltage: is_supported(processor_handle.device_voltage(
                AmdVoltageType::AMDSMI_VOLT_TYPE_VDDGFX,
                AmdVoltageMetric::AMDSMI_VOLT_CURRENT,
            ))?,
            gpu_memories_usage,
            gpu_temperatures,
        })
    }

    /// Test and return the availability of feature on a given
    pub fn with_detected_features<H: ProcessorHandle>(device: &H) -> Result<(&H, Self), AmdStatus> {
        Self::detect_on(device).map(|features| (device, features))
    }

    pub fn has_any(&self) -> bool {
        self.gpu_energy_consumption
            || self.gpu_activity_usage
            || self.gpu_power_consumption
            || self.gpu_power_state_management
            || self.gpu_process
            || self.gpu_voltage
            || self.gpu_memories_usage.iter().any(|&(_memory, supported)| supported)
            || self.gpu_temperatures.iter().any(|&(_sensor, supported)| supported)
    }
}

impl Display for OptionalFeatures {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
        if self.gpu_process {
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
mod test {
    use amd_smi_wrapper::{AmdError, MockProcessorHandle};

    use super::*;
    use crate::tests::mocks::{
        MOCK_ACTIVITY, MOCK_ENERGY, MOCK_MEMORY, MOCK_POWER, MOCK_TEMPERATURE, MOCK_VOLTAGE, mock_process,
    };

    use std::ptr::eq;

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
            gpu_process: false,
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
        features.gpu_process = true;
        features.gpu_voltage = true;

        features
            .gpu_memories_usage
            .push((AmdMemoryType::AMDSMI_MEM_TYPE_VRAM, true));
        features
            .gpu_temperatures
            .push((AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_EDGE, true));

        assert_eq!(
            format!("{features}"),
            "gpu_activity_usage, gpu_energy_consumption, gpu_power_consumption, gpu_power_state_management, gpu_process_info, gpu_voltage, gpu_memory_usages::amdsmi_memory_type_t(0), gpu_temperatures::amdsmi_temperature_type_t(0)"
        );
    }

    // Test `is_supported` function with identified amdsmi status errors to disable a feature
    #[test]
    fn test_is_supported_errors() {
        let errors = [
            AmdStatus::AMDSMI_STATUS_NO_PERM,
            AmdStatus::AMDSMI_STATUS_NOT_SUPPORTED,
            AmdStatus::AMDSMI_STATUS_NOT_YET_IMPLEMENTED,
            AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA,
        ];
        for &err in &errors {
            let res = is_supported::<u32>(Err(AmdError(err))).unwrap();
            assert!(!res);
        }
    }

    // Test `is_supported` function with other amdsmi status errors
    #[test]
    fn test_is_supported_other_error() {
        let res = is_supported::<u32>(Err(AmdError(AmdStatus::AMDSMI_STATUS_UNKNOWN_ERROR)));
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), AmdStatus::AMDSMI_STATUS_UNKNOWN_ERROR);
    }

    // Test `has_any` function with no feature available
    #[test]
    fn test_has_any_all_false() {
        let features = OptionalFeatures {
            gpu_activity_usage: false,
            gpu_energy_consumption: false,
            gpu_power_consumption: false,
            gpu_power_state_management: false,
            gpu_process: false,
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
            gpu_process: is_supported(Ok(())).unwrap(),
            gpu_voltage: is_supported(Ok(())).unwrap(),
            gpu_memories_usage: vec![(AmdMemoryType::AMDSMI_MEM_TYPE_VRAM, true)],
            gpu_temperatures: vec![(AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_EDGE, true)],
        };
        assert!(features.has_any());
    }

    // Test `detect_on` function in success case
    #[test]
    fn test_detect_on_success() {
        let mut mock = MockProcessorHandle::new();

        mock.expect_device_activity().returning(|| Ok(MOCK_ACTIVITY));

        mock.expect_device_energy_consumption().returning(|| Ok(MOCK_ENERGY));

        mock.expect_device_power_consumption().returning(|| Ok(MOCK_POWER));

        mock.expect_device_power_managment().returning(|| Ok(true));
        mock.expect_device_process_list().returning(|| Ok(vec![mock_process()]));
        mock.expect_device_voltage().returning(|_, _| Ok(MOCK_VOLTAGE));

        mock.expect_device_memory_usage().returning(|mem_type| {
            MOCK_MEMORY
                .iter()
                .find(|(t, _)| *t == mem_type)
                .map(|(_, v)| Ok(*v))
                .unwrap_or(Err(AmdError(AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA)))
        });

        mock.expect_device_temperature().returning(|sensor, metric| {
            if metric != AmdTemperatureMetric::AMDSMI_TEMP_CURRENT {
                return Err(AmdError(AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA));
            }
            MOCK_TEMPERATURE
                .iter()
                .find(|(s, _)| *s == sensor)
                .map(|(_, v)| Ok(*v))
                .unwrap_or(Err(AmdError(AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA)))
        });

        let features = OptionalFeatures::detect_on(&mock).unwrap();
        assert_eq!(features.gpu_memories_usage.iter().filter(|(_, s)| *s).count(), 2);
        assert_eq!(features.gpu_temperatures.iter().filter(|(_, s)| *s).count(), 7);
    }

    // Test `with_detected_features` function in success case
    #[test]
    fn test_with_detected_features_success() {
        let mut mock = MockProcessorHandle::new();

        mock.expect_device_activity().returning(|| Ok(MOCK_ACTIVITY));

        mock.expect_device_energy_consumption()
            .returning(|| Err(AmdError(AmdStatus::AMDSMI_STATUS_NO_PERM)));
        mock.expect_device_power_consumption().returning(|| Ok(MOCK_POWER));
        mock.expect_device_power_managment().returning(|| Ok(true));
        mock.expect_device_process_list().returning(|| Ok(vec![mock_process()]));
        mock.expect_device_voltage()
            .returning(|_, _| Err(AmdError(AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA)));
        mock.expect_device_memory_usage()
            .returning(|_| Err(AmdError(AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA)));
        mock.expect_device_temperature()
            .returning(|_, _| Err(AmdError(AmdStatus::AMDSMI_STATUS_UNEXPECTED_DATA)));

        let (res, features) = OptionalFeatures::with_detected_features(&mock).unwrap();

        assert!(eq(res, &mock));
        assert!(features.has_any());
    }
}
