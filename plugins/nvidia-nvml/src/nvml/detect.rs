//! Detection of all GPU devices.

use anyhow::Context;

use super::NvmlLib;
use crate::nvml::{
    NvmlDevice,
    features::{DetectedDevice, OptionalFeatures},
};

/// Detected NVML devices.
pub struct Devices<D: NvmlDevice> {
    devices: Vec<Option<DetectedDevice<D>>>,
}

/// Statistics about the device detection.
pub struct DetectionStats {
    pub found_devices: usize,
    pub working_devices: usize,
}

/// What to do when there is an error with a specific device during the detection?
#[derive(Debug, PartialEq, Eq)]
pub enum DeviceFailureStrategy {
    /// Skip the device.
    /// The error is logged and the detection continues.
    Skip,

    /// Fail immediately.
    /// The detection is stopped and the error is returned.
    Fail,
}

impl<D: NvmlDevice> Devices<D> {
    /// Detects the GPUs that are available on the machine and stores them in a new `NvmlDevices` structure.
    pub fn detect<L: NvmlLib<Device = D>>(nvml: &L, on_failure: DeviceFailureStrategy) -> anyhow::Result<Devices<D>> {
        fn get_and_detect<L: NvmlLib>(nvml: &L, index: u32) -> anyhow::Result<(L::Device, OptionalFeatures)> {
            let device = nvml.device_by_index(index)?;
            let features = OptionalFeatures::detect_on(&device)
                .with_context(|| format!("could not detect the features available on device {device}"))?;
            Ok((device, features))
        }

        let count = nvml.device_count()?;
        let mut devices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let device = match get_and_detect(nvml, i) {
                Ok((gpu, features)) => {
                    if features.has_any() {
                        Some(DetectedDevice { features, inner: gpu })
                    } else {
                        log::warn!("Skipping GPU device {i} ({gpu}) because it supports no useful feature.");
                        None
                    }
                }
                Err(e) => {
                    match on_failure {
                        DeviceFailureStrategy::Skip => {
                            log::warn!("Skipping GPU device {i} because of error:\n{e:?}");
                            None
                        }
                        DeviceFailureStrategy::Fail => {
                            // don't skip, fail immediately
                            Err(e).context(format!("failed to inspect GPU device {i}"))?
                        }
                    }
                }
            };
            devices.push(device);
        }
        Ok(Devices { devices })
    }

    /// Gets and return status of GPU device detection on the operating system.
    pub fn detection_stats(&self) -> DetectionStats {
        let n_found = self.devices.len();
        let n_failed = self.devices.iter().filter(|d| d.is_none()).count();
        let n_ok = n_found - n_failed;
        DetectionStats {
            found_devices: n_found,
            working_devices: n_ok,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &DetectedDevice<D>> {
        self.devices.iter().flatten()
    }

    pub fn into_iter(self) -> impl Iterator<Item = DetectedDevice<D>> {
        self.devices.into_iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use nvml_wrapper::error::NvmlError;

    use crate::nvml::{MockNvmlDevice, MockNvmlLib};

    use super::*;

    #[test]
    fn detect_no_device() {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(0)).times(1);
        nvml.expect_device_by_index().never();

        let devices = Devices::detect(&nvml, DeviceFailureStrategy::Fail).unwrap();
        assert!(devices.devices.is_empty());
        assert_eq!(devices.detection_stats().found_devices, 0);
        assert_eq!(devices.detection_stats().working_devices, 0);
    }

    #[test]
    fn detect_ok() {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(1)).times(1);
        nvml.expect_device_by_index().returning(|i| {
            assert_eq!(i, 0, "invalid index");
            let mut device = MockNvmlDevice::new();
            device.expect_total_energy_consumption().returning(|| Ok(0)).times(1);
            device.expect_power_usage().returning(|| Ok(145)).times(1);
            device
                .expect_temperature()
                .returning(|_| Err(NvmlError::NotSupported))
                .times(1);
            device
                .expect_utilization_rates()
                .returning(|| Err(NvmlError::NotSupported))
                .times(1);
            device
                .expect_decoder_utilization()
                .returning(|| Err(NvmlError::NotSupported))
                .times(1);
            device
                .expect_encoder_utilization()
                .returning(|| Err(NvmlError::NotSupported))
                .times(1);
            device
                .expect_process_utilization_stats()
                .returning(|_| Err(NvmlError::NotSupported))
                .times(1);
            device
                .expect_running_compute_processes()
                .returning(|| Ok(Vec::new()))
                .times(1);
            device
                .expect_running_graphics_processes()
                .returning(|| Ok(Vec::new()))
                .times(1);
            Ok(device)
        });

        let devices = Devices::detect(&nvml, DeviceFailureStrategy::Fail).unwrap();
        assert_eq!(devices.devices.len(), 1);
        let _gpu0 = devices.devices[0]
            .as_ref()
            .expect("gpu0 should be detected without error");
        assert_eq!(devices.detection_stats().found_devices, 1);
        assert_eq!(devices.detection_stats().working_devices, 1);
    }

    #[test]
    fn fail_when_device_get_fails() {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(1)).times(1);
        nvml.expect_device_by_index()
            .returning(|_| Err(anyhow::Error::msg("test error")));

        let devices = Devices::detect(&nvml, DeviceFailureStrategy::Fail);
        assert!(devices.is_err()); // we cannot use expect_err because Devices does not implement Debug
    }

    #[test]
    fn skip_when_device_get_fails() {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(1)).times(1);
        nvml.expect_device_by_index()
            .returning(|_| Err(anyhow::Error::msg("test error")));

        let devices =
            Devices::detect(&nvml, DeviceFailureStrategy::Skip).expect("detect should not fail with the skip strategy");
        assert_eq!(devices.devices.len(), 1);
        assert_eq!(devices.iter().collect::<Vec<_>>().len(), 0);
        assert_eq!(devices.detection_stats().found_devices, 1);
        assert_eq!(devices.detection_stats().working_devices, 0);
    }

    #[test]
    fn fail_when_detect_fails() {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(1)).times(1);
        nvml.expect_device_by_index().returning(|i| {
            assert_eq!(i, 0, "invalid index");
            let mut device = MockNvmlDevice::new();
            device
                .expect_total_energy_consumption()
                .returning(|| Err(NvmlError::GpuLost))
                .times(1);
            Ok(device)
        });

        let devices = Devices::detect(&nvml, DeviceFailureStrategy::Fail);
        assert!(devices.is_err());
    }

    #[test]
    fn skip_when_detect_fails() {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(1)).times(1);
        nvml.expect_device_by_index().returning(|i| {
            assert_eq!(i, 0, "invalid index");
            let mut device = MockNvmlDevice::new();
            device
                .expect_total_energy_consumption()
                .returning(|| Err(NvmlError::GpuLost))
                .times(1);
            Ok(device)
        });

        let devices =
            Devices::detect(&nvml, DeviceFailureStrategy::Skip).expect("detect should not fail with the skip strategy");
        assert_eq!(devices.devices.len(), 1);
        assert_eq!(devices.iter().collect::<Vec<_>>().len(), 0);
        assert_eq!(devices.detection_stats().found_devices, 1);
        assert_eq!(devices.detection_stats().working_devices, 0);
    }
}
