use anyhow::Context;
use nvml_wrapper::{Device, Nvml, error::NvmlError};
use nvml_wrapper_sys::bindings::nvmlDevice_t;
use std::sync::Arc;

use super::features::OptionalFeatures;

/// Detected NVML devices.
pub struct NvmlDevices {
    pub devices: Vec<Option<ManagedDevice>>,
}

/// An NVML device that has been probed for available features.
pub struct ManagedDevice {
    /// The library must be initialized and alive (not dropped), otherwise the handle will no longer work.
    /// We use an Arc to ensure this in a way that's more easy for us than a lifetime on the struct.
    pub lib: Arc<Nvml>,
    /// A pointer to the device, as returned by NVML.
    pub handle: nvmlDevice_t,
    /// Status of the optional features: which feature is available on this device?
    pub features: OptionalFeatures,
    /// PCI bus ID of the device.
    pub bus_id: String,
}

/// Statistics about the device detection.
pub struct DetectionStats {
    pub found_devices: usize,
    pub failed_devices: usize,
    pub working_devices: usize,
}

impl NvmlDevices {
    /// Detects the GPUs that are available on the machine and stores them in a new `NvmlDevices` structure.
    ///
    /// If `skip_failed_devices` is `true`, ignore inaccessible GPUs. Some fatal errors will still make the function return early with an error.
    /// If `skip_failed_devices` is `false`, returns an error on the first device that fails.
    pub fn detect(skip_failed_devices: bool) -> anyhow::Result<NvmlDevices> {
        let nvml = Arc::new(Nvml::init().context(
            "NVML initialization failed, please check your driver (do you have a dekstop/server NVidia GPU?",
        )?);

        let count = nvml.device_count()?;
        let mut devices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let device = match nvml
                .device_by_index(i)
                .and_then(OptionalFeatures::with_detected_features)
            {
                Ok((gpu, features)) => {
                    let pci_info = gpu.pci_info();
                    if features.has_any() {
                        // Extract the device pointer because we will manage the lifetimes ourselves.
                        let handle = unsafe { gpu.handle() };
                        let lib = nvml.clone();
                        let bus_id = pci_info?.bus_id;
                        let d = ManagedDevice {
                            lib,
                            handle,
                            features,
                            bus_id,
                        };
                        Some(d)
                    } else {
                        let bus_id = match pci_info {
                            Ok(pci) => format!("PCI bus {}", pci.bus_id),
                            Err(e) => format!("failed to get bus ID: {e}"),
                        };
                        log::warn!("Skipping GPU device {i} ({bus_id}) because it supports no useful feature.");
                        None
                    }
                }
                Err(e) => {
                    if skip_failed_devices {
                        match e {
                            // errors that can be skipped (device's fault)
                            NvmlError::InsufficientPower
                            | NvmlError::NoPermission
                            | NvmlError::IrqIssue
                            | NvmlError::GpuLost => {
                                log::warn!("Skipping GPU device {i} because of error: {e}");
                                None
                            }
                            // critical errors related to nvml itself
                            other => Err(other)?,
                        }
                    } else {
                        // don't skip, fail immediately
                        Err(e)?
                    }
                }
            };
            devices.push(device);
        }
        Ok(NvmlDevices { devices })
    }

    /// Gets and return status of GPU device detection on the operating system.
    pub fn detection_stats(&self) -> DetectionStats {
        let n_found = self.devices.len();
        let n_failed = self.devices.iter().filter(|d| d.is_none()).count();
        let n_ok = n_found - n_failed;
        DetectionStats {
            found_devices: n_found,
            failed_devices: n_failed,
            working_devices: n_ok,
        }
    }
}

impl ManagedDevice {
    pub fn as_wrapper(&self) -> Device<'_> {
        unsafe { Device::new(self.handle, &self.lib) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test `detect` function with valid and invalid devices detection
    #[ignore = "NO GPU"]
    #[test]
    fn test_detect_with_valid_devices() {
        let devices = NvmlDevices::detect(false).expect("Device recognizing");
        assert!(!devices.devices.is_empty());

        let devices = NvmlDevices::detect(true).expect("Device recognizing");
        assert!(!devices.devices.is_empty());
    }

    // Test `detect` function with stats detection
    #[ignore = "NO GPU"]
    #[test]
    fn test_detect_stats() {
        let devices = NvmlDevices::detect(false).expect("Device recognizing");
        assert_eq!(devices.detection_stats().found_devices, devices.devices.len());
    }

    // Test `as_wrapper` function with PCI informations
    #[ignore = "NO GPU"]
    #[test]
    fn test_as_wrapper() {
        let nvml = Arc::new(Nvml::init().expect("Initialize NVML lib"));
        let device = nvml.device_by_index(0).expect("Device recognizing");
        let handle = unsafe { device.handle() };
        let bus_id = device.pci_info().expect("PCI info").bus_id;

        let managed_device = ManagedDevice {
            lib: nvml.clone(),
            handle,
            features: OptionalFeatures::detect_on(&device).expect("Detect features"),
            bus_id,
        };

        let wrapped_device = managed_device.as_wrapper();
        assert!(wrapped_device.pci_info().is_ok());
    }
}
