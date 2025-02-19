use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{elements::error::PollError, Source},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use amdsmi::{
    amdsmi_get_energy_count, amdsmi_get_gpu_activity, amdsmi_get_gpu_memory_usage, amdsmi_get_power_info,
    amdsmi_get_temp_metric, amdsmi_is_gpu_power_management_enabled,
    AmdsmiTemperatureMetricT, AmdsmiStatusT,
};

use super::{features::{MEMORY_TYPE, SENSOR_TYPE}, device::ManagedDevice, error::AmdError, metrics::Metrics};

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

/// The GPU device handler cannot be sent between threads safely
unsafe impl Send for AmdGpuSource {}

impl AmdGpuSource {
    pub fn new(device: ManagedDevice, metrics: Metrics) -> Result<AmdGpuSource, AmdsmiStatusT> {
        let bus_id = std::borrow::Cow::Owned(device.bus_id.clone());
        Ok(AmdGpuSource {
            energy_counter: CounterDiff::with_max_value(u64::MAX),
            device,
            metrics,
            resource: Resource::Gpu { bus_id },
        })
    }
}

impl Source for AmdGpuSource {
    fn poll(&mut self, measurement: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;
        let device = self.device.as_wrapper();
        let consumer = ResourceConsumer::LocalMachine;

        // GPU energy consumption metric pushed
        if features.gpu_energy_consumption {
            let (energy, resolution, _timestamp) = amdsmi_get_energy_count(device.as_ptr()).map_err(AmdError)?;
            let diff = self.energy_counter.update((energy * resolution as u64) / 1_000).difference();
            if let Some(value) = diff {
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_energy_consumption,
                    self.resource.clone(),
                    consumer.clone(),
                    value,
                ));
            }
        }

        // GPU engine data usage metric pushed
        if features.gpu_engine_usage
            && let Ok(value) = amdsmi_get_gpu_activity(device.as_ptr()).map_err(AmdError) {
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_engine_usage,
                    self.resource.clone(),
                    consumer.clone(),
                    value.gfx_activity as f64,
                ));
        }

        // GPU electric average power consumption metric pushed
        if features.gpu_power_consumption
            && amdsmi_is_gpu_power_management_enabled(device.as_ptr()).map_err(AmdError)?
                && let Ok(value) = amdsmi_get_power_info(device.as_ptr()).map_err(AmdError) {
                    measurement.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_power_consumption,
                        self.resource.clone(),
                        consumer.clone(),
                        value.current_socket_power as u64,
                    ));
        }

        // GPU memories used metric pushed
        for (mem, label) in &MEMORY_TYPE {
            if *features.gpu_memory_usages.get(mem).unwrap_or(&false)
                && let Ok(value) = amdsmi_get_gpu_memory_usage(device.as_ptr(), *mem).map_err(AmdError) {
                    measurement.push(
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
            if *features.gpu_temperatures.get(sensor).unwrap_or(&false)
                && let Ok(value) = amdsmi_get_temp_metric(device.as_ptr(), *sensor, AmdsmiTemperatureMetricT::AmdsmiTempCurrent).map_err(AmdError) {
                    measurement.push(
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
        if features.gpu_process_info
            && let Ok(process_list) = amdsmi::amdsmi_get_gpu_process_list(device.as_ptr()).map_err(AmdError) {
            for process in process_list {
                let consumer = ResourceConsumer::Process {
                    pid: process.pid,
                };

                // Process memory usage
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_memory_usage,
                    self.resource.clone(),
                    consumer.clone(),
                    process.mem,
                ));

                // Process GFX engine usage
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_engine_usage_gfx,
                    self.resource.clone(),
                    consumer.clone(),
                    process.engine_usage.gfx,
                ));
                // Process encode engine usage
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_engine_usage_encode,
                    self.resource.clone(),
                    consumer.clone(),
                    process.engine_usage.enc,
                ));

                // Process GTT memory usage
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_memory_usage_gtt,
                    self.resource.clone(),
                    consumer.clone(),
                    process.memory_usage.gtt_mem,
                ));
                // Process CPU memory usage
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_memory_usage_cpu,
                    self.resource.clone(),
                    consumer.clone(),
                    process.memory_usage.cpu_mem,
                ));
                // Process VRAM memory usage
                measurement.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_memory_usage_vram,
                    self.resource.clone(),
                    consumer.clone(),
                    process.memory_usage.vram_mem,
                ));
            }
        }

        Ok(())
    }
}
