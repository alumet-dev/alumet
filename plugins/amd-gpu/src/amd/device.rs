use std::collections::HashMap;

use super::features::OptionalFeatures;
use crate::{get_amdsmi_instance, interface::ProcessorHandle};

/// Detected AMD GPU devices via AMDSMI.
pub struct AmdGpuDevices<'a> {
    /// Counter of detection errors on AMD GPU device.
    pub failure_count: usize,
    /// Set of parameters that defines an AMD GPU device.
    pub devices: Vec<ManagedDevice<'a>>,
}

/// An AMD GPU device that has been probed for available features.
pub struct ManagedDevice<'a> {
    /// A pointer to the device.
    pub handle: ProcessorHandle<'a>,
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

impl<'a> AmdGpuDevices<'a> {
    /// Detects all AMD GPUs and returns an AmdGpuDevices object.
    pub fn detect(skip_failed_devices: bool) -> anyhow::Result<AmdGpuDevices<'a>> {
        // Get our global AMD SMI instance
        let amdsmi = get_amdsmi_instance();

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

        let mut devices: Vec<ManagedDevice<'a>> = devices.into_values().collect();
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
    use super::*;
    use crate::bindings::{
        amdsmi_processor_handle, amdsmi_status_t, amdsmi_status_t_AMDSMI_STATUS_INVAL,
        amdsmi_status_t_AMDSMI_STATUS_SUCCESS,
    };

    use crate::tests::ffi_mock::{
        ffi_mocks_processor_handles::set_mock_processor_handles, ffi_mocks_socket_handles::set_mock_socket_handles,
        ffi_mocks_uuid::set_mock_uuid,
    };

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    // Test `detect` function for 1 GPU in case we skip failed devices
    #[test]
    fn test_detect_skip_failed_devices() {
        set_mock_socket_handles(1, SUCCESS, SUCCESS);
        set_mock_processor_handles(vec![0 as amdsmi_processor_handle], SUCCESS, SUCCESS);
        set_mock_uuid(vec![0], ERROR);

        let res = AmdGpuDevices::detect(true).unwrap();
        assert_eq!(res.failure_count, 1);
        assert_eq!(res.devices.len(), 0);
    }

    // Test `detect` function for in case we failed to skip
    #[test]
    fn test_detect_fail_skip() {
        set_mock_socket_handles(1, SUCCESS, SUCCESS);
        set_mock_processor_handles(vec![0 as amdsmi_processor_handle], SUCCESS, SUCCESS);
        set_mock_uuid(vec![0], ERROR);

        let res = AmdGpuDevices::detect(false);
        assert!(res.is_err());
    }
}
