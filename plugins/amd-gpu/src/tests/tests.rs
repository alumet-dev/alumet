use crate::{
    AmdGpuPlugin, Config,
    amd::utils::{
        METRIC_LABEL_ACTIVITY, METRIC_LABEL_ENERGY, METRIC_LABEL_MEMORY, METRIC_LABEL_POWER, METRIC_LABEL_PROCESS_CPU,
        METRIC_LABEL_PROCESS_ENCODE, METRIC_LABEL_PROCESS_GFX, METRIC_LABEL_PROCESS_GTT, METRIC_LABEL_PROCESS_MEMORY,
        METRIC_LABEL_PROCESS_VRAM, METRIC_LABEL_TEMPERATURE, METRIC_LABEL_VOLTAGE, METRIC_TEMP, PLUGIN_NAME,
        UNEXPECTED_DATA, UNKNOWN_ERROR,
    },
    tests::mocks::{
        MOCK_ACTIVITY, MOCK_ENERGY, MOCK_MEMORY, MOCK_POWER, MOCK_PROCESS, MOCK_SOURCE_NAME, MOCK_TEMPERATURE,
        MOCK_UUID, MOCK_VOLTAGE,
    },
};
use amd_smi_wrapper::{AmdError, MockAmdInterface, MockProcessorHandle, MockSocketHandle};
use std::{thread::sleep, time::Duration};

use alumet::{
    agent::{
        self, Builder,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::WrappedMeasurementValue,
    pipeline::naming::SourceName,
    resources::Resource,
    test::{RuntimeExpectations, StartupExpectations},
    units::{PrefixedUnit, Unit},
};

// Mock fake toml table for configuration
#[cfg(test)]
fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

// Test to start AMD GPU plugin integration in ALUMET with available GPU device and metrics
#[test]
fn test_start_success() {
    let mut mock_init = MockAmdInterface::new();

    mock_init.expect_socket_handles().returning(|| {
        let mut mock_socket = MockSocketHandle::new();
        mock_socket.expect_processor_handles().returning(|| {
            let mut mock_processor = MockProcessorHandle::new();

            mock_processor
                .expect_device_uuid()
                .returning(|| Ok(MOCK_UUID.to_owned()));

            mock_processor.expect_device_activity().returning(|| Ok(MOCK_ACTIVITY));

            mock_processor
                .expect_device_energy_consumption()
                .returning(|| Ok(MOCK_ENERGY));

            mock_processor
                .expect_device_power_consumption()
                .returning(|| Ok(MOCK_POWER));

            mock_processor.expect_device_power_managment().returning(|| Ok(true));

            mock_processor
                .expect_device_process_list()
                .returning(|| Ok(vec![MOCK_PROCESS]));

            mock_processor
                .expect_device_voltage()
                .returning(|_, _| Ok(MOCK_VOLTAGE));

            mock_processor.expect_device_memory_usage().returning(|mem_type| {
                MOCK_MEMORY
                    .iter()
                    .find(|(t, _)| *t == mem_type)
                    .map(|(_, v)| Ok(*v))
                    .unwrap_or(Err(AmdError(UNEXPECTED_DATA)))
            });

            mock_processor.expect_device_temperature().returning(|sensor, metric| {
                if metric != METRIC_TEMP {
                    return Err(AmdError(UNEXPECTED_DATA));
                }

                MOCK_TEMPERATURE
                    .iter()
                    .find(|(s, _)| *s == sensor)
                    .map(|(_, v)| Ok(*v))
                    .unwrap_or(Err(AmdError(UNEXPECTED_DATA)))
            });

            Ok(vec![mock_processor])
        });

        Ok(vec![mock_socket])
    });

    mock_init.expect_stop().returning(|| Ok(()));

    let config = Config { ..Default::default() };
    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: AmdGpuPlugin::test_metadata(mock_init),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new()
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
        .expect_source(PLUGIN_NAME, MOCK_SOURCE_NAME);

    let runtime_expectation = RuntimeExpectations::new().test_source(
        SourceName::from_str(PLUGIN_NAME, MOCK_SOURCE_NAME),
        || {},
        |ctx| {
            let m = ctx.measurements();
            let metric = |name| ctx.metrics().by_name(name).unwrap().0;
            let gpu_points = |metric| {
                m.iter().filter(move |p| {
                    p.metric == metric
                        && matches!(
                            &p.resource,
                            Resource::Gpu { bus_id } if bus_id.as_ref() == MOCK_UUID
                        )
                })
            };

            // GPU activity usage
            {
                let metric = metric(METRIC_LABEL_ACTIVITY);
                let points: Vec<_> = gpu_points(metric).collect();

                for p in points {
                    let attr_type = p
                        .attributes()
                        .find(|(k, _)| *k == "activity_type")
                        .expect("Missing activity_type attribute")
                        .1
                        .to_string();

                    match attr_type.as_str() {
                        "graphic_core" => {
                            assert_eq!(p.value, WrappedMeasurementValue::F64(MOCK_ACTIVITY.gfx_activity as f64))
                        }
                        "memory_management" => {
                            assert_eq!(p.value, WrappedMeasurementValue::F64(MOCK_ACTIVITY.mm_activity as f64))
                        }
                        "unified_memory_controller" => {
                            assert_eq!(p.value, WrappedMeasurementValue::F64(MOCK_ACTIVITY.umc_activity as f64))
                        }
                        e => panic!("Unexpected type {e}"),
                    }
                }
            }

            // GPU memory usage
            {
                let metric = metric(METRIC_LABEL_MEMORY);
                let points: Vec<_> = gpu_points(metric).collect();

                for p in points {
                    let mem_type = p
                        .attributes()
                        .find(|(k, _)| *k == "memory_type")
                        .expect("Missing memory_type attribute")
                        .1
                        .to_string();

                    match mem_type.as_str() {
                        "memory_graphic_translation_table" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_MEMORY[1].1 as u64))
                        }
                        "memory_video_computing" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_MEMORY[0].1 as u64))
                        }
                        e => panic!("Unexpected type {e}"),
                    }
                }
            }

            // GPU temperatures
            {
                let metric = metric(METRIC_LABEL_TEMPERATURE);
                let points: Vec<_> = gpu_points(metric).collect();

                for p in points {
                    let zone = p
                        .attributes()
                        .find(|(k, _)| *k == "thermal_zone")
                        .expect("Missing thermal_zone attribute")
                        .1
                        .to_string();

                    match zone.as_str() {
                        "thermal_global" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[0].1 as u64))
                        }
                        "thermal_hotspot" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[1].1 as u64))
                        }
                        "thermal_high_bandwidth_memory_0" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[2].1 as u64))
                        }
                        "thermal_high_bandwidth_memory_1" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[3].1 as u64))
                        }
                        "thermal_high_bandwidth_memory_2" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[4].1 as u64))
                        }
                        "thermal_high_bandwidth_memory_3" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[5].1 as u64))
                        }
                        "thermal_pci_bus" => {
                            assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_TEMPERATURE[6].1 as u64))
                        }
                        e => panic!("Unexpected type {e}"),
                    }
                }
            }

            // GPU energy consumption
            {
                let metric = metric(METRIC_LABEL_ENERGY);
                if let Some(p) = gpu_points(metric).next() {
                    assert_eq!(p.value, WrappedMeasurementValue::F64(MOCK_ENERGY.energy as f64));
                }
            }

            // GPU power consumption
            {
                let metric = metric(METRIC_LABEL_POWER);
                let p = gpu_points(metric).next().expect("Unexpected value");
                assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_POWER.socket_power as u64));
            }

            // GPU voltage
            {
                let metric = metric(METRIC_LABEL_VOLTAGE);
                let p = gpu_points(metric).next().expect("Unexpected value");
                assert_eq!(p.value, WrappedMeasurementValue::U64(MOCK_VOLTAGE as u64));
            }

            // GPU processes informations
            {
                let expected_names = ["p1"];
                let process_metrics = [
                    METRIC_LABEL_PROCESS_MEMORY,
                    METRIC_LABEL_PROCESS_ENCODE,
                    METRIC_LABEL_PROCESS_GFX,
                    METRIC_LABEL_PROCESS_GTT,
                    METRIC_LABEL_PROCESS_CPU,
                    METRIC_LABEL_PROCESS_VRAM,
                ];

                for &metric_id in &process_metrics {
                    let metric = metric(metric_id);
                    let points: Vec<_> = gpu_points(metric).collect();

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
                            ("p1", METRIC_LABEL_PROCESS_MEMORY) => WrappedMeasurementValue::U64(MOCK_PROCESS.mem),
                            ("p1", METRIC_LABEL_PROCESS_ENCODE) => {
                                WrappedMeasurementValue::U64(MOCK_PROCESS.engine_usage.enc)
                            }
                            ("p1", METRIC_LABEL_PROCESS_GFX) => {
                                WrappedMeasurementValue::U64(MOCK_PROCESS.engine_usage.gfx)
                            }
                            ("p1", METRIC_LABEL_PROCESS_GTT) => {
                                WrappedMeasurementValue::U64(MOCK_PROCESS.memory_usage.gtt_mem)
                            }
                            ("p1", METRIC_LABEL_PROCESS_CPU) => {
                                WrappedMeasurementValue::U64(MOCK_PROCESS.memory_usage.cpu_mem)
                            }
                            ("p1", METRIC_LABEL_PROCESS_VRAM) => {
                                WrappedMeasurementValue::U64(MOCK_PROCESS.memory_usage.vram_mem)
                            }
                            e => panic!("Unexpected type and metrics: {e:?}"),
                        };

                        assert_eq!(p.value, expected_value);
                    }
                }
            }

            sleep(Duration::from_secs(1))
        },
    );

    let agent = Builder::new(plugins)
        .with_expectations(startup_expectation)
        .with_expectations(runtime_expectation)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();
}

// Test to start AMD GPU plugin integration in ALUMET without GPU device
#[test]
fn test_start_error() {
    let mut mock_init = MockAmdInterface::new();
    mock_init.expect_socket_handles().returning(|| Ok(vec![]));
    mock_init.expect_stop().returning(|| Ok(()));

    let mut plugins = PluginSet::new();
    let config = Config { ..Default::default() };

    plugins.add_plugin(PluginInfo {
        metadata: AmdGpuPlugin::test_metadata(mock_init),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let agent = agent::Builder::new(plugins).build_and_start();

    assert!(agent.is_err())
}

// Test to start AMD GPU plugin integration in ALUMET with GPU device detected but no metrics available
#[test]
fn test_start_success_without_stats() {
    let mut mock_init = MockAmdInterface::new();

    mock_init.expect_socket_handles().returning(|| {
        let mut mock_socket = MockSocketHandle::new();
        mock_socket.expect_processor_handles().returning(|| {
            let mut mock_processor = MockProcessorHandle::new();
            mock_processor
                .expect_device_uuid()
                .returning(|| Ok(MOCK_UUID.to_owned()));

            mock_processor
                .expect_device_activity()
                .returning(|| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_energy_consumption()
                .returning(|| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_power_consumption()
                .returning(|| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_power_managment()
                .returning(|| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_process_list()
                .returning(|| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_voltage()
                .returning(|_, _| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_memory_usage()
                .returning(|_| Err(AmdError(UNKNOWN_ERROR)));
            mock_processor
                .expect_device_temperature()
                .returning(|_, _| Err(AmdError(UNKNOWN_ERROR)));

            Ok(vec![mock_processor])
        });

        Ok(vec![mock_socket])
    });

    mock_init.expect_stop().returning(|| Ok(()));

    let config = Config { ..Default::default() };
    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: AmdGpuPlugin::test_metadata(mock_init),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let agent = Builder::new(plugins).build_and_start();

    assert!(agent.is_err())
}
