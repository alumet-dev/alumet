use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, error::NvmlError};
use std::time::SystemTime;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::elements::error::PollError,
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};

use crate::nvml::{features::AvailableVersion, metrics::Metrics};

use super::device::ManagedDevice;

/// Measurement source that queries NVML devices.
pub struct NvmlSource {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counter: CounterDiff,
    /// Handle to the GPU, with features information.
    device: ManagedDevice,
    /// Alumet metrics IDs.
    metrics: Metrics,
    /// Alumet resource ID.
    resource: Resource,

    /// Last poll timestamp
    last_poll_timestamp: Option<Timestamp>,
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
            resource: Resource::Gpu { bus_id },
            last_poll_timestamp: None,
        })
    }
}

impl alumet::pipeline::Source for NvmlSource {
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
                    milli_joules,
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

                let processes_samples = device.process_utilization_stats(unix_ts)?;

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
