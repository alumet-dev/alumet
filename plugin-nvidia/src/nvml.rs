use std::{sync::Arc, time::SystemTime};

use alumet::metrics::MetricCreationError;
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint},
    metrics::TypedMetricId,
    pipeline::PollError,
    plugin::util::{CounterDiff, CounterDiffUpdate},
    plugin::AlumetStart,
    resources::ResourceId,
    units::Unit,
};
use anyhow::Context;
use nvml_wrapper::{error::NvmlError, Device, Nvml};
use nvml_wrapper_sys::bindings::nvmlDevice_t;

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

/// Measurement source that queries NVML devices.
pub struct NvmlSource {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counter: CounterDiff,
    /// Handle to the GPU, with features information.
    device: ManagedDevice,
    /// Alumet metrics IDs.
    metrics: Metrics,
    /// Alumet resource ID.
    resource: ResourceId,
}

// The pointer `nvmlDevice_t` returned by NVML can be sent between threads.
// NVML is thread-safe according to its documentation.
unsafe impl Send for NvmlSource {}

impl NvmlSource {
    pub fn new(device: ManagedDevice, metrics: Metrics) -> Result<NvmlSource, NvmlError> {
        let bus_id = std::borrow::Cow::Owned(device.bus_id.clone());
        Ok(NvmlSource {
            energy_counter: CounterDiff::with_max_value(u64::MAX),
            device,
            metrics,
            resource: ResourceId::Gpu { bus_id },
        })
    }
}

impl alumet::pipeline::Source for NvmlSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: SystemTime) -> Result<(), PollError> {
        let features = &self.device.features;
        let device = self.device.as_wrapper();

        if features.total_energy_consumption {
            // the difference in milliJoules
            let diff = match self.energy_counter.update(device.total_energy_consumption()?) {
                CounterDiffUpdate::FirstTime => None,
                CounterDiffUpdate::Difference(diff) => Some(diff),
                CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
            };
            if let Some(diff) = diff {
                // if meaningful (we need at least two measurements), convert to joules and push
                let joules: f64 = diff as f64 / 1000.0;
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.total_energy_consumption,
                    self.resource.clone(),
                    joules,
                ))
            }
        }

        if features.instant_power {
            // the power in milliWatts
            let power = device.power_usage()?;
            // convert to watts and push
            let watts = power as f64 / 1000.0;
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.instant_power,
                self.resource.clone(),
                watts,
            ))
        }

        if features.major_utilization {
            let u = device.utilization_rates()?;
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.major_utilization_gpu,
                self.resource.clone(),
                u.gpu as u64,
            ));
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.major_utilization_memory,
                self.resource.clone(),
                u.memory as u64,
            ));
        }

        // TODO more metrics
        // TODO explore device.samples() to gather multiple metrics at once
        Ok(())
    }
}

/// Contains the ids of the measured metrics.
#[derive(Clone)]
pub struct Metrics {
    total_energy_consumption: TypedMetricId<f64>,
    instant_power: TypedMetricId<f64>,
    major_utilization_gpu: TypedMetricId<u64>,
    major_utilization_memory: TypedMetricId<u64>,
    decoder_utilization: TypedMetricId<u64>,
    encoder_utilization: TypedMetricId<u64>,
    running_compute_processes: TypedMetricId<u64>,
    running_graphics_processes: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            total_energy_consumption: alumet.create_metric(
                "nvml_consumed_energy",
                Unit::Joule,
                "energy consumption by the GPU since the previous measurement",
            )?,
            instant_power: alumet.create_metric(
                "nvml_instant_power",
                Unit::Watt,
                "instantaneous power of the GPU at the time of the measurement",
            )?,
            major_utilization_gpu: alumet.create_metric("gpu_utilization", Unit::Unity, "")?,
            major_utilization_memory: alumet.create_metric("memory_utilization", Unit::Unity, "")?,
            decoder_utilization: alumet.create_metric("decoder_utilization", Unit::Unity, "")?,
            encoder_utilization: alumet.create_metric("encoder_utilization", Unit::Unity, "")?,
            running_compute_processes: alumet.create_metric("n_compute_processes", Unit::Unity, "")?,
            running_graphics_processes: alumet.create_metric("n_graphic_processes", Unit::Unity, "")?,
        })
    }
}

/// Indicates which features are available on a given NVML device.
#[derive(Debug)]
pub struct OptionalFeatures {
    total_energy_consumption: bool,
    instant_power: bool,
    major_utilization: bool,
    decoder_utilization: bool,
    encoder_utilization: bool,
    running_compute_processes: AvailableVersion,
    running_graphics_processes: AvailableVersion,
}

/// Indicates which version of a NVML function is available on a given device.
#[derive(Debug, PartialEq, Eq)]
pub enum AvailableVersion {
    Latest,
    V2,
    None,
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(device: &Device) -> Result<Self, NvmlError> {
        Ok(Self {
            total_energy_consumption: is_supported(device.total_energy_consumption())?,
            instant_power: is_supported(device.power_usage())?,
            major_utilization: is_supported(device.utilization_rates())?,
            decoder_utilization: is_supported(device.decoder_utilization())?,
            encoder_utilization: is_supported(device.encoder_utilization())?,
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
            || self.running_compute_processes != AvailableVersion::None
            || self.running_graphics_processes != AvailableVersion::None
    }
}

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

fn if_supported<T>(res: Result<T, NvmlError>) -> Result<Option<T>, NvmlError> {
    match res {
        Ok(t) => Ok(Some(t)),
        Err(NvmlError::NotSupported) => Ok(None),
        Err(e) => Err(e),
    }
}

fn is_supported<T>(res: Result<T, NvmlError>) -> Result<bool, NvmlError> {
    match res {
        Ok(_) => Ok(true),
        Err(NvmlError::NotSupported) => Ok(false),
        Err(e) => Err(e),
    }
}

impl NvmlDevices {
    /// Detects the GPUs that are available on the machine, and adds them to this container.
    ///
    /// If `skip_failed_devices` is true, inaccessible GPUs will be ignored.
    /// If `ski_failed_devices` is false, the function will return an error at the first inaccessible GPU.
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
                        let d = ManagedDevice { lib, handle, features, bus_id };
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

    pub fn get(&self, index: usize) -> Option<Device<'_>> {
        if let Some(Some(device)) = self.devices.get(index) {
            Some(unsafe { Device::new(device.handle, &device.lib) })
        } else {
            None
        }
    }
}

impl ManagedDevice {
    pub fn as_wrapper<'a>(&'a self) -> Device<'a> {
        unsafe { Device::new(self.handle, &self.lib) }
    }
}
