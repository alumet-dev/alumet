use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, MeasurementType, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use amd_smi_wrapper::{
    handles::ProcessorHandle,
    metrics::{AmdTemperatureMetric, AmdVoltageMetric, AmdVoltageType},
};
use anyhow::Result;
use std::{borrow::Cow, collections::HashMap};

use super::{device::ManagedDevice, metrics::Metrics};
use crate::amd::utils::{MEMORY_TYPE, SENSOR_TYPE};

/// Measurement source that queries AMD GPU devices.
pub struct AmdGpuSource<H: ProcessorHandle> {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counters: HashMap<String, CounterDiff>,
    /// Handle to the GPU, with features information.
    devices: Vec<ManagedDevice<H>>,
    /// Alumet metrics IDs.
    metrics: Metrics,
}

/// Check if an [`f64`] or [`u64`] metric value is not null, to push it or not in Alumet pipeline.
/// The collect of certain metrics is triggered under specific conditions (activity engine, running processes...).
/// So, if the value of these metrics is null, it's best to ignore them to economize pipeline resources.
trait MeasurementValue {
    fn check_push(&self) -> bool;
}

impl MeasurementValue for u64 {
    fn check_push(&self) -> bool {
        *self != 0
    }
}

impl MeasurementValue for f64 {
    fn check_push(&self) -> bool {
        *self != 0.0 && !self.is_nan()
    }
}

/// Push metric method conditioned by [`MeasurementValue::check_push`] to send only not null values in pipeline.
fn push<T, F>(
    measurements: &mut MeasurementAccumulator,
    timestamp: Timestamp,
    metric: TypedMetricId<T>,
    resource: &Resource,
    consumer: &ResourceConsumer,
    value: T,
    attributes: F,
) where
    T: MeasurementValue + MeasurementType<T = T>,
    F: FnOnce() -> (&'static str, String),
{
    if value.check_push() {
        let (key, label) = attributes();
        measurements.push(
            MeasurementPoint::new(timestamp, metric, resource.clone(), consumer.clone(), value).with_attr(key, label),
        );
    }
}

impl<H: ProcessorHandle> AmdGpuSource<H> {
    pub fn new(devices: Vec<ManagedDevice<H>>, metrics: Metrics) -> Self {
        let energy_counters = devices
            .iter()
            .map(|d| (d.bus_id.clone(), CounterDiff::with_max_value(u64::MAX)))
            .collect();

        Self {
            energy_counters,
            devices,
            metrics,
        }
    }

    /// Retrieves and push all data concerning the activity utilization of a given AMD GPU device.
    fn handle_gpu_activity(
        &self,
        device: &ManagedDevice<H>,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        consumer: &ResourceConsumer,
        resource: &Resource,
    ) {
        let features = &device.features;

        if features.gpu_activity_usage
            && let Ok(value) = device.handle.device_activity()
        {
            const KEY: &str = "activity_type";

            push(
                measurements,
                timestamp,
                self.metrics.gpu_activity_usage,
                resource,
                consumer,
                value.gfx_activity as f64,
                || (KEY, "graphic_core".to_string()),
            );

            push(
                measurements,
                timestamp,
                self.metrics.gpu_activity_usage,
                resource,
                consumer,
                value.mm_activity as f64,
                || (KEY, "memory_management".to_string()),
            );

            push(
                measurements,
                timestamp,
                self.metrics.gpu_activity_usage,
                resource,
                consumer,
                value.umc_activity as f64,
                || (KEY, "unified_memory_controller".to_string()),
            );
        }
    }

    /// Retrieves and push all data concerning the running process resources consumption of a given AMD GPU device.
    fn handle_gpu_processes(
        &self,
        device: &ManagedDevice<H>,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        resource: &Resource,
        compute_units_number: u32,
    ) {
        let features = &device.features;

        if features.gpu_process
            && let Ok(process_list) = device.handle.device_process_list()
        {
            const KEY: &str = "process_name";

            for process in process_list {
                let consumer = ResourceConsumer::Process { pid: process.pid };
                let name = &process.name;

                if compute_units_number > 0 {
                    push(
                        measurements,
                        timestamp,
                        self.metrics.process_cu_occupancy,
                        resource,
                        &consumer,
                        (process.cu_occupancy as f64 / compute_units_number as f64) * 100.0,
                        || (KEY, name.to_string()),
                    );
                }

                push(
                    measurements,
                    timestamp,
                    self.metrics.process_memory_usage,
                    resource,
                    &consumer,
                    process.mem,
                    || (KEY, name.to_string()),
                );

                push(
                    measurements,
                    timestamp,
                    self.metrics.process_engine_usage_gfx,
                    resource,
                    &consumer,
                    process.engine_usage.gfx,
                    || (KEY, name.to_string()),
                );

                push(
                    measurements,
                    timestamp,
                    self.metrics.process_engine_usage_encode,
                    resource,
                    &consumer,
                    process.engine_usage.enc,
                    || (KEY, name.to_string()),
                );

                push(
                    measurements,
                    timestamp,
                    self.metrics.process_memory_usage_gtt,
                    resource,
                    &consumer,
                    process.memory_usage.gtt_mem,
                    || (KEY, name.to_string()),
                );

                push(
                    measurements,
                    timestamp,
                    self.metrics.process_memory_usage_cpu,
                    resource,
                    &consumer,
                    process.memory_usage.cpu_mem,
                    || (KEY, name.to_string()),
                );

                push(
                    measurements,
                    timestamp,
                    self.metrics.process_memory_usage_vram,
                    resource,
                    &consumer,
                    process.memory_usage.vram_mem,
                    || (KEY, name.to_string()),
                );
            }
        }
    }
}

impl<H: ProcessorHandle> Source for AmdGpuSource<H> {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        // No consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        for device in &self.devices {
            let resource = Resource::Gpu {
                bus_id: Cow::Owned(device.bus_id.clone()),
            };

            let features = &device.features;

            // GPU engine data usage metric pushed
            self.handle_gpu_activity(device, measurements, timestamp, &consumer, &resource);

            // Global GPU chip information
            if features.gpu_asic_info
                && let Ok(value) = device.handle.device_asic_info()
            {
                // Push GPU compute-graphic process informations if processes existing
                self.handle_gpu_processes(device, measurements, timestamp, &resource, value.num_of_compute_units);
            }

            // GPU energy consumption metric pushed
            if features.gpu_energy_consumption
                && let Ok(res) = device.handle.device_energy_consumption()
                && let Some(counter) = self.energy_counters.get_mut(&device.bus_id)
                && let Some(diff) = counter.update(res.energy).difference()
            {
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_energy_consumption,
                    resource.clone(),
                    consumer.clone(),
                    (diff as f64 * res.resolution as f64) / 1e3,
                ));
            }

            // GPU instant electric power consumption metric pushed
            if features.gpu_power_consumption
                && device.handle.device_power_managment()?
                && let Ok(value) = device.handle.device_power_consumption()
            {
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_power_consumption,
                    resource.clone(),
                    consumer.clone(),
                    value.socket_power,
                ));
            }

            // Voltage
            if features.gpu_voltage
                && let Ok(value) = device.handle.device_voltage(
                    AmdVoltageType::AMDSMI_VOLT_TYPE_VDDGFX,
                    AmdVoltageMetric::AMDSMI_VOLT_CURRENT,
                )
            {
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_voltage,
                    resource.clone(),
                    consumer.clone(),
                    value as u64,
                ));
            }

            // GPU memories used metric pushed
            for (mem_type, label) in &MEMORY_TYPE {
                if features
                    .gpu_memories_usage
                    .iter()
                    .find(|(m, _)| *m == *mem_type)
                    .map(|(_, v)| *v)
                    .unwrap_or(false)
                    && let Ok(value) = device.handle.device_memory_usage(*mem_type)
                {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.gpu_memory_usages,
                            resource.clone(),
                            consumer.clone(),
                            value,
                        )
                        .with_attr("memory_type", label.to_string()),
                    );
                }
            }

            // GPU temperatures metric pushed
            for (sensor, label) in &SENSOR_TYPE {
                if features
                    .gpu_temperatures
                    .iter()
                    .find(|(s, _)| *s == *sensor)
                    .map(|(_, v)| *v)
                    .unwrap_or(false)
                    && let Ok(value) = device
                        .handle
                        .device_temperature(*sensor, AmdTemperatureMetric::AMDSMI_TEMP_CURRENT)
                {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.gpu_temperatures,
                            resource.clone(),
                            consumer.clone(),
                            value as u64,
                        )
                        .with_attr("thermal_zone", label.to_string()),
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MeasurementValue;

    #[test]
    fn test_check_push_interger() {
        assert_eq!(0.check_push(), false);
        assert_eq!(2.check_push(), true);
    }

    #[test]
    fn test_check_push_float() {
        assert_eq!(0.0.check_push(), false);
        assert_eq!(2.5.check_push(), true);
        assert_eq!((-4.5).check_push(), true);
        assert_eq!(f64::NAN.check_push(), false);
    }
}
