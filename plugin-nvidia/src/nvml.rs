use std::time::SystemTime;

use alumet::{
    metrics::{MeasurementAccumulator, MeasurementPoint, MeasurementValue, MetricId, ResourceId},
    pipeline::PollError,
    util::{CounterDiff, CounterDiffUpdate},
};
use nvml_wrapper::{error::NvmlError, Device, Nvml};

pub struct NvmlSource<'a> {
    energy_counter: CounterDiff,
    features: OptionalFeatures,
    metrics: Metrics,
    resource: ResourceId,
    device: Device<'a>,
    _nvml: &'a Nvml,
}

impl<'a> NvmlSource<'a> {
    pub fn new(
        features: OptionalFeatures,
        metrics: Metrics,
        device: Device<'a>,
        nvml: &'a Nvml,
    ) -> Result<NvmlSource<'a>, NvmlError> {
        Ok(NvmlSource {
            energy_counter: CounterDiff::with_max_value(u64::MAX),
            features,
            metrics,
            resource: ResourceId::Gpu {
                bus_id: device.pci_info()?.bus_id.into(),
            },
            device,
            _nvml: nvml,
        })
    }
}

impl<'a> alumet::pipeline::Source for NvmlSource<'a> {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: SystemTime) -> Result<(), PollError> {
        if self.features.total_energy_consumption {
            // the difference in milliJoules
            let diff = match self.energy_counter.update(self.device.total_energy_consumption()?) {
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
                    MeasurementValue::Float(joules),
                ))
            }
        }

        if self.features.instant_power {
            // the power in milliWatts
            let power = self.device.power_usage()?;
            // convert to watts and push
            let watts = power as f64 / 1000.0;
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.instant_power,
                self.resource.clone(),
                MeasurementValue::Float(watts),
            ))
        }

        if self.features.major_utilization {
            let u = self.device.utilization_rates()?;
            let major_gpu = MeasurementValue::UInt(u.gpu as _);
            let major_mem = MeasurementValue::UInt(u.memory as _);
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.major_utilization_gpu,
                self.resource.clone(),
                major_gpu,
            ));
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.major_utilization_memory,
                self.resource.clone(),
                major_mem,
            ));
        }

        // TODO more metrics
        // TODO explore device.samples() to gather multiple metrics at once
        Ok(())
    }
}

pub struct Metrics {
    total_energy_consumption: MetricId,
    instant_power: MetricId,
    major_utilization_gpu: MetricId,
    major_utilization_memory: MetricId,
    decoder_utilization: MetricId,
    encoder_utilization: MetricId,
    running_compute_processes: MetricId,
    running_graphics_processes: MetricId,
}

pub struct OptionalFeatures {
    total_energy_consumption: bool,
    instant_power: bool,
    major_utilization: bool,
    decoder_utilization: bool,
    encoder_utilization: bool,
    running_compute_processes: AvailableVersion,
    running_graphics_processes: AvailableVersion,
}

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
        Ok(t) => Ok(true),
        Err(NvmlError::NotSupported) => Ok(false),
        Err(e) => Err(e),
    }
}
