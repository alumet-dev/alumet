use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;
use std::{borrow::Cow, ffi::CStr};

use super::{device::ManagedDevice, features::OptionalFeatures, metrics::Metrics};
use crate::{
    amd::utils::{MEMORY_TYPE, METRIC_TEMP, SENSOR_TYPE},
    bindings::*,
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
    pub fn handle_gpu_activity(
        &self,
        features: &OptionalFeatures,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
        consumer: ResourceConsumer,
    ) -> anyhow::Result<()> {
        Ok(
            if features.gpu_activity_usage
                && let Ok(value) = self.device.handle.get_device_activity()
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
            },
        )
    }

    /// Retrieves and push all data concerning the running process ressources consumption of a given AMD GPU device.
    fn handle_gpu_processes(
        &self,
        features: &OptionalFeatures,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) -> anyhow::Result<()> {
        Ok(
            if features.gpu_process_info
                && let Ok(process_list) = self.device.handle.get_device_process_list()
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
            },
        )
    }
}

impl Source for AmdGpuSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;

        // No consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        // GPU engine data usage metric pushed
        self.handle_gpu_activity(features, measurements, timestamp, consumer.clone())?;

        // GPU energy consumption metric pushed
        if features.gpu_energy_consumption
            && let Ok((energy, resolution, _timestamp)) = self.device.handle.get_device_energy_consumption()
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
            && self.device.handle.get_device_power_managment()?
            && let Ok(value) = self.device.handle.get_device_power_consumption()
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
        if features.gpu_voltage {
            const SENSOR_TYPE: amdsmi_voltage_type_t = amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX;
            const METRIC: amdsmi_voltage_type_t = amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT;

            if let Ok(value) = self.device.handle.get_device_voltage(SENSOR_TYPE, METRIC) {
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
                && let Ok(value) = self.device.handle.get_device_memory_usage(*mem_type)
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
            if features
                .gpu_temperatures
                .iter()
                .find(|(s, _)| (*s) == (*sensor))
                .map(|(_, v)| *v)
                .unwrap_or(false)
                && let Ok(value) = self.device.handle.get_device_temperature(*sensor, METRIC_TEMP)
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
        self.handle_gpu_processes(features, measurements, timestamp)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests_probe {
    use crate::{
        AmdGpuPlugin, Config,
        amd::utils::{
            METRIC_LABEL_ACTIVITY, METRIC_LABEL_ENERGY, METRIC_LABEL_MEMORY, METRIC_LABEL_POWER,
            METRIC_LABEL_PROCESS_CPU, METRIC_LABEL_PROCESS_ENCODE, METRIC_LABEL_PROCESS_GFX, METRIC_LABEL_PROCESS_GTT,
            METRIC_LABEL_PROCESS_MEMORY, METRIC_LABEL_PROCESS_VRAM, METRIC_LABEL_TEMPERATURE, METRIC_LABEL_VOLTAGE,
            PLUGIN_NAME,
        },
        tests::mocks::tests_mocks::{MOCK_ENERGY, MOCK_UUID, config_to_toml_table},
    };
    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        measurement::WrappedMeasurementValue,
        pipeline::naming::SourceName,
        plugin::PluginMetadata,
        test::{RuntimeExpectations, StartupExpectations},
        units::{PrefixedUnit, Unit},
    };
    use std::{panic, time::Duration};

    // Test `start` function for amd-gpu plugin metric collect with correct values
    #[test]
    fn test_start_success() {
        let mut plugins = PluginSet::new();
        let config = Config { ..Default::default() };

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectations = StartupExpectations::new()
            .expect_metric::<f64>(METRIC_LABEL_ACTIVITY, Unit::Percent.clone())
            .expect_metric::<f64>(METRIC_LABEL_ENERGY, PrefixedUnit::milli(Unit::Joule))
            .expect_metric::<u64>(METRIC_LABEL_MEMORY, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_POWER, Unit::Watt.clone())
            .expect_metric::<u64>(METRIC_LABEL_TEMPERATURE, Unit::DegreeCelsius.clone())
            .expect_metric::<u64>(METRIC_LABEL_VOLTAGE, PrefixedUnit::milli(Unit::Volt))
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_MEMORY, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_ENCODE, PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_GFX, PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_GTT, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_CPU, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_VRAM, Unit::Byte.clone())
            .expect_source(PLUGIN_NAME, MOCK_UUID);

        let source = SourceName::from_str(PLUGIN_NAME, MOCK_UUID);

        let run_expect = RuntimeExpectations::new().test_source(
            source.clone(),
            || {},
            |ctx| {
                let m = ctx.measurements();
                let metrics = ctx.metrics();
                let get_metric = |name| metrics.by_name(name).unwrap().0;

                // GPU activity usage
                {
                    let metric = get_metric(METRIC_LABEL_ACTIVITY);
                    let points: Vec<_> = m.iter().filter(|p| p.metric == metric).collect();

                    for p in points {
                        let attr_type = p
                            .attributes()
                            .find(|(k, _)| *k == "activity_type")
                            .expect("Missing attribute type")
                            .1
                            .to_string();

                        match attr_type.as_str() {
                            "graphic_core" => assert_eq!(p.value, WrappedMeasurementValue::U64(131072)),
                            "memory_management" => assert_eq!(p.value, WrappedMeasurementValue::U64(262144)),
                            "unified_memory_controller" => assert_eq!(p.value, WrappedMeasurementValue::U64(524288)),
                            e => panic!("Unexpected type {e}"),
                        }
                    }
                }

                // GPU energy consumption
                {
                    let metric = get_metric(METRIC_LABEL_ENERGY);
                    let p = m.iter().find(|p| p.metric == metric).unwrap();
                    assert_eq!(p.value, WrappedMeasurementValue::F64(MOCK_ENERGY as f64));
                }

                // GPU memory usage
                {
                    let metric = get_metric(METRIC_LABEL_MEMORY);
                    let points: Vec<_> = m.iter().filter(|p| p.metric == metric).collect();

                    for p in points {
                        let attr_type = p
                            .attributes()
                            .find(|(k, _)| *k == "memory_type")
                            .expect("Missing attribute type")
                            .1
                            .to_string();

                        match attr_type.as_str() {
                            "memory_graphic_translation_table" => {
                                assert_eq!(p.value, WrappedMeasurementValue::U64(131072))
                            }
                            "memory_video_computing" => assert_eq!(p.value, WrappedMeasurementValue::U64(262144)),
                            e => panic!("Unexpected type {e}"),
                        }
                    }
                }

                // GPU power consumption
                {
                    let metric = get_metric(METRIC_LABEL_POWER);
                    let p = m.iter().find(|p| p.metric == metric).unwrap();
                    assert_eq!(p.value, WrappedMeasurementValue::U64(43));
                }

                // GPU temperatures
                {
                    let metric = get_metric(METRIC_LABEL_TEMPERATURE);
                    let points: Vec<_> = m.iter().filter(|p| p.metric == metric).collect();

                    for p in points {
                        let attr_type = p
                            .attributes()
                            .find(|(k, _)| *k == "sensor_type")
                            .expect("Missing attribute type")
                            .1
                            .to_string();

                        match attr_type.as_str() {
                            "thermal_global" => assert_eq!(p.value, WrappedMeasurementValue::U64(45)),
                            "thermal_hotspot" => assert_eq!(p.value, WrappedMeasurementValue::U64(46)),
                            "thermal_high_bandwidth_memory_0" => assert_eq!(p.value, WrappedMeasurementValue::U64(47)),
                            "thermal_high_bandwidth_memory_1" => assert_eq!(p.value, WrappedMeasurementValue::U64(48)),
                            "thermal_high_bandwidth_memory_2" => assert_eq!(p.value, WrappedMeasurementValue::U64(49)),
                            "thermal_high_bandwidth_memory_3" => assert_eq!(p.value, WrappedMeasurementValue::U64(50)),
                            "thermal_pci_bus" => assert_eq!(p.value, WrappedMeasurementValue::U64(51)),
                            e => panic!("Unexpected type {e}"),
                        }
                    }
                }

                // GPU voltage consumption
                {
                    let metric = get_metric(METRIC_LABEL_VOLTAGE);
                    let p = m.iter().find(|p| p.metric == metric).unwrap();
                    assert_eq!(p.value, WrappedMeasurementValue::U64(830));
                }

                // GPU processes informations
                {
                    let expected_names = "p1";
                    let process_metrics = [
                        METRIC_LABEL_PROCESS_MEMORY,
                        METRIC_LABEL_PROCESS_ENCODE,
                        METRIC_LABEL_PROCESS_GFX,
                        METRIC_LABEL_PROCESS_GTT,
                        METRIC_LABEL_PROCESS_CPU,
                        METRIC_LABEL_PROCESS_VRAM,
                    ];

                    for &metric_id in &process_metrics {
                        let metric = get_metric(metric_id);
                        let points: Vec<_> = m.iter().filter(|p| p.metric == metric).collect();

                        for p in points {
                            let process_name = p
                                .attributes()
                                .find(|(k, _)| k == &"process_name")
                                .expect("Missing attribute type")
                                .1
                                .to_string();

                            assert!(
                                expected_names.contains(&process_name.as_str()),
                                "Unexpected {process_name}"
                            );

                            let expected_value = match (process_name.as_str(), metric_id) {
                                ("p1", METRIC_LABEL_PROCESS_MEMORY) => WrappedMeasurementValue::U64(32),
                                ("p1", METRIC_LABEL_PROCESS_ENCODE) => WrappedMeasurementValue::U64(64),
                                ("p1", METRIC_LABEL_PROCESS_GFX) => WrappedMeasurementValue::U64(5135145681),
                                ("p1", METRIC_LABEL_PROCESS_GTT) => WrappedMeasurementValue::U64(256),
                                ("p1", METRIC_LABEL_PROCESS_CPU) => WrappedMeasurementValue::U64(1024),
                                ("p1", METRIC_LABEL_PROCESS_VRAM) => WrappedMeasurementValue::U64(2048),

                                e => panic!("Unexpected type and metrics: {e:?}"),
                            };

                            assert_eq!(p.value, expected_value);
                        }
                    }
                }
            },
        );

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .with_expectations(run_expect)
            .build_and_start()
            .unwrap();

        // Send shutdown message
        agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();
    }
}
