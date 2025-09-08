use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;
use std::borrow::Cow;

use rocm_smi_lib::{
    RocmErr, rsmi_temperature_metric_t_RSMI_TEMP_CURRENT, rsmi_voltage_metric_t_RSMI_VOLT_CURRENT,
    rsmi_voltage_type_t_RSMI_VOLT_TYPE_VDDGFX,
};

use crate::amd::features::OptionalFeatures;

use super::{
    device::ManagedDevice,
    features::{
        MEMORY_TYPE, SENSOR_TYPE, get_device_activity, get_device_compute_process_info, get_device_energy,
        get_device_memory_usage, get_device_power, get_device_temperature, get_device_voltage,
    },
    metrics::Metrics,
};

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

impl AmdGpuSource {
    pub fn new(device: ManagedDevice, metrics: Metrics) -> Result<AmdGpuSource, RocmErr> {
        let bus_id = Cow::Owned(device.bus_id.to_string());
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
        dv_ind: u32,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        consumer: ResourceConsumer,
    ) {
        if features.gpu_activity
            && let Ok((gfx, mm, umc)) = get_device_activity(dv_ind)
        {
            const KEY: &str = "activity_type";

            if gfx != 0 {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_activity,
                        self.resource.clone(),
                        consumer.clone(),
                        gfx as u64,
                    )
                    .with_attr(KEY, "graphic_core"),
                );
            }
            if mm != 0 {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_activity,
                        self.resource.clone(),
                        consumer.clone(),
                        mm as u64,
                    )
                    .with_attr(KEY, "memory_management"),
                );
            }
            if umc != 0 {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_activity,
                        self.resource.clone(),
                        consumer.clone(),
                        umc as u64,
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
        dv_ind: u32,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) {
        if features.gpu_process_info
            && let Ok(process_list) = get_device_compute_process_info(dv_ind)
        {
            for process in process_list {
                let consumer = ResourceConsumer::Process {
                    pid: process.process_id,
                };

                let vram = process.vram_usage;
                let sdma = process.sdma_usage;
                let occupancy = process.cu_occupancy;

                if vram != 0 {
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.process_compute_unit_usage,
                        self.resource.clone(),
                        consumer.clone(),
                        vram,
                    ));
                }
                if sdma != 0 {
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.process_memory_usage_vram,
                        self.resource.clone(),
                        consumer.clone(),
                        sdma,
                    ));
                }
                if occupancy != 0 {
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        self.metrics.process_sdma_usage,
                        self.resource.clone(),
                        consumer.clone(),
                        occupancy as u64,
                    ));
                }
            }
        }
    }
}

impl Source for AmdGpuSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;
        let dv_ind = self.device.identifier;

        // No consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        // GPU activity utilization metric pushed
        self.handle_gpu_activity(features, dv_ind, measurements, timestamp, consumer.clone());

        // GPU energy consumption metric pushed
        if features.gpu_energy_consumption
            && let Ok((energy, resolution, _timestamp)) = get_device_energy(dv_ind)
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
            && let Ok(value) = get_device_power(dv_ind)
        {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.gpu_power_consumption,
                self.resource.clone(),
                consumer.clone(),
                value / 1_000_000,
            ));
        }

        // GPU instant voltage consumption metric pushed
        if features.gpu_voltage_consumption
            && let Ok(value) = get_device_voltage(
                dv_ind,
                rsmi_voltage_type_t_RSMI_VOLT_TYPE_VDDGFX,
                rsmi_voltage_metric_t_RSMI_VOLT_CURRENT,
            )
        {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.gpu_voltage_consumption,
                self.resource.clone(),
                consumer.clone(),
                value as f64 / 1e3,
            ));
        }

        // GPU memories used metric pushed
        for (mem_type, label) in &MEMORY_TYPE {
            if features
                .gpu_memories_usage
                .iter()
                .find(|(m, _)| (*m) == (*mem_type))
                .map(|(_, v)| *v)
                .unwrap_or(false)
                && let Ok(value) = get_device_memory_usage(dv_ind, *mem_type)
            {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_memories_usage,
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
            if features
                .gpu_temperatures
                .iter()
                .find(|(s, _)| (*s) == (*sensor))
                .map(|(_, v)| *v)
                .unwrap_or(false)
                && let Ok(value) = get_device_temperature(dv_ind, *sensor, rsmi_temperature_metric_t_RSMI_TEMP_CURRENT)
            {
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.gpu_temperatures,
                        self.resource.clone(),
                        consumer.clone(),
                        value as u64 / 1_000,
                    )
                    .with_attr("thermal_zone", label.to_string()),
                );
            }
        }

        // Push GPU compute-graphic process informations if processes existing
        self.handle_gpu_processes(features, dv_ind, measurements, timestamp);

        Ok(())
    }
}
