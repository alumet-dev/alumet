use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use amd_smi_lib_sys::bindings::*;
use anyhow::Result;
use std::{borrow::Cow, ffi::CStr};

use super::{device::ManagedDevice, error::AmdError, features::OptionalFeatures, metrics::Metrics, utils::*};

/// Measurement source that queries AMD GPU devices.
pub struct AmdGpuSource {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counter: CounterDiff,
    /// Handle to the GPU, with features information.
    device: ManagedDevice,
    /// Alumet metrics IDs.
    metrics: Metrics,
    /// Alumet resource ID.
    resource: Resource,
}

/// SAFETY: The amd libary is thread-safe and returns pointers to a safe global state, which we can pass to other threads.
unsafe impl Send for ManagedDevice {}

impl AmdGpuSource {
    pub fn new(device: ManagedDevice, metrics: Metrics) -> Result<AmdGpuSource, amdsmi_status_t> {
        let bus_id = Cow::Owned(device.bus_id.clone());
        Ok(AmdGpuSource {
            energy_counter: CounterDiff::with_max_value(u64::MAX),
            device,
            metrics,
            resource: Resource::Gpu { bus_id },
        })
    }

    /// Retrieves and push all data concerning the activity utilization of a given AMD GPU device.
    fn handle_gpu_activity(
        &self,
        features: &OptionalFeatures,
        handle: amdsmi_processor_handle,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        consumer: ResourceConsumer,
    ) {
        if features.gpu_activity_usage
            && let Ok(value) = get_device_activity(handle)
        {
            const KEY: &str = "activity_type";

            let gfx = value.gfx_activity;
            let mm = value.mm_activity;
            let umc = value.umc_activity;

            if gfx != 0 {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_activity_usage,
                        self.resource.clone(),
                        consumer.clone(),
                        gfx as f64,
                    )
                    .with_attr(KEY, "graphic_core"),
                );
            }
            if mm != 0 {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_activity_usage,
                        self.resource.clone(),
                        consumer.clone(),
                        mm as f64,
                    )
                    .with_attr(KEY, "memory_management"),
                );
            }
            if umc != 0 {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_activity_usage,
                        self.resource.clone(),
                        consumer.clone(),
                        umc as f64,
                    )
                    .with_attr(KEY, "unified_memory_controller"),
                );
            }
        }
    }

    /// Retrieves and push all data concerning the running process ressources consumption of a given AMD GPU device.
    fn handle_gpu_processes(
        &self,
        features: &OptionalFeatures,
        handle: amdsmi_processor_handle,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) {
        if features.gpu_process_info
            && let Ok(process_list) = get_device_process_list(handle).map_err(AmdError)
        {
            const KEY: &str = "process_name";

            for process in process_list {
                let consumer = ResourceConsumer::Process { pid: process.pid };

                let mem = process.mem;
                let gfx = process.engine_usage.gfx;
                let enc = process.engine_usage.enc;
                let gtt_mem = process.memory_usage.gtt_mem;
                let cpu_mem = process.memory_usage.cpu_mem;
                let vram_mem = process.memory_usage.vram_mem;

                // Process path name
                let ascii = unsafe { CStr::from_ptr(process.name.as_ptr()) };
                let name = ascii.to_str().unwrap_or("");

                // Process memory usage
                if mem != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.process_memory_usage,
                            self.resource.clone(),
                            consumer.clone(),
                            process.mem,
                        )
                        .with_attr(KEY, name.to_string()),
                    );
                }

                // Process GFX engine usage
                if gfx != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.process_engine_usage_gfx,
                            self.resource.clone(),
                            consumer.clone(),
                            process.engine_usage.gfx,
                        )
                        .with_attr(KEY, name.to_string()),
                    );
                }
                // Process encode engine usage
                if enc != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.process_engine_usage_encode,
                            self.resource.clone(),
                            consumer.clone(),
                            process.engine_usage.enc,
                        )
                        .with_attr(KEY, name.to_string()),
                    );
                }

                // Process GTT memory usage
                if gtt_mem != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.process_memory_usage_gtt,
                            self.resource.clone(),
                            consumer.clone(),
                            process.memory_usage.gtt_mem,
                        )
                        .with_attr(KEY, name.to_string()),
                    );
                }
                // Process CPU memory usage
                if cpu_mem != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.process_memory_usage_cpu,
                            self.resource.clone(),
                            consumer.clone(),
                            process.memory_usage.cpu_mem,
                        )
                        .with_attr(KEY, name.to_string()),
                    );
                }
                // Process VRAM memory usage
                if vram_mem != 0 {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.process_memory_usage_vram,
                            self.resource.clone(),
                            consumer.clone(),
                            process.memory_usage.vram_mem,
                        )
                        .with_attr(KEY, name.to_string()),
                    );
                }
            }
        }
    }
}

impl Source for AmdGpuSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;
        let handle = self.device.handle;

        // No consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        // GPU engine data usage metric pushed
        self.handle_gpu_activity(features, handle, measurements, timestamp, consumer.clone());

        // GPU energy consumption metric pushed
        if features.gpu_energy_consumption
            && let Ok((energy, resolution, _timestamp)) = get_device_energy(handle).map_err(AmdError)
        {
            let diff = self.energy_counter.update(energy).difference();
            if let Some(value) = diff {
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_energy_consumption,
                    self.resource.clone(),
                    consumer.clone(),
                    (value as f64 * resolution as f64) / 1e3,
                ));
            }
        }

        // GPU instant electric power consumption metric pushed
        if features.gpu_power_consumption
            && get_device_power_managment(handle).map_err(AmdError)?
            && let Ok(value) = get_device_power(handle).map_err(AmdError)
        {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.gpu_power_consumption,
                self.resource.clone(),
                consumer.clone(),
                value.average_socket_power as u64,
            ));
        }

        // GPU instant electric power consumption metric pushed
        if features.gpu_voltage && get_device_power_managment(handle).map_err(AmdError)? {
            const SENSOR_TYPE: amdsmi_voltage_type_t = amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX;
            const METRIC: amdsmi_voltage_type_t = amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT;

            if let Ok(value) = get_device_voltage(handle, SENSOR_TYPE, METRIC).map_err(AmdError) {
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_voltage,
                    self.resource.clone(),
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
                .find(|(m, _)| (*m) == (*mem_type))
                .map(|(_, v)| *v)
                .unwrap_or(false)
                && let Ok(value) = get_device_memory_usage(handle, *mem_type)
            {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_memory_usages,
                        self.resource.clone(),
                        consumer.clone(),
                        value,
                    )
                    .with_attr("memory_type", label.to_string()),
                );
            }
        }

        // GPU temperatures metric pushed
        for (sensor, label) in &SENSOR_TYPE {
            const METRIC: amdsmi_temperature_metric_t = amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT;

            if features
                .gpu_temperatures
                .iter()
                .find(|(s, _)| (*s) == (*sensor))
                .map(|(_, v)| *v)
                .unwrap_or(false)
                && let Ok(value) = get_device_temperature(handle, *sensor, METRIC)
            {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_temperatures,
                        self.resource.clone(),
                        consumer.clone(),
                        value as u64,
                    )
                    .with_attr("thermal_zone", label.to_string()),
                );
            }
        }

        // Push GPU compute-graphic process informations if processes existing
        self.handle_gpu_processes(features, handle, measurements, timestamp);

        Ok(())
    }
}
