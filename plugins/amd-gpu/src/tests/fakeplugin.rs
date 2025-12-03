#[cfg(test)]
mod tests_integration {
    use crate::bindings::{
        amdsmi_engine_usage_t, amdsmi_power_info_t, amdsmi_proc_info_t, amdsmi_processor_handle, amdsmi_status_t,
        amdsmi_status_t_AMDSMI_STATUS_SUCCESS,
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
    use std::{mem::zeroed, panic, time::Duration};

    use crate::{
        AmdGpuPlugin, Config,
        tests::ffi_mock::{
            ffi_mocks_activity_usage::set_mock_activity_usage,
            ffi_mocks_energy_consumption::set_mock_energy_consumption, ffi_mocks_memory_usage::set_mock_memory_usage,
            ffi_mocks_power_consumption::set_mock_power_consumption,
            ffi_mocks_power_management_status::set_mock_power_management_status,
            ffi_mocks_process_list::set_mock_process_list, ffi_mocks_processor_handles::set_mock_processor_handles,
            ffi_mocks_socket_handles::set_mock_socket_handles, ffi_mocks_temperature::set_mock_temperature,
            ffi_mocks_uuid::set_mock_uuid, ffi_mocks_voltage_consumption::set_mock_voltage_consumption,
        },
    };

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const PLUGIN: &str = "amd-gpu";
    const SOURCE: &str = "a4ff740f-0000-1000-80ea-e05c945bb3b2";
    const TIMESTAMP: u64 = 1708236479191334820;

    const ACTIVITY: &str = "amd_gpu_activity_usage";
    const ENERGY: &str = "amd_gpu_energy_consumption";
    const MEMORY: &str = "amd_gpu_memory_usage";
    const POWER: &str = "amd_gpu_power_consumption";
    const TEMPERATURE: &str = "amd_gpu_temperature";
    const VOLTAGE: &str = "amd_gpu_voltage";
    const PROCESS_MEMORY: &str = "amd_gpu_process_memory_usage";
    const PROCESS_ENCODE: &str = "amd_gpu_process_engine_usage_encode";
    const PROCESS_GFX: &str = "amd_gpu_process_engine_gfx";
    const PROCESS_GTT: &str = "amd_gpu_process_memory_usage_gtt";
    const PROCESS_CPU: &str = "amd_gpu_process_memory_usage_cpu";
    const PROCESS_VRAM: &str = "amd_gpu_process_memory_usage_vram";

    // Mock fake toml table for configuration
    fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    // Function for process name with ascii bytes encoding
    fn set_name(buffer: &mut [i8; 256], name: &str) {
        buffer.fill(0);
        for (i, &b) in name.as_bytes().iter().enumerate() {
            buffer[i] = b as i8;
        }
    }

    // Test `start` function for amd-gpu plugin metric collect without device
    #[test]
    fn test_start_error() {
        let mut plugins = PluginSet::new();
        let config = Config { ..Default::default() };

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectation = StartupExpectations::new().expect_source(PLUGIN, SOURCE);

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectation)
            .build_and_start();

        assert!(agent.is_err())
    }

    // Test `start` function for amd-gpu plugin metric collect with correct values
    #[test]
    fn test_start_success() {
        let mut power: amdsmi_power_info_t = unsafe { zeroed() };
        let mut activity: amdsmi_engine_usage_t = unsafe { zeroed() };
        let mut process_1: amdsmi_proc_info_t = unsafe { zeroed() };
        let mut process_2: amdsmi_proc_info_t = unsafe { zeroed() };

        power.current_socket_power = 43;
        power.average_socket_power = 40;

        activity.gfx_activity = 34;
        activity.mm_activity = 12;
        activity.umc_activity = 56;

        set_name(&mut process_1.name, "p1");
        process_1.pid = 1;
        process_1.mem = 32;
        process_1.engine_usage.enc = 64;
        process_1.engine_usage.gfx = 128;
        process_1.memory_usage.gtt_mem = 256;
        process_1.memory_usage.cpu_mem = 1024;
        process_1.memory_usage.vram_mem = 2048;

        set_name(&mut process_2.name, "p2");
        process_2.pid = 2;
        process_2.mem = 4096;
        process_2.engine_usage.enc = 8192;
        process_2.engine_usage.gfx = 16384;
        process_2.memory_usage.gtt_mem = 32768;
        process_2.memory_usage.cpu_mem = 65536;
        process_2.memory_usage.vram_mem = 131072;

        //set_mock_init(SUCCESS);

        set_mock_socket_handles(1, SUCCESS, SUCCESS);
        set_mock_processor_handles(vec![0 as amdsmi_processor_handle], SUCCESS, SUCCESS);

        set_mock_uuid(
            SOURCE.bytes().map(|b| b as i8).chain(std::iter::once(0)).collect(),
            SUCCESS,
        );

        set_mock_activity_usage(SUCCESS, activity);
        set_mock_energy_consumption(123456789, 0.5, TIMESTAMP, SUCCESS);
        set_mock_power_management_status(true, SUCCESS);
        set_mock_power_consumption(power, SUCCESS);
        set_mock_voltage_consumption(830, SUCCESS);
        set_mock_memory_usage(13443072, SUCCESS);
        set_mock_temperature(52, SUCCESS);
        set_mock_process_list(vec![process_1, process_2], SUCCESS);

        let mut plugins = PluginSet::new();
        let config = Config { ..Default::default() };

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectations = StartupExpectations::new()
            .expect_metric::<f64>(ACTIVITY, Unit::Percent.clone())
            .expect_metric::<f64>(ENERGY, PrefixedUnit::milli(Unit::Joule))
            .expect_metric::<u64>(MEMORY, Unit::Byte.clone())
            .expect_metric::<u64>(POWER, Unit::Watt.clone())
            .expect_metric::<u64>(TEMPERATURE, Unit::DegreeCelsius.clone())
            .expect_metric::<u64>(VOLTAGE, PrefixedUnit::milli(Unit::Volt))
            .expect_metric::<u64>(PROCESS_MEMORY, Unit::Byte.clone())
            .expect_metric::<u64>(PROCESS_ENCODE, PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>(PROCESS_GFX, PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>(PROCESS_GTT, Unit::Byte.clone())
            .expect_metric::<u64>(PROCESS_CPU, Unit::Byte.clone())
            .expect_metric::<u64>(PROCESS_VRAM, Unit::Byte.clone())
            .expect_source(PLUGIN, SOURCE);

        let source = SourceName::from_str(PLUGIN, SOURCE);

        let run_expect = RuntimeExpectations::new().test_source(
            source.clone(),
            || {},
            |ctx| {
                let m = ctx.measurements();
                let metrics = ctx.metrics();
                let get_metric = |name| metrics.by_name(name).unwrap().0;

                // GPU activity usage
                {
                    let metric = get_metric(ACTIVITY);
                    let points: Vec<_> = m.iter().filter(|p| p.metric == metric).collect();

                    for p in points {
                        let attr_type = p
                            .attributes()
                            .find(|(k, _)| *k == "activity_type")
                            .expect("Missing attribute type")
                            .1
                            .to_string();

                        match attr_type.as_str() {
                            "graphic_core" => assert_eq!(p.value, WrappedMeasurementValue::U64(34)),
                            "memory_management" => assert_eq!(p.value, WrappedMeasurementValue::U64(12)),
                            "unified_memory_controller" => assert_eq!(p.value, WrappedMeasurementValue::U64(56)),
                            e => panic!("Unexpected type {e}"),
                        }
                    }
                }

                // GPU energy consumption
                {
                    let metric = get_metric(ENERGY);
                    let p = m.iter().find(|p| p.metric == metric).unwrap();
                    assert_eq!(p.value, WrappedMeasurementValue::F64(123456789.0));
                }

                // GPU memory usage
                {
                    let metric = get_metric(MEMORY);
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
                                assert_eq!(p.value, WrappedMeasurementValue::U64(13443072))
                            }
                            "memory_video_computing" => assert_eq!(p.value, WrappedMeasurementValue::U64(13443072)),
                            e => panic!("Unexpected type {e}"),
                        }
                    }
                }

                // GPU power consumption
                {
                    let metric = get_metric(POWER);
                    let p = m.iter().find(|p| p.metric == metric).unwrap();
                    assert_eq!(p.value, WrappedMeasurementValue::U64(43));
                }

                // GPU temperatures
                {
                    let metric = get_metric(TEMPERATURE);
                    let points: Vec<_> = m.iter().filter(|p| p.metric == metric).collect();

                    for p in points {
                        let attr_type = p
                            .attributes()
                            .find(|(k, _)| *k == "sensor_type")
                            .expect("Missing attribute type")
                            .1
                            .to_string();

                        match attr_type.as_str() {
                            "thermal_global" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            "thermal_hotspot" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            "thermal_high_bandwidth_memory_0" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            "thermal_high_bandwidth_memory_1" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            "thermal_high_bandwidth_memory_2" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            "thermal_high_bandwidth_memory_3" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            "thermal_pci_bus" => assert_eq!(p.value, WrappedMeasurementValue::U64(52)),
                            e => panic!("Unexpected type {e}"),
                        }
                    }
                }

                // GPU voltage consumption
                {
                    let metric = get_metric(VOLTAGE);
                    let p = m.iter().find(|p| p.metric == metric).unwrap();
                    assert_eq!(p.value, WrappedMeasurementValue::U64(830));
                }

                // GPU processes informations
                {
                    let expected_names = ["p1", "p2"];
                    let process_metrics = [
                        PROCESS_MEMORY,
                        PROCESS_ENCODE,
                        PROCESS_GFX,
                        PROCESS_GTT,
                        PROCESS_CPU,
                        PROCESS_VRAM,
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
                                ("p1", PROCESS_MEMORY) => WrappedMeasurementValue::U64(32),
                                ("p1", PROCESS_ENCODE) => WrappedMeasurementValue::U64(64),
                                ("p1", PROCESS_GFX) => WrappedMeasurementValue::U64(5135145681),
                                ("p1", PROCESS_GTT) => WrappedMeasurementValue::U64(256),
                                ("p1", PROCESS_CPU) => WrappedMeasurementValue::U64(1024),
                                ("p1", PROCESS_VRAM) => WrappedMeasurementValue::U64(2048),

                                ("p2", PROCESS_MEMORY) => WrappedMeasurementValue::U64(4096),
                                ("p2", PROCESS_ENCODE) => WrappedMeasurementValue::U64(8192),
                                ("p2", PROCESS_GFX) => WrappedMeasurementValue::U64(16384),
                                ("p2", PROCESS_GTT) => WrappedMeasurementValue::U64(32768),
                                ("p2", PROCESS_CPU) => WrappedMeasurementValue::U64(65536),
                                ("p2", PROCESS_VRAM) => WrappedMeasurementValue::U64(131072),

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
