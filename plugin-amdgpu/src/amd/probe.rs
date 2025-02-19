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
    pub gpu_counter: HashMap<String, CounterDiff>,
    /// Metric type based on GPU clock frequency data.
    pub clock_frequencies: TypedMetricId<u64>,
    /// Metric type based on GPU energy consumption data.
    pub energy_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU engine units usage data.
    pub engine_usage: TypedMetricId<u64>,
    /// Metric type based on GPU used memory data.
    pub memory_usages: TypedMetricId<u64>,
    /// Metric type base on GPU PCI bus sent data consumption.
    pub pci_data_sent: TypedMetricId<u64>,
    /// Metric type base on GPU PCI bus received data consumption.
    pub pci_data_received: TypedMetricId<u64>,
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
        clock_frequencies: TypedMetricId<u64>,
        energy_consumption: TypedMetricId<u64>,
        engine_usage: TypedMetricId<u64>,
        memory_usages: TypedMetricId<u64>,
        pci_data_sent: TypedMetricId<u64>,
        pci_data_received: TypedMetricId<u64>,
        power_consumption: TypedMetricId<u64>,
        temperatures: TypedMetricId<u64>,
        process_counter: TypedMetricId<u64>,
        process_usage_compute_unit: TypedMetricId<u64>,
        process_usage_vram: TypedMetricId<u64>,
    ) -> Self {
        Self {
            clock_frequencies,
            energy_consumption,
            engine_usage,
            memory_usages,
            pci_data_sent,
            pci_data_received,
            power_consumption,
            temperatures,
            process_counter,
            process_usage_compute_unit,
            process_usage_vram,
            gpu_counter: HashMap::new(),
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
                if !self.gpu_counter.contains_key(&id) {
                    self.gpu_counter
                        .insert(id.clone(), CounterDiff::with_max_value(u64::MAX));
                }
                let counter = self.gpu_counter.get_mut(&id).unwrap();

                // GPU energy consumption metric pushed
                if let Some(data) = device.metrics.energy_consumption {
                    if data.supported {
                        let diff = counter.update(data.value.unwrap()).difference();
                        if let Some(value) = diff {
                            measurement.push(MeasurementPoint::new(
                                timestamp,
                                self.energy_consumption,
                                resource.clone(),
                                consumer.clone(),
                                value,
                            ));
                        }
                    }
                }

                // GPU electric average power consumption metric pushed
                if let Some(data) = device.metrics.power_consumption {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.power_consumption,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }

                // GPU engine data usage metric pushed
                if let Some(data) = device.metrics.engine_usage {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.engine_usage,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }

                // GPU PCI bus sent data consumption metric pushed
                if let Some(data) = device.metrics.pci_data_sent {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.pci_data_sent,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }
                // GPU PCI bus received data consumption metric pushed
                if let Some(data) = device.metrics.pci_data_received {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.pci_data_received,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }

                // GPU clock frequencies metrics pushed
                for (data, clock_type) in &device.metrics.clock_frequencies {
                    if data.supported {
                        measurement.push(
                            MeasurementPoint::new(
                                timestamp,
                                self.clock_frequencies,
                                resource.clone(),
                                consumer.clone(),
                                data.value.unwrap(),
                            )
                            .with_attr("clock_type", clock_type.to_string()),
                        );
                    }
                }

                // GPU memories used metric pushed
                for (data, memory_type) in &device.metrics.memory_usages {
                    if data.supported {
                        measurement.push(
                            MeasurementPoint::new(
                                timestamp,
                                self.memory_usages,
                                resource.clone(),
                                consumer.clone(),
                                data.value.unwrap(),
                            )
                            .with_attr("memory_type", memory_type.to_string()),
                        );
                    }
                }

                // GPU temperatures metric pushed
                for (data, thermal_zone) in &device.metrics.temperatures {
                    if data.supported {
                        measurement.push(
                            MeasurementPoint::new(
                                timestamp,
                                self.temperatures,
                                resource.clone(),
                                consumer.clone(),
                                data.value.unwrap(),
                            )
                            .with_attr("thermal_zone", thermal_zone.to_string()),
                        );
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
                if let Some(data) = processes.counter {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.process_counter,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }

                // GPU compute processes compute unit usage metric pushed
                if let Some(data) = processes.compute_unit_usage {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.process_usage_compute_unit,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }

                // GPU compute processes VRAM memory usage metric pushed
                if let Some(data) = processes.vram_usage {
                    if data.supported {
                        measurement.push(MeasurementPoint::new(
                            timestamp,
                            self.process_usage_vram,
                            resource.clone(),
                            consumer.clone(),
                            data.value.unwrap(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
