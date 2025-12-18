use anyhow::{Context, anyhow};
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, error::NvmlError};
use std::time::SystemTime;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};

use crate::{
    features::AvailableVersion,
    metrics::{FullMetrics, MinimalMetrics},
    nvml_ext::DeviceExt,
};

use super::device::ManagedDevice;

pub enum SourceProvider {
    Full(FullMetrics),
    Minimal(MinimalMetrics),
}

/// Measurement source that queries NVML devices.
pub struct FullSource {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counter: CounterDiff,
    /// Handle to the GPU, with features information.
    device: ManagedDevice,
    /// Alumet metrics IDs.
    metrics: FullMetrics,
    /// Alumet resource ID.
    resource: Resource,

    /// Last poll timestamp
    last_poll_timestamp: Option<Timestamp>,
}

/// A minimal measurement source, that only queries basic NVML info in order to be faster.
pub struct MinimalSource {
    device: ManagedDevice,
    metrics: MinimalMetrics,
    resource: Resource,

    /// Previous power, to compute the energy
    previous_power: Option<PowerMeasure>,
}

// SAFETY: The pointer `nvmlDevice_t` returned by NVML can be sent between threads.
// NVML is thread-safe according to its documentation.
unsafe impl Send for FullSource {}
unsafe impl Send for MinimalSource {}

impl FullSource {
    pub fn new(device: ManagedDevice, metrics: FullMetrics) -> Result<Self, NvmlError> {
        let bus_id = std::borrow::Cow::Owned(device.bus_id.clone());
        Ok(FullSource {
            energy_counter: CounterDiff::with_max_value(u64::MAX),
            device,
            metrics,
            resource: Resource::Gpu { bus_id },
            last_poll_timestamp: None,
        })
    }
}

impl Source for FullSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;
        let device = self.device.as_wrapper();

        // no consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        if features.total_energy_consumption {
            // the difference in milliJoules
            let diff = self
                .energy_counter
                .update(device.total_energy_consumption()?)
                .difference();
            if let Some(milli_joules) = diff {
                // if meaningful (we need at least two measurements), push
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.total_energy_consumption,
                    self.resource.clone(),
                    consumer.clone(),
                    milli_joules as f64,
                ))
            }
        }

        // Get power consumption in milliWatts
        if features.instant_power {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.instant_power,
                self.resource.clone(),
                consumer.clone(),
                device.power_usage()? as u64,
            ))
        }

        // Get temperature of GPU in °C
        if features.temperature_gpu {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.temperature_gpu,
                self.resource.clone(),
                consumer.clone(),
                device.temperature(TemperatureSensor::Gpu)? as u64,
            ));
        }

        // Get the current utilization rates memory for this device major subsystems in percentage
        if features.major_utilization {
            let u = device.utilization_rates()?;
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.major_utilization_gpu,
                self.resource.clone(),
                consumer.clone(),
                u.gpu as u64,
            ));
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.major_utilization_memory,
                self.resource.clone(),
                consumer.clone(),
                u.memory as u64,
            ));
        }

        // Get the current utilization and sampling size in μs for the decoder
        if features.decoder_utilization {
            let u = device.decoder_utilization()?;
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.decoder_utilization,
                self.resource.clone(),
                consumer.clone(),
                u.utilization as u64,
            ));
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.decoder_sampling_period_us,
                self.resource.clone(),
                consumer.clone(),
                u.sampling_period as u64,
            ));
        }

        // Get the current utilization and sampling size in μs for the encoder
        if features.encoder_utilization {
            let u = device.encoder_utilization()?;
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.encoder_utilization,
                self.resource.clone(),
                consumer.clone(),
                u.utilization as u64,
            ));
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.encoder_sampling_period_us,
                self.resource.clone(),
                consumer.clone(),
                u.sampling_period as u64,
            ));
        }

        let n_compute_processes = match features.running_compute_processes {
            AvailableVersion::Latest => Some(device.running_compute_processes_count()?),
            AvailableVersion::V2 => Some(device.running_compute_processes_count_v2()?),
            AvailableVersion::None => None,
        };
        if let Some(n) = n_compute_processes {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.running_compute_processes,
                self.resource.clone(),
                consumer.clone(),
                n as u64,
            ));
        }

        let n_graphic_processes = match features.running_graphics_processes {
            AvailableVersion::Latest => Some(device.running_graphics_processes_count()?),
            AvailableVersion::V2 => Some(device.running_graphics_processes_count_v2()?),
            AvailableVersion::None => None,
        };
        if let Some(n) = n_graphic_processes {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.running_graphics_processes,
                self.resource.clone(),
                consumer.clone(),
                n as u64,
            ));
        }

        // Collection of the device processes-scoped measurements
        if features.process_utilization_stats {
            if let Some(last_poll_timestamp) = self.last_poll_timestamp {
                let unix_ts = last_poll_timestamp
                    .duration_since(SystemTime::UNIX_EPOCH.into())?
                    .as_secs();

                let processes_samples = device
                    .fixed_process_utilization_stats(unix_ts)
                    .context("process_utilization_stats failed")?;

                for process_sample in processes_samples {
                    let consumer = ResourceConsumer::Process {
                        pid: process_sample.pid,
                    };
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.sm_utilization,
                        self.resource.clone(),
                        consumer.clone(),
                        process_sample.sm_util as u64,
                    ));
                    // Frame buffer memory utilization
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.major_utilization_memory,
                        self.resource.clone(),
                        consumer.clone(),
                        process_sample.mem_util as u64,
                    ));
                    // Encoder utilization
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.encoder_utilization,
                        self.resource.clone(),
                        consumer.clone(),
                        process_sample.enc_util as u64,
                    ));
                    // Decoder utilization
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.decoder_utilization,
                        self.resource.clone(),
                        consumer.clone(),
                        process_sample.dec_util as u64,
                    ));
                }
            }
            self.last_poll_timestamp = Some(timestamp);
        }

        Ok(())
    }
}

struct PowerMeasure {
    t: Timestamp,
    power: u32,
}

impl MinimalSource {
    pub fn new(device: ManagedDevice, metrics: MinimalMetrics) -> anyhow::Result<Self> {
        let bus_id = std::borrow::Cow::Owned(device.bus_id.clone());
        if !device.features.instant_power {
            return Err(anyhow!(
                "minimal mode cannot be used on GPU [{bus_id}]: nvmlDeviceGetPowerUsage is not supported"
            ));
        }
        Ok(Self {
            device,
            metrics,
            resource: Resource::Gpu { bus_id },
            previous_power: None,
        })
    }
}

impl Source for MinimalSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let device = self.device.as_wrapper();

        // no consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;
        let power = device.power_usage()?;
        measurements.push(MeasurementPoint::new(
            timestamp,
            self.metrics.instant_power,
            self.resource.clone(),
            consumer.clone(),
            power as u64,
        ));

        // Estimate the energy consumption.
        let current_power = PowerMeasure { t: timestamp, power };
        if let Some(previous) = &self.previous_power {
            let energy = current_power.compute_energy(previous).unwrap();
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.total_energy_consumption,
                self.resource.clone(),
                consumer.clone(),
                energy,
            ));
        }
        self.previous_power = Some(current_power);
        Ok(())
    }
}

impl PowerMeasure {
    /// Computes an energy from a power, using as time the time elapsed between
    /// the current timestamp and the previous timestamp.
    ///
    /// This function first computes the time elapsed between two timestamps.
    /// It return an error if it's not possible
    /// The energy is computed using a discrete integral with the formula: Energy = ((Power(t0) + Power(t1)) / 2) * Δt
    fn compute_energy(&self, previous: &PowerMeasure) -> anyhow::Result<f64> {
        let time_elapsed = self.t.duration_since(previous.t)?.as_secs_f64();
        let energy_consumed = ((self.power + previous.power) as f64) * 0.5 * time_elapsed;
        Ok(energy_consumed)
    }
}
