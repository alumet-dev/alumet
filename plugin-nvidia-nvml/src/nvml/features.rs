use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, error::NvmlError, Device};
use std::fmt::Display;

/// Indicates which version of a NVML function is available on a given device.
#[derive(Debug, PartialEq, Eq)]
pub enum AvailableVersion {
    Latest,
    V2,
    None,
}

/// Indicates which features are available on a given NVML device.
#[derive(Debug)]
pub struct OptionalFeatures {
    /// Total electric energy consumed by GPU.
    pub total_energy_consumption: bool,
    /// Electric energy consumption measured at a given time.
    pub instant_power: bool,
    /// GPU temperature.
    pub temperature_gpu: bool,
    /// GPU rate utilization.
    pub major_utilization: bool,
    /// GPU video decoding property.
    pub decoder_utilization: bool,
    /// GPU video encoding property.
    pub encoder_utilization: bool,
    /// Utilization stats for relevant currently running processes.
    pub process_utilization_stats: bool,
    /// Relevant currently running computing processes data.
    pub running_compute_processes: AvailableVersion,
    /// Relevant currently running graphical processes data.
    pub running_graphics_processes: AvailableVersion,
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(device: &Device) -> Result<Self, NvmlError> {
        Ok(Self {
            total_energy_consumption: is_supported(device.total_energy_consumption())?,
            instant_power: is_supported(device.power_usage())?,
            temperature_gpu: is_supported(device.temperature(TemperatureSensor::Gpu))?,
            major_utilization: is_supported(device.utilization_rates())?,
            decoder_utilization: is_supported(device.decoder_utilization())?,
            encoder_utilization: is_supported(device.encoder_utilization())?,
            process_utilization_stats: is_supported(device.process_utilization_stats(0))?,
            running_compute_processes: check_running_compute_processes(device)?,
            running_graphics_processes: check_running_graphics_processes(device)?,
        })
    }

    pub fn with_detected_features<'a>(device: Device<'a>) -> Result<(Device<'a>, Self), NvmlError> {
        Self::detect_on(&device).map(|features| (device, features))
    }

    pub fn has_any(&self) -> bool {
        self.total_energy_consumption
            || self.instant_power
            || self.major_utilization
            || self.decoder_utilization
            || self.encoder_utilization
            || self.temperature_gpu
            || self.running_compute_processes != AvailableVersion::None
            || self.running_graphics_processes != AvailableVersion::None
    }
}

impl Display for OptionalFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut available = Vec::new();
        if self.total_energy_consumption {
            available.push("total_energy_consumption");
        }
        if self.instant_power {
            available.push("instant_power");
        }
        if self.major_utilization {
            available.push("major_utilization");
        }
        if self.decoder_utilization {
            available.push("decoder_utilization");
        }
        if self.encoder_utilization {
            available.push("encoder_utilization");
        }
        if self.process_utilization_stats {
            available.push("process_utilization_stats");
        }
        if self.temperature_gpu {
            available.push("temperature_gpu");
        }
        match self.running_compute_processes {
            AvailableVersion::Latest => available.push("running_compute_processes(latest)"),
            AvailableVersion::V2 => available.push("running_compute_processes(v2)"),
            AvailableVersion::None => (),
        };
        match self.running_graphics_processes {
            AvailableVersion::Latest => available.push("running_graphics_processes(latest)"),
            AvailableVersion::V2 => available.push("running_graphics_processes(v2)"),
            AvailableVersion::None => (),
        };
        write!(f, "{}", available.join(", "))
    }
}

/// Checks which version of `running_compute_processes` is available (if any) on this NVML device.
fn check_running_compute_processes(device: &Device) -> Result<AvailableVersion, NvmlError> {
    match device.running_compute_processes() {
        Ok(_) => Ok(AvailableVersion::Latest),
        Err(NvmlError::FailedToLoadSymbol(_)) => match device.running_compute_processes_v2() {
            Ok(_) => Ok(AvailableVersion::V2),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// Checks which version of `running_graphic_processes` is available (if any) on this NVML device.
fn check_running_graphics_processes(device: &Device) -> Result<AvailableVersion, NvmlError> {
    match device.running_graphics_processes() {
        Ok(_) => Ok(AvailableVersion::Latest),
        Err(NvmlError::FailedToLoadSymbol(_)) => match device.running_graphics_processes_v2() {
            Ok(_) => Ok(AvailableVersion::V2),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// Checks if a feature is supported by the available GPU by inspecting the return type of an NVML function.
///
/// # Example
/// ```ignore
/// let device: &nvml_wrapper::Device = todo!();
/// let power_available = features::is_supported(device.power_usage()).expect("test");
/// ```
fn is_supported<T>(res: Result<T, NvmlError>) -> Result<bool, NvmlError> {
    match res {
        Ok(_) => Ok(true),
        Err(NvmlError::NotSupported) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvml_wrapper::{error::NvmlError, Nvml};

    // Test `fmt` function with all correctly implemented data in features
    #[ignore = "NO GPU"]
    #[test]
    fn test_fmt_with_all_valid_data() {
        let features = OptionalFeatures {
            total_energy_consumption: true,
            instant_power: true,
            major_utilization: true,
            decoder_utilization: true,
            encoder_utilization: true,
            process_utilization_stats: true,
            temperature_gpu: true,
            running_compute_processes: AvailableVersion::Latest,
            running_graphics_processes: AvailableVersion::Latest,
        };
        assert_eq!(
            format!("{}", features),
            "total_energy_consumption, instant_power, major_utilization, decoder_utilization, encoder_utilization, process_utilization_stats, temperature_gpu, running_compute_processes(latest), running_graphics_processes(latest)"
        );
    }

    // Test `fmt` function with some implemented data in features
    #[ignore = "NO GPU"]
    #[test]
    fn test_fmt_with_data() {
        let features = OptionalFeatures {
            total_energy_consumption: false,
            instant_power: false,
            major_utilization: false,
            decoder_utilization: true,
            encoder_utilization: true,
            process_utilization_stats: false,
            temperature_gpu: true,
            running_compute_processes: AvailableVersion::None,
            running_graphics_processes: AvailableVersion::None,
        };
        assert_eq!(
            format!("{}", features),
            "decoder_utilization, encoder_utilization, temperature_gpu"
        );
    }

    // Test `fmt` function with with v2 running processes implemented data in feature
    #[ignore = "NO GPU"]
    #[test]
    fn test_fmt_with_v2_running_processes() {
        let features = OptionalFeatures {
            total_energy_consumption: false,
            instant_power: false,
            major_utilization: false,
            decoder_utilization: false,
            encoder_utilization: false,
            process_utilization_stats: false,
            temperature_gpu: false,
            running_compute_processes: AvailableVersion::V2,
            running_graphics_processes: AvailableVersion::V2,
        };
        assert_eq!(
            format!("{}", features),
            "running_compute_processes(v2), running_graphics_processes(v2)"
        );
    }

    // Test `has_any` function to check existence of a real device
    #[ignore = "NO GPU"]
    #[test]
    fn test_has_any() {
        let nvml = Nvml::init().expect("Initialize NVML lib");
        let device = nvml.device_by_index(0).expect("Device recognizing");
        let features = OptionalFeatures::detect_on(&device).expect("Detect features");
        assert!(features.has_any());
    }

    // Test `has_any` function with no implemented data in features
    #[ignore = "NO GPU"]
    #[test]
    fn test_has_any_no_data() {
        let features = OptionalFeatures {
            total_energy_consumption: false,
            instant_power: false,
            major_utilization: false,
            decoder_utilization: false,
            encoder_utilization: false,
            process_utilization_stats: false,
            temperature_gpu: false,
            running_compute_processes: AvailableVersion::None,
            running_graphics_processes: AvailableVersion::None,
        };
        assert!(!features.has_any());
    }

    // Test `is_supported` function in successfully and failure cases
    #[ignore = "NO GPU"]
    #[test]
    fn test_is_supported() {
        let result: Result<(), NvmlError> = Ok(());
        assert!(matches!(is_supported(result), Ok(true)));

        let result: Result<(), NvmlError> = Err(NvmlError::NotSupported);
        assert!(matches!(is_supported(result), Ok(false)));

        let error = NvmlError::Unknown;
        let result: Result<(), NvmlError> = Err(error);
        let avail = is_supported(result);

        match avail {
            Err(e) => assert!(matches!(e, NvmlError::Unknown)),
            _ => panic!("Expected error {:?}", avail),
        }
    }

    // Test `check_running_compute_processes` function in successfully case
    #[ignore = "NO GPU"]
    #[test]
    fn test_check_running_compute_processes() {
        let nvml = Nvml::init().expect("Initialize NVML lib");
        let device = nvml.device_by_index(0).expect("Device recognizing");
        let result = check_running_compute_processes(&device);

        match result {
            Ok(version) => {
                assert_eq!(version, AvailableVersion::Latest);
            }
            Err(NvmlError::FailedToLoadSymbol(_)) => {
                assert!(true);
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    // Test `check_running_graphics_processes` function in successfully case
    #[ignore = "NO GPU"]
    #[test]
    fn test_check_running_graphics_processes() {
        let nvml = Nvml::init().expect("Initialize NVML lib");
        let device = nvml.device_by_index(0).expect("Device recognizing");
        let result = check_running_graphics_processes(&device);

        match result {
            Ok(version) => {
                assert_eq!(version, AvailableVersion::Latest);
            }
            Err(NvmlError::FailedToLoadSymbol(_)) => {
                assert!(true);
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }
}
