use std::collections::HashMap;

use super::features::OptionalFeatures;
use crate::interface::{AmdSmiRef, MockableProcessorHandle, ProcessorProvider};

/// Detected AMD GPU devices via AMDSMI.
pub struct AmdGpuDevices {
    /// Counter of detection errors on AMD GPU device.
    pub failure_count: usize,
    /// Set of parameters that defines an AMD GPU device.
    pub devices: Vec<ManagedDevice>,
}

/// An AMD GPU device that has been probed for available features.
pub struct ManagedDevice {
    /// A pointer to the device.
    pub handle: MockableProcessorHandle,
    /// Status of the various features available or not on a device.
    pub features: OptionalFeatures,
    /// PCI bus ID of the device.
    pub bus_id: String,
}

/// Statistics about the device detection.
pub struct DetectionStats {
    /// Detected AMD GPU device.
    pub found_devices: usize,
    /// Detected working features on the AMD GPU device.
    pub working_devices: usize,
}

impl AmdGpuDevices {
    /// Detects all AMD GPUs and returns an AmdGpuDevices object.
    pub fn detect(amdsmi: &AmdSmiRef, skip_failed_devices: bool) -> anyhow::Result<Self> {
        // Get our global AMD SMI instance
        let mut devices = HashMap::new();
        let mut failure_count = 0;

        // Iterate over sockets
        for socket in amdsmi.get_socket_handles()? {
            // Iterate over processor handles
            for processor in socket.get_processor_handles()? {
                // Detect available features
                match OptionalFeatures::with_detected_features(&processor) {
                    Ok((_, features)) => {
                        let bus_id = processor.get_device_uuid()?.to_string();

                        if features.has_any() {
                            devices.insert(
                                bus_id.clone(),
                                ManagedDevice {
                                    handle: processor,
                                    features,
                                    bus_id,
                                },
                            );
                        } else {
                            log::warn!("Skipping GPU device because it supports no useful feature.");
                            failure_count += 1;
                        }
                    }

                    Err(e) => {
                        if skip_failed_devices {
                            failure_count += 1;
                            log::warn!("Skipping GPU device because of error:\n{e:?}");
                        } else {
                            return Err(crate::AmdError(e).into());
                        }
                    }
                }
            }
        }

        let mut devices: Vec<ManagedDevice> = devices.into_values().collect();
        devices.sort_by_key(|device| device.bus_id.clone());

        Ok(AmdGpuDevices { devices, failure_count })
    }

    /// Gets statistics about device detection.
    pub fn detection_stats(&self) -> DetectionStats {
        let n_failed = self.failure_count;
        let n_ok = self.devices.len();
        let n_found = n_ok + n_failed;

        DetectionStats {
            found_devices: n_found,
            working_devices: n_ok,
        }
    }
}

#[cfg(test)]
mod tests_device {
    use super::AmdSmiRef;
    use crate::{
        amd::{
            device::AmdGpuDevices,
            utils::{METRIC_TEMP, UNEXPECTED_DATA},
        },
        interface::{AmdError, MockProcessorProvider, MockSocketHandle},
        tests::mocks::tests_mocks::{
            MOCK_ACTIVITY, MOCK_ENERGY, MOCK_ENERGY_RESOLUTION, MOCK_MEMORY, MOCK_POWER, MOCK_PROCESS,
            MOCK_TEMPERATURE, MOCK_UUID, MOCK_VOLTAGE,
        },
    };

    const TIMESTAMP: u64 = 1712024507665;
    const UUID: &str = "a4ff740f-0000-1000-80ea-e05c945bb3b2";

    // Test `detection_stats` function with no GPUs detected
    #[test]
    fn test_detection_stats_no_devices() {
        let devices = AmdGpuDevices {
            devices: vec![],
            failure_count: 0,
        };

        let stats = devices.detection_stats();
        assert_eq!(stats.found_devices, 0);
        assert_eq!(stats.working_devices, 0);
    }

    // Test `detection_stats` function with GPUs detected but no working device
    #[test]
    fn test_detection_stats_failed() {
        let devices = AmdGpuDevices {
            devices: vec![],
            failure_count: 5,
        };

        let stats = devices.detection_stats();
        assert_eq!(stats.found_devices, 5);
        assert_eq!(stats.working_devices, 0);
    }

    // Test `detect` function in success case with valid GPUs and metrics
    #[test]
    fn test_detect_success() {
        let mut mock_init = AmdSmiRef::new();
        let mut mock_socket = MockSocketHandle::new();
        let mut mock_processor = MockProcessorProvider::new();

        mock_processor
            .expect_get_device_uuid()
            .returning(|| Ok(MOCK_UUID.to_owned()));

        mock_processor
            .expect_get_device_activity()
            .returning(|| Ok(MOCK_ACTIVITY));

        mock_processor
            .expect_get_device_energy_consumption()
            .returning(|| Ok((MOCK_ENERGY, MOCK_ENERGY_RESOLUTION, TIMESTAMP)));

        mock_processor
            .expect_get_device_power_consumption()
            .returning(|| Ok(MOCK_POWER));

        mock_processor
            .expect_get_device_power_managment()
            .returning(|| Ok(true));
        mock_processor
            .expect_get_device_process_list()
            .returning(|| Ok(vec![MOCK_PROCESS]));
        mock_processor
            .expect_get_device_voltage()
            .returning(|_, _| Ok(MOCK_VOLTAGE));

        mock_processor.expect_get_device_memory_usage().returning(|mem_type| {
            MOCK_MEMORY
                .iter()
                .find(|(t, _)| *t == mem_type)
                .map(|(_, v)| Ok(*v))
                .unwrap_or(Err(AmdError(UNEXPECTED_DATA)))
        });

        mock_processor
            .expect_get_device_temperature()
            .returning(|sensor, metric| {
                if metric != METRIC_TEMP {
                    return Err(AmdError(UNEXPECTED_DATA));
                }
                MOCK_TEMPERATURE
                    .iter()
                    .find(|(s, _)| *s == sensor)
                    .map(|(_, v)| Ok(*v))
                    .unwrap_or(Err(AmdError(UNEXPECTED_DATA)))
            });

        mock_socket
            .expect_get_processor_handles()
            .return_once(move || Ok(vec![mock_processor]));

        mock_init
            .expect_get_socket_handles()
            .return_once(move || Ok(vec![mock_socket]));

        let res = AmdGpuDevices::detect(&mock_init, false).expect("should work");
        assert_eq!(res.failure_count, 0);
    }

    // Test `detect` function for a GPU with no features available
    #[test]
    fn test_detect_error_skipped() {
        let mut mock_init = AmdSmiRef::new();
        let mut mock_socket = MockSocketHandle::new();
        let mut mock_processor = MockProcessorProvider::new();

        mock_processor
            .expect_get_device_uuid()
            .returning(|| Ok(UUID.to_owned()));

        mock_processor
            .expect_get_device_activity()
            .returning(|| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_energy_consumption()
            .returning(|| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_power_consumption()
            .returning(|| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_power_managment()
            .returning(|| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_process_list()
            .returning(|| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_voltage()
            .returning(|_, _| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_memory_usage()
            .returning(|_| Err(AmdError(UNEXPECTED_DATA)));
        mock_processor
            .expect_get_device_temperature()
            .returning(|_, _| Err(AmdError(UNEXPECTED_DATA)));

        mock_socket
            .expect_get_processor_handles()
            .return_once(move || Ok(vec![mock_processor]));

        mock_init
            .expect_get_socket_handles()
            .return_once(move || Ok(vec![mock_socket]));

        let res = AmdGpuDevices::detect(&mock_init, true).expect("error should be skipped");

        assert_eq!(res.failure_count, 1);
        assert!(res.devices.is_empty());
    }
}
