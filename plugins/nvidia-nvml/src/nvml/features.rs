use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, error::NvmlError};
use std::fmt::Display;

use super::{NvmlDevice, NvmlResult};

pub struct DetectedDevice<D: NvmlDevice> {
    /// Status of the optional features: which feature is available on this device?
    pub features: OptionalFeatures,

    pub inner: D,
}

/// Indicates which version of a NVML function is available on a given device.
#[derive(Debug, PartialEq, Eq)]
pub enum AvailableVersion {
    Latest,
    V2,
    None,
}

/// Indicates which features are available on a given NVML device.
#[derive(Debug, PartialEq)]
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
    pub fn detect_on(device: &impl NvmlDevice) -> NvmlResult<Self> {
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
fn check_running_compute_processes(device: &impl NvmlDevice) -> Result<AvailableVersion, NvmlError> {
    detect_biversion(
        device,
        |d| d.running_compute_processes(),
        |d| d.running_compute_processes_v2(),
    )
}

/// Checks which version of `running_graphic_processes` is available (if any) on this NVML device.
fn check_running_graphics_processes(device: &impl NvmlDevice) -> Result<AvailableVersion, NvmlError> {
    detect_biversion(
        device,
        |d| d.running_graphics_processes(),
        |d| d.running_graphics_processes_v2(),
    )
}

fn detect_biversion<D: NvmlDevice, R1, R2>(
    device: &D,
    v3_latest: impl FnOnce(&D) -> NvmlResult<R1>,
    v2: impl FnOnce(&D) -> NvmlResult<R2>,
) -> NvmlResult<AvailableVersion> {
    match v3_latest(device) {
        Ok(_) => Ok(AvailableVersion::Latest),
        Err(NvmlError::FailedToLoadSymbol(_) | NvmlError::NotSupported) => match v2(device) {
            Ok(_) => Ok(AvailableVersion::V2),
            Err(NvmlError::FailedToLoadSymbol(_) | NvmlError::NotSupported) => Ok(AvailableVersion::None),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// Checks if a feature is supported by the available GPU by inspecting the return type of an NVML function.
///
/// # Example
/// ```ignore
/// let device: &nvml_wrapper::ManagedDevice = todo!();
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
    use crate::nvml::MockNvmlDevice;

    use super::{AvailableVersion, OptionalFeatures};
    use nvml_wrapper::error::NvmlError;
    use pretty_assertions::assert_eq;

    #[test]
    fn features_fmt_all() {
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

    #[test]
    fn features_fmt_some() {
        let features = OptionalFeatures {
            total_energy_consumption: true,
            instant_power: false,
            major_utilization: true,
            decoder_utilization: true,
            encoder_utilization: true,
            process_utilization_stats: false,
            temperature_gpu: false,
            running_compute_processes: AvailableVersion::V2,
            running_graphics_processes: AvailableVersion::None,
        };
        assert_eq!(
            format!("{}", features),
            "total_energy_consumption, major_utilization, decoder_utilization, encoder_utilization, running_compute_processes(v2)"
        );
    }

    /// Test `has_any` function with no available features
    #[test]
    fn has_any_no_features() {
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

    #[test]
    pub fn detect_some() {
        let mut device = MockNvmlDevice::new();
        device.expect_total_energy_consumption().returning(|| Ok(0));
        device.expect_power_usage().returning(|| Ok(145));
        device.expect_temperature().returning(|_| Err(NvmlError::NotSupported));
        device
            .expect_utilization_rates()
            .returning(|| Err(NvmlError::NotSupported));
        device
            .expect_decoder_utilization()
            .returning(|| Err(NvmlError::NotSupported));
        device
            .expect_encoder_utilization()
            .returning(|| Err(NvmlError::NotSupported));
        device
            .expect_process_utilization_stats()
            .returning(|_| Err(NvmlError::NotSupported));

        // compute processes: no v3, but v2
        device.expect_running_compute_processes().returning(|| {
            Err(NvmlError::FailedToLoadSymbol(String::from(
                "nvmlDeviceGetComputeRunningProcesses_v3",
            )))
        });
        device.expect_running_compute_processes_v2().returning(|| {
            Ok(Vec::new()) // no processes
        });

        // graphics processes: not supported
        device
            .expect_running_graphics_processes()
            .returning(|| {
                Err(NvmlError::FailedToLoadSymbol(String::from(
                    "nvmlDeviceGetGraphicsRunningProcesses_v3",
                )))
            })
            .times(1);
        device
            .expect_running_graphics_processes_v2()
            .returning(|| {
                Err(NvmlError::FailedToLoadSymbol(String::from(
                    "nvmlDeviceGetGraphicsRunningProcesses_v2",
                )))
            })
            .times(1);

        let features = OptionalFeatures::detect_on(&device).expect("detection failed");
        assert_eq!(
            features,
            OptionalFeatures {
                total_energy_consumption: true,
                instant_power: true,
                temperature_gpu: false,
                major_utilization: false,
                decoder_utilization: false,
                encoder_utilization: false,
                process_utilization_stats: false,
                running_compute_processes: AvailableVersion::V2,
                running_graphics_processes: AvailableVersion::None
            }
        );

        // v3 is now available
        device
            .expect_running_graphics_processes()
            .returning(|| Ok(Vec::new()))
            .times(1);
        device.expect_running_graphics_processes_v2().never();
        let features = OptionalFeatures::detect_on(&device).expect("detection failed");
        assert_eq!(
            features,
            OptionalFeatures {
                total_energy_consumption: true,
                instant_power: true,
                temperature_gpu: false,
                major_utilization: false,
                decoder_utilization: false,
                encoder_utilization: false,
                process_utilization_stats: false,
                running_compute_processes: AvailableVersion::V2,
                running_graphics_processes: AvailableVersion::Latest
            }
        );

        // failure
        device
            .expect_running_graphics_processes()
            .returning(|| Err(NvmlError::GpuLost))
            .times(1);
        OptionalFeatures::detect_on(&device).expect_err("should fail");
    }
}
