use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;
use std::collections::HashMap;

use crate::amd::metric::{gather_amd_gpu_device_metrics, gather_amd_gpu_metric_process};

/// Collect and format AMD GPU metrics to push them.
pub struct AmdGpuProbe {
    /// [`CounterDiff`] for a specific GPU.
    pub gpu_counter: HashMap<String, CounterDiff>,
    /// Metric type based on GPU clock frequency data.
    pub clock_frequency: TypedMetricId<u64>,
    /// Metric type based on GPU energy consumption data.
    pub energy_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU engine units usage data.
    pub engine_usage: TypedMetricId<u64>,
    /// Metric type based on GPU used memory data.
    pub memory_usage: TypedMetricId<u64>,
    /// Metric type base on GPU PCI bus sent data consumption.
    pub pci_data_sent: TypedMetricId<u64>,
    /// Metric type base on GPU PCI bus received data consumption.
    pub pci_data_received: TypedMetricId<u64>,
    /// Metric type based on GPU electric power consumption data.
    pub power_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU temperature data.
    pub temperature: TypedMetricId<u64>,
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
        clock_frequency: TypedMetricId<u64>,
        energy_consumption: TypedMetricId<u64>,
        engine_usage: TypedMetricId<u64>,
        memory_usage: TypedMetricId<u64>,
        pci_data_sent: TypedMetricId<u64>,
        pci_data_received: TypedMetricId<u64>,
        power_consumption: TypedMetricId<u64>,
        temperature: TypedMetricId<u64>,
        process_counter: TypedMetricId<u64>,
        process_usage_compute_unit: TypedMetricId<u64>,
        process_usage_vram: TypedMetricId<u64>,
    ) -> Self {
        Self {
            clock_frequency,
            energy_consumption,
            engine_usage,
            memory_usage,
            pci_data_sent,
            pci_data_received,
            power_consumption,
            temperature,
            process_counter,
            process_usage_compute_unit,
            process_usage_vram,
            gpu_counter: HashMap::new(),
        }
    }
}

impl Source for AmdGpuProbe {
    fn poll(&mut self, measurement: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for device in gather_amd_gpu_device_metrics()? {
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
                for (data, label) in &device.metrics.clock_frequency {
                    if data.supported {
                        measurement.push(
                            MeasurementPoint::new(
                                timestamp,
                                self.clock_frequency,
                                resource.clone(),
                                ResourceConsumer::LocalMachine,
                                data.value.unwrap(),
                            )
                            .with_attr("clock_type", label.to_string()),
                        );
                    }
                }

                // GPU memories used metric pushed
                for (data, label) in &device.metrics.memory_usage {
                    if data.supported {
                        measurement.push(
                            MeasurementPoint::new(
                                timestamp,
                                self.memory_usage,
                                resource.clone(),
                                ResourceConsumer::LocalMachine,
                                data.value.unwrap(),
                            )
                            .with_attr("memory_type", label.to_string()),
                        );
                    }
                }

                // GPU temperatures metric pushed
                for (data, label) in &device.metrics.temperature {
                    if data.supported {
                        measurement.push(
                            MeasurementPoint::new(
                                timestamp,
                                self.temperature,
                                resource.clone(),
                                ResourceConsumer::LocalMachine,
                                data.value.unwrap(),
                            )
                            .with_attr("thermal_zone", label.to_string()),
                        );
                    }
                }
            }
        }

        // Push GPU compute process informations if processes existing
        let metric_gpu_process = gather_amd_gpu_metric_process()?;

        // GPU processes PID metric pushed
        if let Some(pid) = metric_gpu_process.pid {
            let resource = Resource::LocalMachine;
            let consumer = ResourceConsumer::Process { pid };

            // GPU compute processes compute unit usage metric pushed
            if let Some(data) = metric_gpu_process.counter {
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
            if let Some(data) = metric_gpu_process.compute_unit_usage {
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
            if let Some(data) = metric_gpu_process.vram_usage {
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
        Ok(())
    }
}
