use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;
use std::{borrow::Cow, collections::HashMap, ffi::CStr};

use super::{device::ManagedDevice, metrics::Metrics};
use crate::{
    amd::utils::{MEMORY_TYPE, METRIC_TEMP, SENSOR_TYPE},
    bindings::{
        amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT, amdsmi_voltage_type_t,
        amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX,
    },
};

/// Measurement source that queries AMD GPU devices.
pub struct AmdGpuSource {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counters: HashMap<String, CounterDiff>,
    /// Handle to the GPU, with features information.
    devices: Vec<ManagedDevice>,
    /// Alumet metrics IDs.
    metrics: Metrics,
}

impl AmdGpuSource {
    pub fn new(devices: Vec<ManagedDevice>, metrics: Metrics) -> Self {
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
        device: &ManagedDevice,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        consumer: &ResourceConsumer,
        resource: &Resource,
    ) -> anyhow::Result<()> {
        let features = &device.features;

        if features.gpu_activity_usage
            && let Ok(value) = device.handle.get_device_activity()
        {
            const KEY: &str = "activity_type";

            let mut push = |v, label| {
                if v != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.gpu_activity_usage,
                            resource.clone(),
                            consumer.clone(),
                            v as f64,
                        )
                        .with_attr(KEY, label),
                    );
                }
            };

            push(value.gfx_activity, "graphic_core");
            push(value.mm_activity, "memory_management");
            push(value.umc_activity, "unified_memory_controller");
        }
        Ok(())
    }

    /// Retrieves and push all data concerning the running process ressources consumption of a given AMD GPU device.
    fn handle_gpu_processes(
        &self,
        device: &ManagedDevice,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        resource: &Resource,
    ) -> anyhow::Result<()> {
        let features = &device.features;

        if features.gpu_process_info
            && let Ok(process_list) = device.handle.get_device_process_list()
        {
            const KEY: &str = "process_name";

            for process in process_list {
                let consumer = ResourceConsumer::Process { pid: process.pid };

                // Process path name
                let ascii = unsafe { CStr::from_ptr(process.name.as_ptr()) };
                let name = ascii.to_str().unwrap_or("");

                let mut push = |metric, value| {
                    if value != 0 {
                        measurements.push(
                            MeasurementPoint::new(timestamp, metric, resource.clone(), consumer.clone(), value)
                                .with_attr(KEY, name.to_string()),
                        );
                    }
                };

                push(self.metrics.process_memory_usage, process.mem);
                push(self.metrics.process_engine_usage_gfx, process.engine_usage.gfx);
                push(self.metrics.process_engine_usage_encode, process.engine_usage.enc);
                push(self.metrics.process_memory_usage_gtt, process.memory_usage.gtt_mem);
                push(self.metrics.process_memory_usage_cpu, process.memory_usage.cpu_mem);
                push(self.metrics.process_memory_usage_vram, process.memory_usage.vram_mem);
            }
        }

        Ok(())
    }
}

impl Source for AmdGpuSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        // No consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        for device in &self.devices {
            let resource = Resource::Gpu {
                bus_id: Cow::Owned(device.bus_id.clone()),
            };

            let features = &device.features;

            // GPU engine data usage metric pushed
            self.handle_gpu_activity(device, measurements, timestamp, &consumer, &resource)?;

            // GPU energy consumption metric pushed
            if features.gpu_energy_consumption
                && let Ok(res) = device.handle.get_device_energy_consumption()
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
                && device.handle.get_device_power_managment()?
                && let Ok(value) = device.handle.get_device_power_consumption()
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
            if features.gpu_voltage {
                const SENSOR: amdsmi_voltage_type_t = amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX;
                const METRIC: amdsmi_voltage_type_t = amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT;

                if let Ok(value) = device.handle.get_device_voltage(SENSOR, METRIC) {
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_voltage,
                        resource.clone(),
                        consumer.clone(),
                        value as u64,
                    ));
                }
            }

            // GPU memories used metric pushed
            for (mem_type, label) in &MEMORY_TYPE {
                if features
                    .gpu_memories_usage
                    .iter()
                    .find(|(m, _)| *m == *mem_type)
                    .map(|(_, v)| *v)
                    .unwrap_or(false)
                    && let Ok(value) = device.handle.get_device_memory_usage(*mem_type)
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
                    && let Ok(value) = device.handle.get_device_temperature(*sensor, METRIC_TEMP)
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

            // Push GPU compute-graphic process informations if processes existing
            self.handle_gpu_processes(device, measurements, timestamp, &resource)?;
        }

        Ok(())
    }
}
