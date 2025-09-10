use anyhow::Context;
use rocm_smi_lib::RocmSmi;

use super::error::AmdError;
use super::features::OptionalFeatures;

/// Detected AMD GPU devices via ROCM-SMI.
pub struct AmdGpuDevices {
    /// Set of parameters that defines an AMD GPU device.
    pub devices: Vec<Option<ManagedDevice>>,
}

/// An AMD GPU device that has been probed for available features.
pub struct ManagedDevice {
    /// A pointer to the device.
    pub identifier: u32,
    /// Status of the various features available or not on a device.
    pub features: OptionalFeatures,
    /// Unique identifier of the device.
    pub bus_id: u64,
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
        let mut rocm = RocmSmi::init().map_err(AmdError)?;

        let count = rocm.get_device_count();
        let mut devices = Vec::with_capacity(count as usize);

        for id in 0..count {
            let device = match OptionalFeatures::detect_on(id) {
                Ok((features, gpu)) => {
                    let gpu_id = rocm
                        .get_device_identifiers(id)
                        .map_err(AmdError)
                        .context("failed to get the unique ID of device")?;

                    if features.has_any() {
                        // Extract the device pointer because we will manage the lifetimes ourselves.
                        let managed_device = ManagedDevice {
                            identifier: gpu,
                            features,
                            bus_id: gpu_id.unique_id.unwrap(),
                        };

                        Some(managed_device)
                    } else {
                        log::warn!("Skipping GPU device {id} because it supports no useful feature.");
                        None
                    }
                }
                Err(e) => {
                    if skip_failed_devices {
                        log::warn!("Skipping GPU device {id} because of error:\n{e:?}");
                        None
                    } else {
                        // Don't skip, fail immediately
                        Err(AmdError(e))?
                    }
                }
            };
            devices.push(device);
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
