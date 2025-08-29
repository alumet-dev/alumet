use amdsmi::{amdsmi_get_gpu_device_bdf, amdsmi_get_processor_handles, amdsmi_get_socket_handles};
use anyhow::Context;
use std::ffi::c_void;

use super::error::AmdError;
use super::features::OptionalFeatures;

/// Detected AMD GPU devices via AMDSMI.
#[derive(Debug)]
pub struct AmdGpuDevices {
    pub devices: Vec<Option<ManagedDevice>>,
}

/// An AMD GPU device that has been probed for available features.
#[derive(Debug)]
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

impl AmdGpuDevices {
    /// Detects the GPUs that are available on the machine and stores them in a new `AmdDevices` structure.
    ///
    /// If `skip_failed_devices` is `true`, ignore inaccessible GPUs. Some fatal errors will still make the function return early with an error.
    /// If `skip_failed_devices` is `false`, returns an error on the first device that fails.
    pub fn detect(skip_failed_devices: bool) -> anyhow::Result<AmdGpuDevices> {
        let socket_handles = amdsmi_get_socket_handles().map_err(AmdError)?;
        let mut devices = Vec::new();

        for socket_handle in socket_handles {
            // Get processor handles for each socket handle
            let handles = amdsmi_get_processor_handles(socket_handle).map_err(AmdError)?;
            for handle in handles {
                let device = match OptionalFeatures::with_detected_features(handle) {
                    Ok((gpu, features)) => {
                        let bus_id = amdsmi_get_gpu_device_bdf(gpu)
                            .map_err(AmdError)
                            .context("failed to get the bus ID of device")?;

                        if features.has_any() {
                            // Extract the device pointer because we will manage the lifetimes ourselves.
                            Some(ManagedDevice {
                                handle: gpu,
                                features,
                                bus_id: bus_id.to_string(),
                            })
                        } else {
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
        Ok(AmdGpuDevices { devices })
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
