use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::WrappedMeasurementValue,
    pipeline::naming::SourceName,
    plugin::PluginMetadata,
    test::{RuntimeExpectations, StartupExpectations},
};
use plugin_grace_hopper::{Config, GraceHopperPlugin};
use std::{fs::File, time::Duration};
use std::{io::Write, thread};
use tempfile::tempdir;

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn test_correct_plugin_with_no_data() {
    let root = tempdir().unwrap();
    let root_path = root.path().to_str().unwrap().to_string();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new().expect_source("grace-hopper", "grace-hopper-source");

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .build_and_start()
        .unwrap();

    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn test_correct_plugin_init_with_one_source_empty_value() {
    let root = tempdir().unwrap();

    let root_path = root.path().to_str().unwrap().to_string();
    let file_path_info = root.path().join("hwmon1/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon1/device/power1_average");
    let file_path_interval = root.path().join("hwmon1/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();

    let mut file = File::create(&file_path_info).unwrap();
    let mut _file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "Module Power Socket 0").unwrap();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new()
        .expect_metric::<f64>("energy_consumed", alumet::units::Unit::Joule)
        .expect_source("grace-hopper", "grace-hopper-source");

    let runtime_expectation = RuntimeExpectations::new()
        .test_source(
            SourceName::from_str("grace-hopper", "grace-hopper-source"),
            || {},
            |_m| {},
        )
        .test_source(
            SourceName::from_str("grace-hopper", "grace-hopper-source"),
            || {},
            |m| {
                assert_eq!(m.len(), 5);
                for elm in m {
                    assert!(elm.value == WrappedMeasurementValue::F64(0.0));
                }
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .with_expectations(runtime_expectation)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn test_correct_plugin_init_with_several_sources() {
    let root = tempdir().unwrap();

    let root_path = root.path().to_str().unwrap().to_string();
    let file_path_info = root.path().join("hwmon1/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon1/device/power1_average");
    let file_path_interval = root.path().join("hwmon1/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "Module Power Socket 0").unwrap();
    writeln!(file_avg, "60000000").unwrap();
    writeln!(file_int, "50").unwrap();

    let file_path_info = root.path().join("hwmon2/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon2/device/power1_average");
    let file_path_interval = root.path().join("hwmon2/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "Grace Power Socket 0").unwrap();
    writeln!(file_avg, "62000000").unwrap();

    let file_path_info = root.path().join("hwmon3/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon3/device/power1_average");
    let file_path_interval = root.path().join("hwmon3/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "CPU Power Socket 2").unwrap();
    writeln!(file_avg, "64000000").unwrap();

    let file_path_info = root.path().join("hwmon6/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon6/device/power1_average");
    let file_path_interval = root.path().join("hwmon6/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "SysIO Power Socket 2").unwrap();
    writeln!(file_avg, "67000000").unwrap();
    writeln!(file_int, "77").unwrap();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new()
        .expect_metric::<f64>("energy_consumed", alumet::units::Unit::Joule)
        .expect_source("grace-hopper", "grace-hopper-source");
    let runtime_expectation = RuntimeExpectations::new()
        .test_source(
            SourceName::from_str("grace-hopper", "grace-hopper-source"),
            || {
                thread::sleep(Duration::from_secs(1));
            },
            |_m| {},
        )
        .test_source(
            SourceName::from_str("grace-hopper", "grace-hopper-source"),
            || {},
            |m| {
                for elm in m {
                    if let Some((_, value)) = elm.attributes().find(|(key, _)| *key == "sensor") {
                        // println!("ELM is: {:?}", elm);
                        let kind = if let alumet::measurement::AttributeValue::String(kind) = value {
                            kind
                        } else if let alumet::measurement::AttributeValue::Str(kind) = value {
                            match *kind {
                                "module" => {
                                    assert_eq!(elm.value, WrappedMeasurementValue::F64(60.0));
                                }
                                "grace" => {
                                    assert_eq!(elm.value, WrappedMeasurementValue::F64(62.0))
                                }
                                "cpu" => {
                                    assert_eq!(elm.value, WrappedMeasurementValue::F64(64.0))
                                }
                                "sysio" => {
                                    assert_eq!(elm.value, WrappedMeasurementValue::F64(67.0))
                                }
                                _ => {
                                    println!("Kind is: {}", kind);
                                    assert!(false, "No correct attribute found")
                                }
                            }
                            continue;
                        } else {
                            panic!("bad kind of AttributeValue"); // Panic if it doesn't match
                        };
                        match kind.as_str() {
                            "module" => {
                                if let alumet::resources::Resource::CpuPackage { id } = elm.resource {
                                    if id != 0 {
                                        assert!(false);
                                    }
                                    match elm.value {
                                        WrappedMeasurementValue::F64(value) => {
                                            println!("value is {:?}", value);
                                            assert!(value >= 60.0 && value <= 61.0);
                                        }
                                        WrappedMeasurementValue::U64(_) => {
                                            assert!(false);
                                        }
                                    }
                                }
                            }
                            "grace" => {
                                if let alumet::resources::Resource::CpuPackage { id } = elm.resource {
                                    if id != 0 {
                                        assert!(false);
                                    }
                                    match elm.value {
                                        WrappedMeasurementValue::F64(value) => {
                                            assert!(value >= 62.0 && value <= 63.0);
                                        }
                                        WrappedMeasurementValue::U64(_) => {
                                            assert!(false);
                                        }
                                    }
                                }
                            }
                            "cpu" => {
                                if let alumet::resources::Resource::CpuPackage { id } = elm.resource {
                                    if id != 2 {
                                        assert!(false);
                                    }
                                    match elm.value {
                                        WrappedMeasurementValue::F64(value) => {
                                            assert!(value >= 64.0 && value <= 65.0);
                                        }
                                        WrappedMeasurementValue::U64(_) => {
                                            assert!(false);
                                        }
                                    }
                                }
                            }
                            "sysio" => {
                                if let alumet::resources::Resource::CpuPackage { id } = elm.resource {
                                    if id != 2 {
                                        assert!(false);
                                    }
                                    match elm.value {
                                        WrappedMeasurementValue::F64(value) => {
                                            assert!(value >= 67.0 && value <= 68.0);
                                        }
                                        WrappedMeasurementValue::U64(_) => {
                                            assert!(false);
                                        }
                                    }
                                }
                            }
                            _ => {
                                println!("Kind is: {}", kind);
                                assert!(false, "No correct attribute found")
                            }
                        }
                    }
                }
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .with_expectations(runtime_expectation)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}
