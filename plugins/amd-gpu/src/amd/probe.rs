use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;
use std::collections::HashMap;

use crate::amd::metric::{gather_amd_gpu_device_measurements, gather_amd_gpu_process_measurements};

/// Collect and format AMD GPU metrics to push them.
pub struct AmdGpuProbe {
    /// [`CounterDiff`] for a specific GPU.
    pub counter_diff: HashMap<String, CounterDiff>,
    /// Metric type based on GPU energy consumption data.
    pub energy_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU engine units usage data.
    pub engine_usage: TypedMetricId<u64>,
    /// Metric type based on GPU used memory data.
    pub memory_usages: TypedMetricId<u64>,
    /// Metric type based on GPU electric power consumption data.
    pub power_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU temperature data.
    pub temperatures: TypedMetricId<u64>,
    /// Metric type base on GPU process counter data.
    pub process_counter: TypedMetricId<u64>,
    ///  Metric type based on GPU process compute unit usage data.
    pub process_usage_compute_unit: TypedMetricId<u64>,
    /// Metric type based on GPU process VRAM memory usage data.
    pub process_usage_vram: TypedMetricId<u64>,
}

impl AmdGpuProbe {
    /// New [`AmdGpuProbe`] instance for metrics implementation and writing.
    pub fn new(
        energy_consumption: TypedMetricId<u64>,
        engine_usage: TypedMetricId<u64>,
        memory_usages: TypedMetricId<u64>,
        power_consumption: TypedMetricId<u64>,
        temperatures: TypedMetricId<u64>,
        process_counter: TypedMetricId<u64>,
        process_usage_compute_unit: TypedMetricId<u64>,
        process_usage_vram: TypedMetricId<u64>,
    ) -> Self {
        Self {
            energy_consumption,
            engine_usage,
            memory_usages,
            power_consumption,
            temperatures,
            process_counter,
            process_usage_compute_unit,
            process_usage_vram,
            counter_diff: HashMap::new(),
        }
    }
}

impl Source for AmdGpuProbe {
    fn poll(&mut self, measurement: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for device in gather_amd_gpu_device_measurements()? {
            let consumer = ResourceConsumer::LocalMachine;

            if let Some(id) = device.metrics.bus_id.clone() {
                let bus_id = std::borrow::Cow::Owned(id.clone());
                let resource = Resource::Gpu { bus_id };

                // Counter initialisation by detected GPU
                if !self.counter_diff.contains_key(&id) {
                    self.counter_diff
                        .insert(id.clone(), CounterDiff::with_max_value(u64::MAX));
                }
                let counter = self.counter_diff.get_mut(&id).unwrap();

                // GPU energy consumption metric pushed
                if device.available.gpu_energy_consumption {
                    if let Some(value) = device.metrics.energy_consumption {
                        let diff = counter.update(value).difference();
                        if let Some(data) = diff {
                            measurement.push(MeasurementPoint::new(
                                timestamp,
                                self.energy_consumption,
                                resource.clone(),
                                consumer.clone(),
                                data,
                            ));
                        }
                    }
                }

                // GPU electric average power consumption metric pushed
                if device.available.gpu_power_consumption {
                    if let Some(value) = device.metrics.power_consumption {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.power_consumption,
                            resource.clone(),
                            consumer.clone(),
                            value,
                        ));
                    }
                }

                // GPU engine data usage metric pushed
                if device.available.gpu_engine_usage {
                    if let Some(value) = device.metrics.engine_usage {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.engine_usage,
                            resource.clone(),
                            consumer.clone(),
                            value,
                        ));
                    }
                }

                // GPU memories used metric pushed
                for (data, memory_type) in &device.metrics.memory_usages {
                    if device.available.gpu_memory_usages {
                        if let Some(value) = data {
                            measurement.push(
                                MeasurementPoint::new(
                                    timestamp,
                                    self.memory_usages,
                                    resource.clone(),
                                    consumer.clone(),
                                    *value,
                                )
                                .with_attr("memory_type", memory_type.to_string()),
                            );
                        }
                    }
                }

                // GPU temperatures metric pushed
                for (data, thermal_zone) in &device.metrics.temperatures {
                    if device.available.gpu_temperatures {
                        if let Some(value) = data {
                            measurement.push(
                                MeasurementPoint::new(
                                    timestamp,
                                    self.temperatures,
                                    resource.clone(),
                                    consumer.clone(),
                                    *value,
                                )
                                .with_attr("thermal_zone", thermal_zone.to_string()),
                            );
                        }
                    }
                }
            }
        }

        // Push GPU compute process informations if processes existing
        for processes in gather_amd_gpu_process_measurements()? {
            if let Some(pid) = processes.pid {
                let resource = Resource::LocalMachine;
                let consumer = ResourceConsumer::Process { pid };

                // GPU compute processes compute unit usage metric pushed
                if let Some(value) = processes.counter {
                    measurement.push(MeasurementPoint::new(
                        timestamp,
                        self.process_counter,
                        resource.clone(),
                        consumer.clone(),
                        value,
                    ));
                }

                // GPU compute processes compute unit usage metric pushed
                if let Some(value) = processes.compute_unit_usage {
                    measurement.push(MeasurementPoint::new(
                        timestamp,
                        self.process_usage_compute_unit,
                        resource.clone(),
                        consumer.clone(),
                        value,
                    ));
                }

                // GPU compute processes VRAM memory usage metric pushed
                if let Some(value) = processes.vram_usage {
                    measurement.push(MeasurementPoint::new(
                        timestamp,
                        self.process_usage_vram,
                        resource.clone(),
                        consumer.clone(),
                        value,
                    ));
                }
            }
        }
        Ok(())
    }
}
