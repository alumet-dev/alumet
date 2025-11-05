use anyhow::Context;
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, error::NvmlError};
use std::time::SystemTime;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::elements::error::PollError,
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};

use crate::{features::AvailableVersion, metrics::Metrics, nvml_ext::DeviceExt};

use super::device::ManagedDevice;

/// Measurement source that queries NVML devices.
pub struct NvmlSource {
    /// Handle to the GPU, with features information.
    device: ManagedDevice,
    /// Alumet metrics IDs.
    metrics: Metrics,
    /// Alumet resource ID.
    resource: Resource,

    /// Previous power, to compute the energy
    previous_power: Option<PowerMeasure>,
}

// The pointer `nvmlDevice_t` returned by NVML can be sent between threads.
// NVML is thread-safe according to its documentation.
unsafe impl Send for NvmlSource {}

impl NvmlSource {
    pub fn new(device: ManagedDevice, metrics: Metrics) -> Result<NvmlSource, NvmlError> {
        let bus_id = std::borrow::Cow::Owned(device.bus_id.clone());
        Ok(NvmlSource {
            device,
            metrics,
            resource: Resource::Gpu { bus_id },
            previous_power: None,
        })
    }
}

fn log_timing<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if log::log_enabled!(log::Level::Debug) {
        let t0 = Timestamp::now();
        let res = f();
        let t1 = Timestamp::now();
        let delta = t1.duration_since(t0).unwrap();
        log::debug!("{label}: {} µs", delta.as_micros());
        res
    } else {
        f()
    }
}

impl alumet::pipeline::Source for NvmlSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;
        let device = self.device.as_wrapper();

        // no consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        // SPEED CONSTRAINT => only call power_usage()

        // Get power consumption in milliWatts
        assert!(
            features.instant_power,
            "this device does not support power measurement, we have nothing to measure"
        );

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
                energy as f64,
            ));
        }
        self.previous_power = Some(current_power);

        Ok(())
    }
}

struct PowerMeasure {
    t: Timestamp,
    power: u32,
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
