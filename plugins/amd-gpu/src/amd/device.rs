use amdsmi::{amdsmi_get_gpu_device_bdf, amdsmi_get_processor_handles, amdsmi_get_socket_handles};
use std::ffi::c_void;

use super::error::AmdError;
use super::features::OptionalFeatures;

/// Detected AMD GPU devices via AMDSMI.
pub struct AmdDevices {
    pub devices: Vec<Option<ManagedDevice>>,
}

/// An AMD GPU device that has been probed for available features.
pub struct ManagedDevice {
    /// A pointer to the device.
    pub handle: *mut c_void,
    /// Status of the various features available or not on a device.
    pub features: OptionalFeatures,
    /// PCI bus ID of the device.
    pub bus_id: String,
}

/// Statistics about the device detection.
pub struct DetectionStats {
    pub found_devices: usize,
    pub working_devices: usize,
}

/// Wrapper to provide the required information about AMD GPU device
pub struct AmDeviceWrapper {
    handle: *mut c_void,
}

impl AmdDevices {
    /// Detects the GPUs that are available on the machine and stores them in a new `AmdDevices` structure.
    ///
    /// If `skip_failed_devices` is `true`, ignore inaccessible GPUs. Some fatal errors will still make the function return early with an error.
    /// If `skip_failed_devices` is `false`, returns an error on the first device that fails.
    pub fn detect(skip_failed_devices: bool) -> anyhow::Result<AmdDevices> {
        let socket_handles = amdsmi_get_socket_handles().map_err(AmdError)?;
        let mut devices = Vec::new();

        for socket_handle in socket_handles {
            // Get processor handles for each socket handle
            let handles = amdsmi_get_processor_handles(socket_handle).map_err(AmdError)?;
            for handle in handles {
                let device = match OptionalFeatures::with_detected_features(handle) {
                    Ok((gpu, features)) => {
                        let bus_id = (amdsmi_get_gpu_device_bdf(gpu).map_err(AmdError)?).to_string();
                        if features.has_any() {
                            // Extract the device pointer because we will manage the lifetimes ourselves.
                            let d = ManagedDevice {
                                handle: gpu,
                                features,
                                bus_id,
                            };
                            Some(d)
                        } else {
                            let bus_id = match amdsmi_get_gpu_device_bdf(gpu) {
                                Ok(id) => format!("PCI bus {id}"),
                                Err(e) => format!("failed to get bus ID: {e}"),
                            };
                            log::warn!("Skipping GPU device ({bus_id}) because it supports no useful feature.");
                            None
                        }
                    }
                    Err(e) => {
                        if skip_failed_devices {
                            log::warn!("Skipping GPU device because of error:\n{e:?}");
                            None
                        } else {
                            // don't skip, fail immediately
                            Err(AmdError(e))?
                        }
                    }
                };
                devices.push(device);
            }
        }
        Ok(AmdDevices { devices })
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
}

impl AmDeviceWrapper {
    pub fn new(handle: *mut c_void) -> Self {
        AmDeviceWrapper { handle }
    }

    /// Get the handle device pointer to identify an AMD GPU device
    pub fn as_ptr(&self) -> *mut c_void {
        self.handle
    }
}

impl ManagedDevice {
    pub fn as_wrapper(&self) -> AmDeviceWrapper {
        AmDeviceWrapper::new(self.handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    // Test `as_wrapper` function function for managed device
    #[test]
    fn test_managed_device_as_wrapper() {
        let ptr = ptr::null_mut();
        let features = OptionalFeatures::default();

        let device = ManagedDevice {
            handle: ptr,
            features,
            bus_id: String::from("0000:00:00.0"),
        };

        let res = device.as_wrapper();
        assert_eq!(res.as_ptr(), ptr);
    }

    // Test `detection_stats` function
    #[test]
    fn test_detection_stats() {
        let devices = vec![
            Some(ManagedDevice {
                handle: ptr::null_mut(),
                features: OptionalFeatures::default(),
                bus_id: String::from("0000:00:00.0"),
            }),
            None,
            Some(ManagedDevice {
                handle: ptr::null_mut(),
                features: OptionalFeatures::default(),
                bus_id: String::from("0000:01:00.0"),
            }),
            None,
        ];

        let amd_devices = AmdDevices { devices };
        let res = amd_devices.detection_stats();

        assert_eq!(res.found_devices, 4);
        assert_eq!(res.working_devices, 2);
    }
}
