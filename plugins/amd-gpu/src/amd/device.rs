use amd_smi_lib_sys::bindings::*;
use anyhow::Context;
use std::collections::HashMap;

use super::error::AmdError;
use super::features::OptionalFeatures;
use super::utils::*;

/// Detected AMD GPU devices via AMDSMI.
#[derive(Debug)]
pub struct AmdGpuDevices {
    /// Counter of detection errors on AMD GPU device.
    pub failure_count: usize,
    /// Set of parameters that defines an AMD GPU device.
    pub devices: Vec<ManagedDevice>,
}

/// An AMD GPU device that has been probed for available features.
#[derive(Debug)]
pub struct ManagedDevice {
    /// A pointer to the device.
    pub handle: amdsmi_processor_handle,
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
    /// Detects the GPUs that are available on the machine and stores them in a new `AmdDevices` structure.
    ///
    /// If `skip_failed_devices` is `true`, ignore inaccessible GPUs. Some fatal errors will still make the function return early with an error.
    /// If `skip_failed_devices` is `false`, returns an error on the first device that fails.
    pub fn detect(skip_failed_devices: bool) -> anyhow::Result<AmdGpuDevices> {
        let mut devices: HashMap<String, ManagedDevice> = HashMap::new();
        let mut failure_count = 0;

        for socket_handle in get_socket_handles().map_err(AmdError)? {
            // Get processor handles for each socket handle
            for handle in get_processor_handles(socket_handle).map_err(AmdError)? {
                match OptionalFeatures::with_detected_features(handle) {
                    Ok((gpu, features)) => {
                        let bus_id = get_device_uuid(gpu)
                            .map_err(AmdError)
                            .context("failed to get the bus ID of device")?;

                        if features.has_any() {
                            // Extract the device pointer because we will manage the lifetimes ourselves.
                            let device = ManagedDevice {
                                handle: gpu,
                                features,
                                bus_id: bus_id.to_string(),
                            };
                            devices.insert(device.bus_id.to_string(), device);
                        } else {
                            log::warn!("Skipping GPU device ({bus_id}) because it supports no useful feature.");
                            failure_count += 1;
                        }
                    }
                    Err(e) => {
                        if skip_failed_devices {
                            failure_count += 1;
                            log::warn!("Skipping GPU device because of error:\n{e:?}");
                        } else {
                            // don't skip, fail immediately
                            Err(AmdError(e))?
                        }
                    }
                };
            }
        }

        let mut devices: Vec<ManagedDevice> = devices.into_values().collect();
        devices.sort_by_key(|device| device.bus_id.clone());

        Ok(AmdGpuDevices { devices, failure_count })
    }

    /// Gets and return status of GPU device detection on the operating system.
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
mod tests {
    use super::*;

    // Test `detect_fails` function with skip_failed_devices = false where device feature detection fails
    #[test]
    fn test_detect_fails_without_skip() {
        let result = AmdGpuDevices::detect(false);
        assert!(result.is_err());
    }
}
